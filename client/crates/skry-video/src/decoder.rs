//! Decodificación H.265/H.264 con FFmpeg.
//!
//! Intenta **decode por hardware** (D3D11VA en Windows, VAAPI en Linux) y cae a
//! **software** si la GPU no está disponible. Con hw, el frame sale en memoria de
//! GPU; se transfiere a memoria de sistema en su formato nativo (NV12) y se
//! entrega tal cual — el renderer sube NV12 directo a la textura (sin pasada de
//! conversión por CPU). El camino software entrega YUV420P. El renderer soporta
//! ambos formatos.
//!
//! Este módulo usa FFI de FFmpeg para el hwaccel (no expuesto por ffmpeg-next),
//! por eso habilita `unsafe_code` que el workspace deja en `warn` por defecto.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicI32, Ordering};

use ffmpeg::frame::Video;
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

/// Decoder de video que acepta los payloads del stream y entrega frames listos
/// para render (NV12 si vino del hardware, YUV420P si fue software).
pub struct Decoder {
    decoder: ffmpeg::decoder::Video,
    hw_active: bool,
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
        Ok(Self { decoder, hw_active })
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
            let ready = self.to_system_frame(&frame)?;
            out.push(DecodedFrame {
                frame: ready,
                pts_us,
            });
        }
        Ok(())
    }

    /// Lleva el frame a memoria de sistema listo para render. Si vino de la GPU,
    /// lo transfiere (queda en NV12, formato nativo del decode hw 8-bit) sin
    /// convertir; el renderer sube NV12 directo. Software ya entrega YUV420P.
    fn to_system_frame(&self, frame: &Video) -> Result<Video, ffmpeg::Error> {
        let hw_fmt = HW_PIX_FMT.load(Ordering::Relaxed);
        let frame_fmt: ffi::AVPixelFormat = frame.format().into();
        let is_hw = self.hw_active && (frame_fmt as i32) == hw_fmt;

        if !is_hw {
            return Ok(frame.clone());
        }

        // Transferir de GPU a memoria de sistema (formato nativo, p.ej. NV12).
        let mut sw = Video::empty();
        // SAFETY: punteros válidos (frame vivo, sw recién creado). transfer_data
        // asigna sw->format/width/height desde el source; fijamos dims explícito
        // como red de seguridad ante implementaciones que no las copien.
        unsafe {
            (*sw.as_mut_ptr()).width = (*frame.as_ptr()).width;
            (*sw.as_mut_ptr()).height = (*frame.as_ptr()).height;
            let ret = ffi::av_hwframe_transfer_data(sw.as_mut_ptr(), frame.as_ptr(), 0);
            if ret < 0 {
                return Err(ffmpeg::Error::from(ret));
            }
            // av_hwframe_transfer_data no copia el pts; lo propagamos.
            (*sw.as_mut_ptr()).pts = (*frame.as_ptr()).pts;
        }
        Ok(sw)
    }
}

/// Callback de FFmpeg para elegir el formato de píxel. Si el formato de hardware
/// está en la lista ofrecida, lo devuelve (activa hwaccel); si no, el primero
/// (software). La lista termina en `AV_PIX_FMT_NONE`.
unsafe extern "C" fn get_hw_format(
    _ctx: *mut ffi::AVCodecContext,
    mut fmts: *const ffi::AVPixelFormat,
) -> ffi::AVPixelFormat {
    if fmts.is_null() {
        return ffi::AVPixelFormat::AV_PIX_FMT_NONE;
    }
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
            // av_buffer_ref puede fallar (null) con memoria al límite: sin esto se
            // instalaría get_format con hw_device_ctx null -> decode hw inválido.
            let device_ref = ffi::av_buffer_ref(hw_device_ctx);
            if device_ref.is_null() {
                ffi::av_buffer_unref(&mut hw_device_ctx);
                continue;
            }
            (*ctx).hw_device_ctx = device_ref;
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
