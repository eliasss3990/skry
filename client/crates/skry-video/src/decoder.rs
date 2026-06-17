//! Decodificación H.265/H.264 con FFmpeg (software por ahora; hw accel a futuro).

use ffmpeg::codec;
use ffmpeg::frame::Video;
use ffmpeg_next as ffmpeg;

use skry_proto::Codec;

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

/// Decoder de video que acepta los payloads del stream y entrega frames YUV.
pub struct Decoder {
    decoder: ffmpeg::decoder::Video,
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoder").finish_non_exhaustive()
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
        let context = codec::context::Context::new();
        let decoder = context.decoder().open_as(codec)?.video()?;
        Ok(Self { decoder })
    }

    /// Decodifica un payload (con su pts en µs) y agrega los frames listos a `out`.
    /// Un encoder de baja latencia sin B-frames entrega ~1 frame por payload.
    pub fn decode(
        &mut self,
        payload: &[u8],
        pts_us: i64,
        out: &mut Vec<DecodedFrame>,
    ) -> Result<(), ffmpeg::Error> {
        let mut packet = ffmpeg::Packet::copy(payload);
        packet.set_pts(Some(pts_us));
        packet.set_dts(Some(pts_us));
        self.decoder.send_packet(&packet)?;
        self.drain(out)
    }

    fn drain(&mut self, out: &mut Vec<DecodedFrame>) -> Result<(), ffmpeg::Error> {
        // Idiom de ffmpeg-next: receive_frame devuelve Err en EAGAIN/EOF, así
        // que el while corta naturalmente cuando no hay más frames listos.
        let mut frame = Video::empty();
        while self.decoder.receive_frame(&mut frame).is_ok() {
            let pts_us = frame.pts().unwrap_or(0);
            out.push(DecodedFrame {
                frame: frame.clone(),
                pts_us,
            });
        }
        Ok(())
    }
}
