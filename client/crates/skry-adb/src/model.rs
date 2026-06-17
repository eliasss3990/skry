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
    /// Sin permisos para acceder al USB (típico en Linux sin reglas udev).
    NoPermissions,
    /// Otro estado reportado por adb (bootloader, recovery, etc.).
    Other(String),
}

impl DeviceState {
    /// Parsea un único token de estado. Los estados multi-palabra
    /// (`no permissions`) se detectan antes, en el parseo de la línea.
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
            DeviceState::NoPermissions => write!(f, "no permissions"),
            DeviceState::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Medio de transporte del dispositivo, inferido del serial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Conectado por cable (serial alfanumérico).
    Usb,
    /// Conectado por red (`host:puerto` o serial mDNS). Operación inalámbrica.
    Wifi,
    /// Emulador de Android (`emulator-NNNN`).
    Emulator,
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
    /// Infiere el transporte a partir del serial:
    /// - `emulator-NNNN` → emulador.
    /// - `host:puerto` o serial mDNS (`..._adb-tls-connect._tcp`) → Wi-Fi.
    /// - el resto (alfanumérico) → USB.
    pub fn infer_transport(serial: &str) -> Transport {
        if serial.starts_with("emulator-") {
            return Transport::Emulator;
        }
        // Wireless debugging (Android 11+) puede aparecer como serial mDNS, no
        // sólo como host:puerto numérico.
        let is_mdns = serial.contains("._adb-tls") || serial.ends_with("._tcp");
        let is_host_port = serial
            .rsplit_once(':')
            .is_some_and(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()));
        if is_mdns || is_host_port {
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
            Transport::Emulator => "emulador",
        };
        match &self.model {
            Some(m) => format!("{} ({m}, {kind})", self.serial),
            None => format!("{} ({kind})", self.serial),
        }
    }
}
