//! Modelo de dispositivos ADB.

use std::fmt;

/// Estado de un dispositivo según lo reporta `adb devices`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    /// Listo para usar.
    Device,
    /// Conectado pero el usuario no aceptó el diálogo de depuración.
    Unauthorized,
    /// Visible pero sin responder (suele resolverse reconectando).
    Offline,
    /// En proceso de autorización.
    Authorizing,
    /// Otro estado reportado por adb (bootloader, recovery, etc.).
    Other(String),
}

impl DeviceState {
    pub fn parse(s: &str) -> DeviceState {
        match s {
            "device" => DeviceState::Device,
            "unauthorized" => DeviceState::Unauthorized,
            "offline" => DeviceState::Offline,
            "authorizing" => DeviceState::Authorizing,
            other => DeviceState::Other(other.to_string()),
        }
    }

    /// ¿El dispositivo está listo para recibir comandos?
    pub fn is_ready(&self) -> bool {
        matches!(self, DeviceState::Device)
    }
}

impl fmt::Display for DeviceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceState::Device => write!(f, "device"),
            DeviceState::Unauthorized => write!(f, "unauthorized"),
            DeviceState::Offline => write!(f, "offline"),
            DeviceState::Authorizing => write!(f, "authorizing"),
            DeviceState::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Medio de transporte del dispositivo, inferido del serial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Conectado por cable (serial alfanumérico).
    Usb,
    /// Conectado por red (el serial es `host:puerto`). Operación inalámbrica.
    Wifi,
}

/// Un dispositivo visible por adb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub state: DeviceState,
    pub transport: Transport,
    /// Modelo, si adb lo expone en la línea (`model:SM-S928B`).
    pub model: Option<String>,
}

impl Device {
    /// Infiere el transporte a partir del serial: los seriales de red tienen la
    /// forma `host:puerto` (ADB sobre Wi-Fi), el resto son USB.
    pub fn infer_transport(serial: &str) -> Transport {
        // Un serial de red contiene ':' separando host y puerto. Los seriales
        // USB son alfanuméricos sin ':'.
        if serial
            .rsplit_once(':')
            .is_some_and(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
        {
            Transport::Wifi
        } else {
            Transport::Usb
        }
    }

    /// Etiqueta legible para mostrar al usuario en listados.
    pub fn label(&self) -> String {
        let kind = match self.transport {
            Transport::Usb => "USB",
            Transport::Wifi => "Wi-Fi",
        };
        match &self.model {
            Some(m) => format!("{} ({m}, {kind})", self.serial),
            None => format!("{} ({kind})", self.serial),
        }
    }
}
