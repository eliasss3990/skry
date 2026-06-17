//! Parseo puro de la salida de adb y selección de dispositivo.
//!
//! Estas funciones no ejecutan nada: transforman texto en modelos. Por eso son
//! 100% testeables sin un dispositivo ni el binario adb conectados, que es donde
//! vive la lógica de resiliencia de los casos de conexión física.

use crate::error::{AdbError, Result};
use crate::model::{Device, DeviceState};

/// Parsea la salida de `adb devices -l`.
///
/// Ignora el encabezado, las vacías y las líneas de estado del daemon (que adb
/// emite a stdout en frío, p. ej. `* daemon started successfully *`) para no
/// inyectar dispositivos fantasma. Cada línea de dispositivo es
/// `serial  estado  key:value...`; se extrae `model:` si está presente.
pub fn parse_devices(output: &str) -> Vec<Device> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "List of devices attached" && !l.starts_with('*'))
        .filter_map(parse_device_line)
        .collect()
}

fn parse_device_line(line: &str) -> Option<Device> {
    let mut parts = line.split_whitespace();
    let serial = parts.next()?.to_string();
    // El resto de la línea: estado (puede ser multi-palabra) + descriptores.
    let rest: Vec<&str> = parts.collect();
    let state = parse_state(&rest)?;
    let model = rest
        .iter()
        .find_map(|kv| kv.strip_prefix("model:"))
        .map(|m| m.replace('_', " "));
    let transport = Device::infer_transport(&serial);
    Some(Device {
        serial,
        state,
        transport,
        model,
    })
}

/// Determina el estado a partir de los tokens posteriores al serial. Maneja el
/// estado multi-palabra `no permissions` (Linux sin udev); el resto es el
/// primer token. Devuelve `None` si no hay token de estado (línea malformada).
fn parse_state(rest: &[&str]) -> Option<DeviceState> {
    match rest {
        // adb imprime "no permissions; see [url]"; el 2º token trae el ';'.
        [first, second, ..] if *first == "no" && second.starts_with("permissions") => {
            Some(DeviceState::NoPermissions)
        }
        [first, ..] => Some(DeviceState::parse(first)),
        [] => None,
    }
}

/// Elige el dispositivo objetivo a partir de los conectados y un serial opcional.
///
/// Concentra la resiliencia de conexión física: sin dispositivo, ambigüedad,
/// no autorizado y estados no operables se mapean a su error accionable.
///
/// El caso C del catálogo (no autorizado) se devuelve como
/// [`AdbError::Unauthorized`] sin bloquear: el loop de espera de los ~10s
/// pidiendo aceptar el diálogo es responsabilidad del orquestador (capa CLI),
/// no de esta función pura.
pub fn select_device(devices: &[Device], desired_serial: Option<&str>) -> Result<Device> {
    if let Some(serial) = desired_serial {
        return match devices.iter().find(|d| d.serial == serial) {
            None => Err(AdbError::SerialNotFound {
                serial: serial.to_string(),
                available: devices.to_vec(),
            }),
            Some(d) => ready_or_state_error(d),
        };
    }

    if devices.is_empty() {
        return Err(AdbError::NoDevice);
    }

    let ready: Vec<&Device> = devices.iter().filter(|d| d.state.is_ready()).collect();
    match ready.as_slice() {
        [only] => Ok((*only).clone()),
        [] => {
            // Ninguno listo. Si hay uno solo, devolver su estado concreto para
            // que el caller pueda esperar autorización; si hay varios, ambiguo.
            if devices.len() == 1 {
                ready_or_state_error(&devices[0])
            } else {
                Err(AdbError::AmbiguousDevice {
                    devices: devices.to_vec(),
                })
            }
        }
        _ => Err(AdbError::AmbiguousDevice {
            devices: devices.to_vec(),
        }),
    }
}

