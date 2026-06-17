use crate::error::{ProtoError, Result};

/// Códec de video negociado entre cliente y server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    H264,
    H265,
}

impl Codec {
    pub fn to_u8(self) -> u8 {
        match self {
            Codec::H264 => 0,
            Codec::H265 => 1,
        }
    }

    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Codec::H264),
            1 => Ok(Codec::H265),
            _ => Err(ProtoError::UnknownDiscriminant {
                kind: "Codec",
                value: v,
            }),
        }
    }

    /// Nombre del códec tal como lo conoce FFmpeg y la CLI.
    pub fn as_str(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::H265 => "h265",
        }
    }
}

impl From<Codec> for u8 {
    fn from(c: Codec) -> u8 {
        c.to_u8()
    }
}

impl TryFrom<u8> for Codec {
    type Error = ProtoError;
    fn try_from(v: u8) -> Result<Codec> {
        Codec::from_u8(v)
    }
}
