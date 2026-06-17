//! Wrapper tipado sobre el binario `adb`.
//!
//! - [`parse`]: parseo puro de la salida de adb y selección de dispositivo
//!   (toda la resiliencia de conexión física, testeable sin hardware).
//! - [`adb`]: ejecución real de comandos adb.
//! - [`model`]: modelo de dispositivos.
//! - [`error`]: errores con mensajes accionables (`docs/resilience.md`).

pub mod adb;
pub mod error;
pub mod model;
pub mod parse;
pub mod wireless;

pub use adb::{Adb, Target, ADB_ENV};
pub use error::{AdbError, Result};
pub use model::{Device, DeviceState, Transport};
pub use parse::{parse_devices, select_device};
pub use wireless::{parse_mdns_services, MdnsKind, MdnsService};
