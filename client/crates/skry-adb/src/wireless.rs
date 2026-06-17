//! Emparejamiento y descubrimiento inalámbrico (ADB sobre Wi-Fi).
//!
//! Parseo puro de las salidas de `adb connect`, `adb pair` y
//! `adb mdns services`. Como el resto del crate, la lógica de parseo no ejecuta
//! nada y es 100% testeable sin red ni dispositivo.
//!
//! Nota importante: `adb connect`/`pair` imprimen el resultado en **stdout** y
//! suelen devolver **código 0 aunque fallen**; por eso el éxito/fracaso se
//! decide parseando el texto, no por el código de salida.

use crate::error::{AdbError, Result};

/// Tipo de servicio mDNS anunciado por la depuración inalámbrica de Android.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdnsKind {
    /// Endpoint al que conectarse (`_adb-tls-connect._tcp`).
    Connect,
    /// Endpoint de emparejamiento por código (`_adb-tls-pairing._tcp`).
    Pairing,
    /// Otro tipo de servicio.
    Other(String),
}

impl MdnsKind {
    fn parse(s: &str) -> MdnsKind {
        match s {
            "_adb-tls-connect._tcp" => MdnsKind::Connect,
            "_adb-tls-pairing._tcp" => MdnsKind::Pairing,
            other => MdnsKind::Other(other.to_string()),
        }
    }
}

/// Un servicio mDNS descubierto por `adb mdns services`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsService {
    pub instance: String,
    pub kind: MdnsKind,
    /// Dirección `host:puerto`.
    pub addr: String,
}

/// Parsea `adb mdns services`.
///
/// Formato por línea: `instancia<ws>tipo<ws>host:puerto`. Ignora el encabezado
/// y las líneas de daemon (`* ...`).
pub fn parse_mdns_services(output: &str) -> Vec<MdnsService> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty() && !l.starts_with('*') && *l != "List of discovered mdns services"
        })
        .filter_map(parse_mdns_line)
        .collect()
}

fn parse_mdns_line(line: &str) -> Option<MdnsService> {
    let mut parts = line.split_whitespace();
    let instance = parts.next()?.to_string();
    let kind = MdnsKind::parse(parts.next()?);
    let addr = parts.next()?.to_string();
    Some(MdnsService {
        instance,
        kind,
        addr,
    })
}

/// Interpreta la salida de `adb connect <host:puerto>`.
///
/// `connected to ...` o `already connected to ...` → OK; cualquier otra cosa
/// (`failed to connect`, `cannot connect`, etc.) → error con el detalle.
pub fn parse_connect_result(target: &str, output: &str) -> Result<()> {
    // Escanear líneas: la de éxito puede venir tras un mensaje de daemon.
    let ok = output.lines().map(str::trim).any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("connected to") || l.starts_with("already connected")
    });
    if ok {
        Ok(())
    } else {
        Err(AdbError::ConnectFailed {
            target: target.to_string(),
            detail: output.trim().to_string(),
        })
    }
}

/// Interpreta la salida de `adb pair <host:puerto> <código>`.
///
/// `Successfully paired ...` → OK; cualquier otra cosa → error con el detalle.
pub fn parse_pair_result(target: &str, output: &str) -> Result<()> {
    let ok = output
        .lines()
        .map(str::trim)
        .any(|l| l.to_ascii_lowercase().starts_with("successfully paired"));
    if ok {
        Ok(())
    } else {
        Err(AdbError::PairFailed {
            target: target.to_string(),
            detail: output.trim().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mdns_services() {
        let out = "List of discovered mdns services\n\
            adb-R5CY139AG4E-AbCdEf\t_adb-tls-connect._tcp\t192.168.1.5:37251\n\
            adb-R5CY139AG4E-AbCdEf\t_adb-tls-pairing._tcp\t192.168.1.5:42000\n";
        let svcs = parse_mdns_services(out);
        assert_eq!(svcs.len(), 2);
        assert_eq!(svcs[0].kind, MdnsKind::Connect);
        assert_eq!(svcs[0].addr, "192.168.1.5:37251");
        assert_eq!(svcs[1].kind, MdnsKind::Pairing);
    }

    #[test]
    fn ignores_daemon_and_header_in_mdns() {
        let out = "* daemon started successfully *\nList of discovered mdns services\n";
        assert!(parse_mdns_services(out).is_empty());
    }

    #[test]
    fn connect_ok_variants() {
        assert!(parse_connect_result("h:5555", "connected to h:5555").is_ok());
        assert!(parse_connect_result("h:5555", "already connected to h:5555").is_ok());
    }

    #[test]
    fn connect_failure_is_error() {
        let err = parse_connect_result(
            "192.168.1.5:5555",
            "failed to connect to '192.168.1.5:5555': Connection refused",
        )
        .unwrap_err();
        assert!(matches!(err, AdbError::ConnectFailed { .. }));
    }

    #[test]
    fn pair_ok_and_failure() {
        assert!(
            parse_pair_result("h:42000", "Successfully paired to h:42000 [guid=adb-xxx]").is_ok()
        );
        assert!(matches!(
            parse_pair_result("h:42000", "Failed: Wrong pairing code").unwrap_err(),
            AdbError::PairFailed { .. }
        ));
    }

    #[test]
    fn connect_only_daemon_line_is_error() {
        // Sin linea de exito, debe ser error (no interpretar el daemon como OK).
        assert!(parse_connect_result("h:5555", "* daemon started successfully *\n").is_err());
    }

    #[test]
    fn connect_case_insensitive() {
        // El parseo es case-insensitive: adb podria variar mayusculas.
        assert!(parse_connect_result("h:5555", "CONNECTED TO h:5555").is_ok());
    }

    #[test]
    fn connect_ok_after_daemon_prefix() {
        // Antiregresion: la linea de exito viene DESPUES del mensaje de daemon.
        let out = "* daemon not running; starting now at tcp:5037 *\n\
            * daemon started successfully *\n\
            connected to 192.168.1.5:5555\n";
        assert!(parse_connect_result("192.168.1.5:5555", out).is_ok());
    }
}
