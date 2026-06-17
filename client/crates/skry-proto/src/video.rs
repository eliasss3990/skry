use std::io::{Read, Write};

use crate::error::{ProtoError, Result};
use crate::wire::{read_u32, read_u64, read_u8, write_u32, write_u64, write_u8};

/// Tope de tamaño de un frame, para frenar lecturas absurdas ante corrupción.
pub const MAX_FRAME_BYTES: u32 = 16 * 1024 * 1024;

const FLAG_KEYFRAME: u8 = 0x01;
const FLAG_CONFIG: u8 = 0x02;

/// Cabecera de un paquete de frame en el canal de video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Presentation timestamp en microsegundos (reloj monotónico del server).
    pub pts: u64,
    /// El frame es un keyframe (IDR).
    pub keyframe: bool,
    /// El payload es configuración (SPS/PPS/VPS), no un frame visible.
    pub config: bool,
    /// Longitud del payload en bytes.
    pub len: u32,
}

impl FrameHeader {
    fn flags(&self) -> u8 {
        let mut f = 0u8;
        if self.keyframe {
            f |= FLAG_KEYFRAME;
        }
        if self.config {
            f |= FLAG_CONFIG;
        }
        f
    }

    pub fn write<W: Write>(&self, w: &mut W) -> Result<()> {
        if self.len > MAX_FRAME_BYTES {
            return Err(ProtoError::LengthExceeded {
                kind: "frame",
                len: self.len as u64,
                max: MAX_FRAME_BYTES as u64,
            });
        }
        write_u64(w, self.pts)?;
        write_u8(w, self.flags())?;
        write_u32(w, self.len)?;
        Ok(())
    }

    pub fn read<R: Read>(r: &mut R) -> Result<FrameHeader> {
        let pts = read_u64(r)?;
        let flags = read_u8(r)?;
        let len = read_u32(r)?;
        if len > MAX_FRAME_BYTES {
            return Err(ProtoError::LengthExceeded {
                kind: "frame",
                len: len as u64,
                max: MAX_FRAME_BYTES as u64,
            });
        }
        Ok(FrameHeader {
            pts,
            keyframe: flags & FLAG_KEYFRAME != 0,
            config: flags & FLAG_CONFIG != 0,
            len,
        })
    }
}

/// Lee un frame completo (cabecera + payload) en un `Vec<u8>` nuevo.
///
/// Conveniencia para el receptor. El payload se acota por `MAX_FRAME_BYTES`
/// dentro de `FrameHeader::read`, así que la reserva nunca es desmedida.
pub fn read_frame<R: Read>(r: &mut R) -> Result<(FrameHeader, Vec<u8>)> {
    let header = FrameHeader::read(r)?;
    let mut payload = vec![0u8; header.len as usize];
    r.read_exact(&mut payload)?;
    Ok((header, payload))
}

/// Escribe un frame completo (cabecera + payload).
pub fn write_frame<W: Write>(w: &mut W, header: &FrameHeader, payload: &[u8]) -> Result<()> {
    debug_assert_eq!(header.len as usize, payload.len());
    header.write(w)?;
    w.write_all(payload)?;
    Ok(())
}
