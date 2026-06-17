//! Lectura y escritura de primitivos en *big-endian* (orden de red).
//!
//! Operan sobre cualquier `Read`/`Write`, de modo que el mismo código sirve
//! para un `TcpStream` o un `Vec<u8>` en los tests. Las cadenas se codifican
//! como `u16` de longitud seguido de los bytes UTF-8.

use std::io::{Read, Write};

use crate::error::{ProtoError, Result};

/// Tope de longitud para cadenas (nombre de dispositivo, mensajes de error).
/// Es el máximo que entra en el prefijo de longitud `u16` del formato.
pub const MAX_STRING_BYTES: usize = u16::MAX as usize;

pub fn write_u8<W: Write>(w: &mut W, v: u8) -> Result<()> {
    w.write_all(&[v])?;
    Ok(())
}

pub fn read_u8<R: Read>(r: &mut R) -> Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

pub fn write_u16<W: Write>(w: &mut W, v: u16) -> Result<()> {
    w.write_all(&v.to_be_bytes())?;
    Ok(())
}

pub fn read_u16<R: Read>(r: &mut R) -> Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_be_bytes(b))
}

pub fn write_u32<W: Write>(w: &mut W, v: u32) -> Result<()> {
    w.write_all(&v.to_be_bytes())?;
    Ok(())
}

pub fn read_u32<R: Read>(r: &mut R) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}

pub fn write_u64<W: Write>(w: &mut W, v: u64) -> Result<()> {
    w.write_all(&v.to_be_bytes())?;
    Ok(())
}

pub fn read_u64<R: Read>(r: &mut R) -> Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_be_bytes(b))
}

pub fn write_string<W: Write>(w: &mut W, s: &str) -> Result<()> {
    let bytes = s.as_bytes();
    if bytes.len() > MAX_STRING_BYTES {
        return Err(ProtoError::LengthExceeded {
            kind: "string",
            len: bytes.len() as u64,
            max: MAX_STRING_BYTES as u64,
        });
    }
    // El tope garantiza que el cast a u16 no trunca: MAX_STRING_BYTES <= u16::MAX.
    write_u16(w, bytes.len() as u16)?;
    w.write_all(bytes)?;
    Ok(())
}

pub fn read_string<R: Read>(r: &mut R) -> Result<String> {
    let len = read_u16(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|_| ProtoError::InvalidUtf8)
}

const _: () = assert!(MAX_STRING_BYTES <= u16::MAX as usize);
