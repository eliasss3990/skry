//! Decodificación H.265/H.264 con FFmpeg.
//!
//! Intenta **decode por hardware** (D3D11VA en Windows, VAAPI en Linux) y cae a
//! **software** si la GPU no está disponible. Con hw, el frame sale en memoria
//! de GPU (NV12); se transfiere a memoria de sistema y se convierte a YUV420P
//! con un scaler, así el renderer ve siempre YUV420P sin importar el camino.
//!
//! Este módulo usa FFI de FFmpeg para el hwaccel (no expuesto por ffmpeg-next),
//! por eso habilita `unsafe_code` que el workspace deja en `warn` por defecto.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicI32, Ordering};

use ffmpeg::format::Pixel;
use ffmpeg::frame::Video;
use ffmpeg::software::scaling::context::Context as Scaler;
use ffmpeg::software::scaling::flag::Flags;
use ffmpeg::{codec, ffi};
use ffmpeg_next as ffmpeg;

use skry_proto::Codec;

/// Formato de píxel de hardware elegido (lo lee el callback `get_format`).
/// `AV_PIX_FMT_NONE` (-1) significa "sin hwaccel".
static HW_PIX_FMT: AtomicI32 = AtomicI32::new(ffi::AVPixelFormat::AV_PIX_FMT_NONE as i32);

/// Un frame decodificado, con su pts en microsegundos (reloj del server).
pub struct DecodedFrame {
    pub frame: Video,
    pub pts_us: i64,
}

impl std::fmt::Debug for DecodedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedFrame")
            .field("pts_us", &self.pts_us)
            .field("width", &self.frame.width())
            .field("height", &self.frame.height())
            .finish_non_exhaustive()
    }
}

/// Decoder de video que acepta los payloads del stream y entrega frames YUV420P.
pub struct Decoder {
    decoder: ffmpeg::decoder::Video,
    hw_active: bool,
    /// Conversor del formato de hardware (NV12) a YUV420P, creado al primer frame.
    scaler: Option<Scaler>,
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoder")
            .field("hw_active", &self.hw_active)
            .finish_non_exhaustive()
    }
}

impl Decoder {
    pub fn new(codec_kind: Codec) -> Result<Self, ffmpeg::Error> {
        ffmpeg::init()?;
        let id = match codec_kind {
            Codec::H264 => codec::Id::H264,
            Codec::H265 => codec::Id::HEVC,
        };
        let codec = ffmpeg::decoder::find(id).ok_or(ffmpeg::Error::DecoderNotFound)?;

        let mut context = codec::context::Context::new();
        // Intentar enganchar un device de hardware antes de abrir el codec.
        let hw_active = unsafe { try_init_hwaccel(context.as_mut_ptr()) };
        if hw_active {
            eprintln!("[skry-video] decode por hardware activo");
        } else {
            eprintln!("[skry-video] decode por software (sin hwaccel)");
        }

        let decoder = context.decoder().open_as(codec)?.video()?;
        Ok(Self {
            decoder,
            hw_active,
            scaler: None,
        })
    }

    /// Decodifica un payload (con su pts en µs) y agrega los frames a `out`.
    pub fn decode(
        &mut self,
        payload: &[u8],
        pts_us: i64,
        out: &mut Vec<DecodedFrame>,
    ) -> Result<(), ffmpeg::Error> {
        let mut packet = ffmpeg::Packet::copy(payload);
        packet.set_pts(Some(pts_us));
        packet.set_dts(Some(pts_us));
        // Idiom estándar: drenar lo pendiente, enviar, drenar lo nuevo.
        self.drain(out)?;
        self.decoder.send_packet(&packet)?;
        self.drain(out)
    }

    fn drain(&mut self, out: &mut Vec<DecodedFrame>) -> Result<(), ffmpeg::Error> {
        let mut frame = Video::empty();
        while self.decoder.receive_frame(&mut frame).is_ok() {
            let pts_us = frame.pts().unwrap_or(0);
            let yuv = self.make_yuv420p(&frame)?;
            out.push(DecodedFrame { frame: yuv, pts_us });
        }
        Ok(())
    }

