//! Errores del módulo ADB, alineados con el catálogo de resiliencia.
//!
//! Cada variante lleva la información para construir un mensaje accionable
//! (diagnóstico + solución) tal como exige `docs/resilience.md`.

use std::fmt;

use crate::model::Device;

#[derive(Debug)]
pub enum AdbError {
    /// El binario `adb` no está en el PATH.
    AdbNotFound,
    /// No hay ningún dispositivo conectado (caso A de resiliencia).
    NoDevice,
    /// Hay varios dispositivos y no se especificó `--serial` (caso B).
    AmbiguousDevice { devices: Vec<Device> },
    /// El serial pedido por el usuario no está entre los conectados.
    SerialNotFound {
        serial: String,
        available: Vec<Device>,
    },
    /// El dispositivo está conectado pero no autorizado (caso C).
    Unauthorized { serial: String },
    /// Sin permisos de acceso al USB (Linux sin reglas udev).
    NoPermissions { serial: String },
    /// El dispositivo está en un estado no operable (offline, etc.).
    NotReady { serial: String, state: String },
    /// Un comando adb terminó con código de error.
    CommandFailed {
        args: Vec<String>,
        code: Option<i32>,
        stderr: String,
    },
    /// Falla de I/O al invocar adb.
    Io(std::io::Error),
}

impl fmt::Display for AdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdbError::AdbNotFound => write!(
                f,
                "no se encontro 'adb' en el PATH.\n-> Solucion: instala las Android Platform Tools y asegurate de que 'adb' este en el PATH."
            ),
            AdbError::NoDevice => write!(
                f,
                "no se detecto ningun dispositivo Android.\n-> Solucion: conecta tu celular (USB o Wi-Fi) y verifica que la 'Depuracion' este activa."
            ),
            AdbError::AmbiguousDevice { devices } => {
                writeln!(
                    f,
                    "multiples dispositivos detectados. No se cual usar.\n-> Solucion: volve a ejecutar especificando el serial: skry --serial <ID>\n\nDispositivos disponibles:"
                )?;
                for d in devices {
                    writeln!(f, "  - {}", d.label())?;
                }
                Ok(())
            }
            AdbError::SerialNotFound { serial, available } => {
                writeln!(
                    f,
                    "el serial '{serial}' no esta entre los dispositivos conectados.\n\nDispositivos disponibles:"
                )?;
                for d in available {
                    writeln!(f, "  - {}", d.label())?;
                }
                Ok(())
            }
            AdbError::Unauthorized { serial } => write!(
                f,
                "dispositivo '{serial}' bloqueado (no autorizado).\n-> Solucion: mira la pantalla del celular y acepta el cartel 'Permitir depuracion USB'."
            ),
            AdbError::NoPermissions { serial } => write!(
                f,
                "sin permisos para acceder al dispositivo '{serial}' por USB.\n-> Solucion (Linux): instala las reglas udev de Android (paquete android-sdk-platform-tools-common) o agrega una regla para el vendor del dispositivo, despues 'adb kill-server' y reconecta."
            ),
            AdbError::NotReady { serial, state } => write!(
                f,
                "dispositivo '{serial}' en estado '{state}', no operable.\n-> Solucion: reconecta el dispositivo o reinicia la depuracion."
            ),
            AdbError::CommandFailed { args, code, stderr } => {
                let code = code.map(|c| c.to_string()).unwrap_or_else(|| "senal".into());
                write!(
                    f,
                    "comando adb fallo (codigo {code}): adb {}\n{}",
                    args.join(" "),
                    stderr.trim()
                )
            }
            AdbError::Io(e) => write!(f, "error de I/O al invocar adb: {e}"),
        }
    }
}

impl std::error::Error for AdbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AdbError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AdbError {
    fn from(e: std::io::Error) -> Self {
        AdbError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, AdbError>;