fn ready_or_state_error(d: &Device) -> Result<Device> {
    match &d.state {
        DeviceState::Device => Ok(d.clone()),
        DeviceState::Unauthorized | DeviceState::Authorizing => Err(AdbError::Unauthorized {
            serial: d.serial.clone(),
        }),
        DeviceState::NoPermissions => Err(AdbError::NoPermissions {
            serial: d.serial.clone(),
        }),
        other => Err(AdbError::NotReady {
            serial: d.serial.clone(),
            state: other.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Transport;

    const SAMPLE: &str = "List of devices attached\n\
        R5CY139AG4E            device usb:1-1.4 product:e1q model:SM_S928B device:e1q transport_id:1\n\
        192.168.1.5:37251      device product:e1q model:SM_S928B device:e1q transport_id:2\n";

    #[test]
    fn parses_usb_and_wifi() {
        let devs = parse_devices(SAMPLE);
        assert_eq!(devs.len(), 2);
        assert_eq!(devs[0].serial, "R5CY139AG4E");
        assert_eq!(devs[0].transport, Transport::Usb);
        assert_eq!(devs[0].model.as_deref(), Some("SM S928B"));
        assert_eq!(devs[1].serial, "192.168.1.5:37251");
        assert_eq!(devs[1].transport, Transport::Wifi);
    }

    #[test]
    fn parses_unauthorized_without_model() {
        let out =
            "List of devices attached\nR5CY139AG4E       unauthorized usb:1-1 transport_id:3\n";
        let devs = parse_devices(out);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].state, DeviceState::Unauthorized);
        assert!(devs[0].model.is_none());
    }

    #[test]
    fn empty_list_parses_to_nothing() {
        assert!(parse_devices("List of devices attached\n\n").is_empty());
    }

    #[test]
    fn ignores_daemon_lines() {
        // En frío, adb emite estas líneas a stdout: no deben volverse fantasmas.
        let out = "* daemon not running; starting now at tcp:5037 *\n\
            * daemon started successfully *\n\
            List of devices attached\n\
            R5CY139AG4E device transport_id:1\n";
        let devs = parse_devices(out);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].serial, "R5CY139AG4E");
    }

    #[test]
    fn parses_no_permissions_state() {
        let out = "List of devices attached\n\
            0123456789ABCDEF       no permissions; see [http://x]  usb:1-1\n";
        let devs = parse_devices(out);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].state, DeviceState::NoPermissions);
    }

    #[test]
    fn infers_mdns_wireless_as_wifi() {
        let out = "List of devices attached\n\
            adb-R5CY139AG4E-AbCdEf._adb-tls-connect._tcp device transport_id:1\n";
        let devs = parse_devices(out);
        assert_eq!(devs[0].transport, Transport::Wifi);
    }

    #[test]
    fn infers_emulator_transport() {
        let out = "List of devices attached\nemulator-5554 device transport_id:1\n";
        let devs = parse_devices(out);
        assert_eq!(devs[0].transport, Transport::Emulator);
    }

    #[test]
    fn select_no_device_errs() {
        assert!(matches!(select_device(&[], None), Err(AdbError::NoDevice)));
    }

    #[test]
    fn select_single_ready() {
        let devs = parse_devices(SAMPLE);
        // Tomar solo el USB para tener uno listo.
        let one = vec![devs[0].clone()];
        let sel = select_device(&one, None).unwrap();
        assert_eq!(sel.serial, "R5CY139AG4E");
    }

    #[test]
    fn select_ambiguous_when_two_ready() {
        let devs = parse_devices(SAMPLE);
        assert!(matches!(
            select_device(&devs, None),
            Err(AdbError::AmbiguousDevice { .. })
        ));
    }

    #[test]
    fn select_by_serial_picks_exact() {
        let devs = parse_devices(SAMPLE);
        let sel = select_device(&devs, Some("192.168.1.5:37251")).unwrap();
        assert_eq!(sel.transport, Transport::Wifi);
    }

    #[test]
    fn select_by_unknown_serial_errs() {
        let devs = parse_devices(SAMPLE);
        assert!(matches!(
            select_device(&devs, Some("NOPE")),
            Err(AdbError::SerialNotFound { .. })
        ));
    }

    #[test]
    fn select_single_unauthorized_yields_unauthorized() {
        let out = "List of devices attached\nR5CY139AG4E unauthorized transport_id:1\n";
        let devs = parse_devices(out);
        assert!(matches!(
            select_device(&devs, None),
            Err(AdbError::Unauthorized { .. })
        ));
    }

    #[test]
    fn select_single_offline_yields_not_ready() {
        let out = "List of devices attached\nR5CY139AG4E offline transport_id:1\n";
        let devs = parse_devices(out);
        assert!(matches!(
            select_device(&devs, None),
            Err(AdbError::NotReady { .. })
        ));
    }

    #[test]
    fn select_serial_unauthorized_yields_unauthorized() {
        let out = "List of devices attached\nABC unauthorized transport_id:1\nXYZ device transport_id:2\n";
        let devs = parse_devices(out);
        assert!(matches!(
            select_device(&devs, Some("ABC")),
            Err(AdbError::Unauthorized { .. })
        ));
    }
}
