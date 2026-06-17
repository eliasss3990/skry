use std::io::{Read, Write};

use crate::error::{ProtoError, Result};
use crate::gear::Gear;
use crate::wire::{read_string, read_u16, read_u32, read_u64, read_u8};
use crate::wire::{write_string, write_u16, write_u32, write_u64, write_u8};

// Tags del cliente (sin bit alto).
const TAG_SET_GEAR: u8 = 0x01;
const TAG_SET_BITRATE: u8 = 0x02;
const TAG_PING: u8 = 0x03;
const TAG_STOP: u8 = 0x04;

// Tags del server (con bit alto 0x80).
const TAG_PONG: u8 = 0x81;
const TAG_TELEMETRY: u8 = 0x82;
const TAG_GEAR_CHANGED: u8 = 0x83;
const TAG_ERROR: u8 = 0x84;

/// Mensaje del cliente hacia el server por el canal de control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMessage {
    /// Pedir cambio de marcha.
    SetGear(Gear),
    /// Forzar un bitrate concreto (bits/s).
    SetBitrate(u32),
    /// Sonda de latencia; el server responde `Pong` con el mismo `seq`.
    Ping(u32),
    /// Pedir el cierre de la sesión.
    Stop,
}

impl ClientMessage {
    pub fn write<W: Write>(&self, w: &mut W) -> Result<()> {
        match self {
            ClientMessage::SetGear(g) => {
                write_u8(w, TAG_SET_GEAR)?;
                write_u8(w, g.to_u8())?;
            }
            ClientMessage::SetBitrate(b) => {
                write_u8(w, TAG_SET_BITRATE)?;
                write_u32(w, *b)?;
            }
            ClientMessage::Ping(seq) => {
                write_u8(w, TAG_PING)?;
                write_u32(w, *seq)?;
            }
            ClientMessage::Stop => write_u8(w, TAG_STOP)?,
        }
        Ok(())
    }

    pub fn read<R: Read>(r: &mut R) -> Result<ClientMessage> {
        let tag = read_u8(r)?;
        match tag {
            TAG_SET_GEAR => Ok(ClientMessage::SetGear(Gear::from_u8(read_u8(r)?)?)),
            TAG_SET_BITRATE => Ok(ClientMessage::SetBitrate(read_u32(r)?)),
            TAG_PING => Ok(ClientMessage::Ping(read_u32(r)?)),
            TAG_STOP => Ok(ClientMessage::Stop),
            _ => Err(ProtoError::UnknownDiscriminant {
                kind: "ClientMessage",
                value: tag,
            }),
        }
    }
}

/// Telemetría que el server reporta periódicamente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Telemetry {
    pub encoded_frames: u64,
    pub dropped_frames: u64,
    pub bitrate: u32,
}

/// Mensaje del server hacia el cliente por el canal de control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMessage {
    /// Respuesta a un `Ping`, con el mismo `seq`.
    Pong(u32),
    /// Telemetría de codificación.
    Telemetry(Telemetry),
    /// Confirmación de la marcha efectiva tras un cambio.
    GearChanged(Gear),
    /// Error reportado por el server.
    Error { code: u16, message: String },
}

impl ServerMessage {
    pub fn write<W: Write>(&self, w: &mut W) -> Result<()> {
        match self {
            ServerMessage::Pong(seq) => {
                write_u8(w, TAG_PONG)?;
                write_u32(w, *seq)?;
            }
            ServerMessage::Telemetry(t) => {
                write_u8(w, TAG_TELEMETRY)?;
                write_u64(w, t.encoded_frames)?;
                write_u64(w, t.dropped_frames)?;
                write_u32(w, t.bitrate)?;
            }
            ServerMessage::GearChanged(g) => {
                write_u8(w, TAG_GEAR_CHANGED)?;
                write_u8(w, g.to_u8())?;
            }
            ServerMessage::Error { code, message } => {
                write_u8(w, TAG_ERROR)?;
                write_u16(w, *code)?;
                write_string(w, message)?;
            }
        }
        Ok(())
    }

    pub fn read<R: Read>(r: &mut R) -> Result<ServerMessage> {
        let tag = read_u8(r)?;
        match tag {
            TAG_PONG => Ok(ServerMessage::Pong(read_u32(r)?)),
            TAG_TELEMETRY => Ok(ServerMessage::Telemetry(Telemetry {
                encoded_frames: read_u64(r)?,
                dropped_frames: read_u64(r)?,
                bitrate: read_u32(r)?,
            })),
            TAG_GEAR_CHANGED => Ok(ServerMessage::GearChanged(Gear::from_u8(read_u8(r)?)?)),
            TAG_ERROR => Ok(ServerMessage::Error {
                code: read_u16(r)?,
                message: read_string(r)?,
            }),
            _ => Err(ProtoError::UnknownDiscriminant {
                kind: "ServerMessage",
                value: tag,
            }),
        }
    }
}
