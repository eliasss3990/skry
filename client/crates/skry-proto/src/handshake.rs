use std::io::{Read, Write};

use crate::codec::Codec;
use crate::error::{ProtoError, Result};
use crate::wire::{read_string, read_u16, read_u8, write_string, write_u16, write_u8};
use crate::{MAGIC, PROTOCOL_VERSION};

/// Primer mensaje del canal de video: el server anuncia los parámetros
/// efectivos de la sesión apenas se acepta la conexión.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    pub codec: Codec,
    pub width: u16,
    pub height: u16,
    pub device_name: String,
}

impl Handshake {
    /// Escribe el handshake completo (magic + versión + cuerpo).
    pub fn write<W: Write>(&self, w: &mut W) -> Result<()> {
        w.write_all(&MAGIC)?;
        write_u16(w, PROTOCOL_VERSION)?;
        write_u8(w, self.codec.to_u8())?;
        write_u16(w, self.width)?;
        write_u16(w, self.height)?;
        write_string(w, &self.device_name)?;
        Ok(())
    }

    /// Lee y valida el handshake. Falla si el magic o la versión no coinciden,
    /// de modo que un cliente y un server incompatibles no avancen a ciegas.
    pub fn read<R: Read>(r: &mut R) -> Result<Handshake> {
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if magic != MAGIC {
            return Err(ProtoError::BadMagic(magic));
        }
        let version = read_u16(r)?;
        if version != PROTOCOL_VERSION {
            return Err(ProtoError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                found: version,
            });
        }
        let codec = Codec::from_u8(read_u8(r)?)?;
        let width = read_u16(r)?;
        let height = read_u16(r)?;
        let device_name = read_string(r)?;
        Ok(Handshake {
            codec,
            width,
            height,
            device_name,
        })
    }
}
