use std::io::{Read, Write};

use crate::error::{ProtoError, Result};
use crate::wire::{read_u8, write_u8};

/// Tipo de canal, declarado por el cliente como **primer byte** de cada socket
/// apenas conecta.
///
/// El server enruta cada conexión por este byte, no por el orden de aceptación:
/// así el emparejamiento de canales es robusto ante cualquier transporte (túnel
/// ADB hoy, Wi-Fi Direct / LAN mañana), donde el orden de llegada puede no ser
/// determinista.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    /// Canal de video (server → cliente). Tras este byte sigue el handshake.
    Video,
    /// Canal de control (bidireccional).
    Control,
}

impl StreamType {
    pub fn to_u8(self) -> u8 {
        match self {
            StreamType::Video => 0x00,
            StreamType::Control => 0x01,
        }
    }

    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0x00 => Ok(StreamType::Video),
            0x01 => Ok(StreamType::Control),
            _ => Err(ProtoError::UnknownDiscriminant {
                kind: "StreamType",
                value: v,
            }),
        }
    }

    pub fn write<W: Write>(self, w: &mut W) -> Result<()> {
        write_u8(w, self.to_u8())
    }

    pub fn read<R: Read>(r: &mut R) -> Result<StreamType> {
        StreamType::from_u8(read_u8(r)?)
    }
}
