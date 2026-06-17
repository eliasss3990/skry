use std::fmt;
use std::io;

/// Errores del protocolo skry.
///
/// Separa dos familias: fallas de I/O (el socket se cerró, etc.) y violaciones
/// del protocolo en sí (datos que no respetan el formato). Nunca se interpreta
/// basura como dato válido: cualquier desvío produce un error explícito.
#[derive(Debug)]
pub enum ProtoError {
    /// Falla de I/O subyacente al leer o escribir.
    Io(io::Error),
    /// El magic del handshake no es "SKRY".
    BadMagic([u8; 4]),
    /// Versión de protocolo incompatible entre cliente y server.
    VersionMismatch { expected: u16, found: u16 },
    /// Discriminante de enum desconocido (códec, marcha, tag de mensaje).
    UnknownDiscriminant { kind: &'static str, value: u8 },
    /// Un campo de longitud excede el máximo permitido.
    LengthExceeded {
        kind: &'static str,
        len: u64,
        max: u64,
    },
    /// Cadena UTF-8 inválida.
    InvalidUtf8,
    /// Al escribir un frame, `header.len` no coincide con el tamaño del payload.
    FrameLenMismatch { header_len: u32, payload_len: usize },
    /// No se pudo reservar memoria para un buffer del tamaño declarado.
    /// Convierte un fallo de asignación en un error con gracia en vez de abortar.
    AllocFailed { kind: &'static str, bytes: usize },
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::Io(e) => write!(f, "error de I/O: {e}"),
            ProtoError::BadMagic(got) => {
                write!(f, "magic de handshake invalido: {got:02X?} (se esperaba \"SKRY\")")
            }
            ProtoError::VersionMismatch { expected, found } => write!(
                f,
                "version de protocolo incompatible: cliente habla v{expected}, server habla v{found}"
            ),
            ProtoError::UnknownDiscriminant { kind, value } => {
                write!(f, "valor desconocido para {kind}: {value}")
            }
            ProtoError::LengthExceeded { kind, len, max } => {
                write!(f, "longitud de {kind} fuera de rango: {len} (max {max})")
            }
            ProtoError::InvalidUtf8 => write!(f, "cadena UTF-8 invalida"),
            ProtoError::FrameLenMismatch {
                header_len,
                payload_len,
            } => write!(
                f,
                "header.len ({header_len}) no coincide con el payload ({payload_len})"
            ),
            ProtoError::AllocFailed { kind, bytes } => {
                write!(f, "no se pudo reservar {bytes} bytes para {kind}")
            }
        }
    }
}

impl std::error::Error for ProtoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProtoError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ProtoError {
    fn from(e: io::Error) -> Self {
        ProtoError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, ProtoError>;
