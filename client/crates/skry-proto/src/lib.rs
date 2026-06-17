//! Protocolo de wire de skry.
//!
//! Define el contrato binario entre el cliente (Rust) y el server (Kotlin):
//! handshake, mensajes de control y framing de video. La codificación es
//! explícita en big-endian (orden de red) para ser portable entre lenguajes.
//!
//! Ver `docs/protocol.md` para la especificación completa.

pub mod codec;
pub mod control;
pub mod error;
pub mod gear;
pub mod handshake;
pub mod video;
pub mod wire;

pub use codec::Codec;
pub use control::{ClientMessage, ServerMessage, Telemetry};
pub use error::{ProtoError, Result};
pub use gear::Gear;
pub use handshake::Handshake;
pub use video::{read_frame, write_frame, FrameHeader, MAX_FRAME_BYTES};

/// Magic que abre el handshake: los bytes ASCII de "SKRY".
pub const MAGIC: [u8; 4] = *b"SKRY";

/// Versión del protocolo. Cliente y server sólo operan si coincide.
pub const PROTOCOL_VERSION: u16 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_round_trip() {
        let hs = Handshake {
            codec: Codec::H265,
            width: 1440,
            height: 3120,
            device_name: "SM-S928B".to_string(),
        };
        let mut buf = Vec::new();
        hs.write(&mut buf).unwrap();
        let back = Handshake::read(&mut &buf[..]).unwrap();
        assert_eq!(hs, back);
    }

    #[test]
    fn handshake_rejects_bad_magic() {
        let mut buf = b"XXXX".to_vec();
        buf.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
        let err = Handshake::read(&mut &buf[..]).unwrap_err();
        assert!(matches!(err, ProtoError::BadMagic(_)));
    }

    #[test]
    fn handshake_rejects_version_mismatch() {
        let mut buf = MAGIC.to_vec();
        buf.extend_from_slice(&(PROTOCOL_VERSION + 1).to_be_bytes());
        let err = Handshake::read(&mut &buf[..]).unwrap_err();
        assert!(matches!(
            err,
            ProtoError::VersionMismatch { expected, found }
                if expected == PROTOCOL_VERSION && found == PROTOCOL_VERSION + 1
        ));
    }

    #[test]
    fn frame_round_trip() {
        let header = FrameHeader {
            pts: 1_234_567,
            keyframe: true,
            config: false,
            len: 4,
        };
        let payload = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let mut buf = Vec::new();
        write_frame(&mut buf, &header, &payload).unwrap();
        let (back_header, back_payload) = read_frame(&mut &buf[..]).unwrap();
        assert_eq!(header, back_header);
        assert_eq!(payload, back_payload);
    }

    #[test]
    fn frame_config_flag_preserved() {
        let header = FrameHeader {
            pts: 0,
            keyframe: false,
            config: true,
            len: 0,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &header, &[]).unwrap();
        let (back, _) = read_frame(&mut &buf[..]).unwrap();
        assert!(back.config);
        assert!(!back.keyframe);
    }

    #[test]
    fn frame_rejects_oversized_len() {
        // Cabecera con len por encima del máximo: debe rechazarse en lectura.
        let mut buf = Vec::new();
        buf.extend_from_slice(&0u64.to_be_bytes()); // pts
        buf.push(0); // flags
        buf.extend_from_slice(&(MAX_FRAME_BYTES + 1).to_be_bytes()); // len
        let err = FrameHeader::read(&mut &buf[..]).unwrap_err();
        assert!(matches!(err, ProtoError::LengthExceeded { .. }));
    }

    #[test]
    fn client_messages_round_trip() {
        let msgs = [
            ClientMessage::SetGear(Gear::High),
            ClientMessage::SetBitrate(8_000_000),
            ClientMessage::Ping(42),
            ClientMessage::Stop,
        ];
        for m in msgs {
            let mut buf = Vec::new();
            m.write(&mut buf).unwrap();
            let back = ClientMessage::read(&mut &buf[..]).unwrap();
            assert_eq!(m, back);
        }
    }

    #[test]
    fn server_messages_round_trip() {
        let msgs = [
            ServerMessage::Pong(42),
            ServerMessage::Telemetry(Telemetry {
                encoded_frames: 1000,
                dropped_frames: 3,
                bitrate: 6_000_000,
            }),
            ServerMessage::GearChanged(Gear::Low),
            ServerMessage::Error {
                code: 7,
                message: "encoder no disponible".to_string(),
            },
        ];
        for m in msgs {
            let mut buf = Vec::new();
            m.write(&mut buf).unwrap();
            let back = ServerMessage::read(&mut &buf[..]).unwrap();
            assert_eq!(m, back);
        }
    }

    #[test]
    fn unknown_tag_is_protocol_error() {
        let err = ClientMessage::read(&mut &[0xFFu8][..]).unwrap_err();
        assert!(matches!(err, ProtoError::UnknownDiscriminant { .. }));
    }

    #[test]
    fn gear_shifts() {
        assert_eq!(Gear::Low.upshift(), Some(Gear::Mid));
        assert_eq!(Gear::High.upshift(), None);
        assert_eq!(Gear::Low.downshift(), None);
        assert_eq!(Gear::High.downshift(), Some(Gear::Mid));
        assert_eq!(Gear::from_fps(60), Gear::Low);
        assert_eq!(Gear::from_fps(144), Gear::High);
        assert_eq!(Gear::from_fps(90), Gear::Mid);
    }

    #[test]
    fn codec_round_trip() {
        for c in [Codec::H264, Codec::H265] {
            assert_eq!(Codec::from_u8(c.to_u8()).unwrap(), c);
        }
        assert!(Codec::from_u8(99).is_err());
    }
}