    /// Devuelve el frame en YUV420P. Si vino de la GPU, lo transfiere a memoria
    /// de sistema (queda en NV12) y lo convierte con el scaler.
    fn make_yuv420p(&mut self, frame: &Video) -> Result<Video, ffmpeg::Error> {
        let hw_fmt = HW_PIX_FMT.load(Ordering::Relaxed);
        let frame_fmt: ffi::AVPixelFormat = frame.format().into();
        let is_hw = self.hw_active && (frame_fmt as i32) == hw_fmt;

        let sw_frame = if is_hw {
            // Transferir de GPU a memoria de sistema (formato nativo, p.ej. NV12).
            let mut sw = Video::empty();
            unsafe {
                let ret = ffi::av_hwframe_transfer_data(sw.as_mut_ptr(), frame.as_ptr(), 0);
                if ret < 0 {
                    return Err(ffmpeg::Error::from(ret));
                }
                // av_hwframe_transfer_data no copia el pts; lo propagamos.
                (*sw.as_mut_ptr()).pts = (*frame.as_ptr()).pts;
            }
            sw
        } else {
            frame.clone()
        };

        // Si ya es YUV420P (camino software típico), no convertir.
        if sw_frame.format() == Pixel::YUV420P {
            return Ok(sw_frame);
        }

        // Convertir (NV12 u otro) -> YUV420P con un scaler reutilizable.
        let (w, h) = (sw_frame.width(), sw_frame.height());
        let scaler = self.get_scaler(sw_frame.format(), w, h)?;
        let mut yuv = Video::empty();
        scaler.run(&sw_frame, &mut yuv)?;
        unsafe {
            (*yuv.as_mut_ptr()).pts = (*sw_frame.as_ptr()).pts;
        }
        Ok(yuv)
    }

    fn get_scaler(&mut self, src: Pixel, w: u32, h: u32) -> Result<&mut Scaler, ffmpeg::Error> {
        if self.scaler.is_none() {
            self.scaler = Some(Scaler::get(
                src,
                w,
                h,
                Pixel::YUV420P,
                w,
                h,
                Flags::BILINEAR,
            )?);
        }
        Ok(self.scaler.as_mut().unwrap())
    }
}

/// Callback de FFmpeg para elegir el formato de píxel. Si el formato de hardware
/// está en la lista ofrecida, lo devuelve (activa hwaccel); si no, el primero
/// (software). La lista termina en `AV_PIX_FMT_NONE`.
unsafe extern "C" fn get_hw_format(
    _ctx: *mut ffi::AVCodecContext,
    mut fmts: *const ffi::AVPixelFormat,
) -> ffi::AVPixelFormat {
    let want = HW_PIX_FMT.load(Ordering::Relaxed);
    let mut first = ffi::AVPixelFormat::AV_PIX_FMT_NONE;
    while unsafe { *fmts } != ffi::AVPixelFormat::AV_PIX_FMT_NONE {
        let fmt = unsafe { *fmts };
        if first == ffi::AVPixelFormat::AV_PIX_FMT_NONE {
            first = fmt;
        }
        if fmt as i32 == want {
            return fmt;
        }
        fmts = unsafe { fmts.add(1) };
    }
    first
}

/// Intenta crear un device de hardware y engancharlo al contexto. Devuelve true
/// si quedó activo. Orden de preferencia por plataforma; si ninguno anda, software.
unsafe fn try_init_hwaccel(ctx: *mut ffi::AVCodecContext) -> bool {
    #[cfg(windows)]
    let types = [
        ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
        ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_DXVA2,
    ];
    #[cfg(not(windows))]
    let types = [
        ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
        ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VDPAU,
    ];

    for &ty in &types {
        let mut hw_device_ctx: *mut ffi::AVBufferRef = std::ptr::null_mut();
        let ret = unsafe {
            ffi::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                ty,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            )
        };
        if ret < 0 || hw_device_ctx.is_null() {
            continue;
        }
        // Formato de píxel de hardware para este tipo de device.
        let hw_pix = hw_pix_fmt_for(ty);
        if hw_pix == ffi::AVPixelFormat::AV_PIX_FMT_NONE {
            unsafe { ffi::av_buffer_unref(&mut hw_device_ctx) };
            continue;
        }
        HW_PIX_FMT.store(hw_pix as i32, Ordering::Relaxed);
        unsafe {
            (*ctx).hw_device_ctx = ffi::av_buffer_ref(hw_device_ctx);
            (*ctx).get_format = Some(get_hw_format);
            ffi::av_buffer_unref(&mut hw_device_ctx);
        }
        return true;
    }
    false
}

fn hw_pix_fmt_for(ty: ffi::AVHWDeviceType) -> ffi::AVPixelFormat {
    use ffi::AVHWDeviceType::*;
    use ffi::AVPixelFormat::*;
    match ty {
        AV_HWDEVICE_TYPE_D3D11VA => AV_PIX_FMT_D3D11,
        AV_HWDEVICE_TYPE_DXVA2 => AV_PIX_FMT_DXVA2_VLD,
        AV_HWDEVICE_TYPE_VAAPI => AV_PIX_FMT_VAAPI,
        AV_HWDEVICE_TYPE_VDPAU => AV_PIX_FMT_VDPAU,
        _ => AV_PIX_FMT_NONE,
    }
}
