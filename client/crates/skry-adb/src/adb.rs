//! Capa de ejecución sobre el binario `adb`.
//!
//! [`Adb`] localiza y corre adb; [`Target`] ata un serial concreto para que
//! cada comando vaya con `-s <serial>` y no haya ambigüedad. La lógica pura de
//! parseo y selección vive en [`crate::parse`] (testeable sin hardware).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::error::{AdbError, Result};
use crate::model::Device;
use crate::parse::{parse_devices, select_device};

/// Variable de entorno para sobreescribir la ruta del binario adb.
pub const ADB_ENV: &str = "SKRY_ADB";

/// Corre `program <args>` y devuelve stdout si terminó OK. Mapea la ausencia del
/// binario a [`AdbError::AdbNotFound`] y el resto a [`AdbError::CommandFailed`]
/// con el comando completo para diagnóstico. Helper compartido por `Adb` y
/// `Target` para no duplicar el manejo de proceso/errores.
fn run_command(program: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => AdbError::AdbNotFound,
            _ => AdbError::Io(e),
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(AdbError::CommandFailed {
            args: args.iter().map(|s| s.to_string()).collect(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Handle del binario adb.
#[derive(Debug, Clone)]
pub struct Adb {
    program: PathBuf,
}

impl Default for Adb {
    fn default() -> Self {
        Adb::new()
    }
}

impl Adb {
    /// Usa `$SKRY_ADB` si está definida, o `adb` del PATH.
    pub fn new() -> Self {
        let program = std::env::var_os(ADB_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("adb"));
        Adb { program }
    }

    /// Corre adb con los argumentos dados y devuelve stdout si terminó OK.
    fn run(&self, args: &[&str]) -> Result<String> {
        run_command(&self.program, args)
    }

    /// Lista los dispositivos visibles (`adb devices -l`).
    pub fn devices(&self) -> Result<Vec<Device>> {
        Ok(parse_devices(&self.run(&["devices", "-l"])?))
    }

    /// Descubre dispositivos anunciados por mDNS (depuración inalámbrica).
    /// Útil para el flujo inalámbrico sin pedir IP al usuario.
    pub fn mdns_services(&self) -> Result<Vec<crate::wireless::MdnsService>> {
        Ok(crate::wireless::parse_mdns_services(
            &self.run(&["mdns", "services"])?,
        ))
    }

    /// Conecta por Wi-Fi a `host:puerto` (`adb connect`). adb suele devolver
    /// código 0 aunque falle, por eso se interpreta la salida de texto.
    pub fn connect(&self, host_port: &str) -> Result<()> {
        crate::wireless::parse_connect_result(host_port, &self.run(&["connect", host_port])?)
    }

    /// Desconecta un endpoint de red (`adb disconnect`).
    pub fn disconnect(&self, host_port: &str) -> Result<()> {
        self.run(&["disconnect", host_port])?;
        Ok(())
    }

    /// Empareja por código con `host:puerto` (`adb pair`). El emparejamiento
    /// requiere que el usuario lea el código en el teléfono (Android lo exige).
    pub fn pair(&self, host_port: &str, code: &str) -> Result<()> {
        crate::wireless::parse_pair_result(host_port, &self.run(&["pair", host_port, code])?)
    }

    /// Resuelve el dispositivo objetivo aplicando la resiliencia de selección.
    pub fn resolve_target(&self, desired_serial: Option<&str>) -> Result<Target> {
        let devices = self.devices()?;
        let device = select_device(&devices, desired_serial)?;
        Ok(Target {
            program: self.program.clone(),
            device,
        })
    }
}

/// Un dispositivo concreto sobre el que operar. Todos los comandos van con
/// `-s <serial>`.
#[derive(Debug, Clone)]
pub struct Target {
    program: PathBuf,
    device: Device,
}

impl Target {
    pub fn device(&self) -> &Device {
        &self.device
    }

    pub fn serial(&self) -> &str {
        &self.device.serial
    }

    /// Construye `adb -s <serial> <args>` listo para correr.
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.arg("-s").arg(&self.device.serial).args(args);
        cmd
    }

    /// Corre `adb -s <serial> <args>` y devuelve stdout si terminó OK. El error
    /// incluye el comando completo (con `-s <serial>`) para diagnóstico real.
    fn run_args(&self, args: &[&str]) -> Result<String> {
        let mut full = vec!["-s", self.device.serial.as_str()];
        full.extend_from_slice(args);
        run_command(&self.program, &full)
    }

    /// Empuja un archivo local al dispositivo (`adb push`).
    pub fn push(&self, local: &str, remote: &str) -> Result<()> {
        self.run_args(&["push", local, remote])?;
        Ok(())
    }

    /// Crea un forward `tcp:<local_port>` → `<remote>` (ej. `localabstract:skry`).
    /// Devuelve el puerto local efectivo (útil si se pidió `tcp:0`).
    pub fn forward(&self, local: &str, remote: &str) -> Result<String> {
        let out = self.run_args(&["forward", local, remote])?;
        // Si el daemon arranca en este comando, su mensaje precede al puerto;
        // tomar la última línea no vacía descarta ese ruido.
        let port = out
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string();
        Ok(port)
    }

    /// Elimina un forward previamente creado.
    pub fn remove_forward(&self, local: &str) -> Result<()> {
        self.run_args(&["forward", "--remove", local])?;
        Ok(())
    }

    /// Corre un comando en el shell del dispositivo y devuelve su stdout.
    pub fn shell(&self, args: &[&str]) -> Result<String> {
        let mut full = vec!["shell"];
        full.extend_from_slice(args);
        self.run_args(&full)
    }

    /// Lanza el server vía `app_process` y devuelve el [`Child`].
    ///
    /// `stdin` queda en `null` (el server no lee de ahí); `stdout`/`stderr`
    /// quedan piped para leer sus logs.
    ///
    /// **Importante**: matar este [`Child`] mata el cliente `adb` local, pero
    /// **no garantiza** matar el proceso `app_process` en el teléfono — adb no
    /// propaga la muerte de forma confiable. Para un cierre limpio, el caller
    /// debe además invocar [`Target::kill_server`] (y el server debería
    /// auto-terminarse al cerrarse sus sockets). Ver `docs/resilience.md`.
    pub fn spawn_app_process(
        &self,
        remote_jar: &str,
        main_class: &str,
        server_args: &[String],
    ) -> Result<Child> {
        let mut cmd = self.command(&["shell"]);
        cmd.arg(format!("CLASSPATH={remote_jar}"))
            .arg("app_process")
            .arg("/")
            .arg(main_class)
            .args(server_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => AdbError::AdbNotFound,
            _ => AdbError::Io(e),
        })
    }

    /// Mata en el teléfono cualquier `app_process` que corra la clase dada.
    /// Necesario para no dejar el server huérfano consumiendo batería (matar el
    /// [`Child`] local no alcanza). Idempotente: si no hay proceso, no es error.
    pub fn kill_server(&self, main_class: &str) -> Result<()> {
        // pkill devuelve 1 si no encontró procesos; lo tratamos como éxito.
        match self.run_args(&["shell", "pkill", "-f", main_class]) {
            Ok(_) => Ok(()),
            Err(AdbError::CommandFailed { code: Some(1), .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::model::{DeviceState, Transport};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    /// Crea un stub ejecutable que imita a adb y devuelve un `Target` que lo usa.
    fn stub_target(script: &str, name: &str) -> Target {
        let path = std::env::temp_dir().join(format!("skry-adb-stub-{name}"));
        // Escribir y CERRAR el archivo antes de ejecutarlo: si el handle de
        // escritura sigue abierto, exec falla con ETXTBSY (test flaky).
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(script.as_bytes()).unwrap();
            f.sync_all().unwrap();
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        Target {
            program: path,
            device: Device {
                serial: "TESTSERIAL".into(),
                state: DeviceState::Device,
                transport: Transport::Usb,
                model: None,
            },
        }
    }

    #[test]
    fn forward_returns_last_line_ignoring_daemon_prefix() {
        // El stub imprime la linea de daemon antes del puerto; forward debe
        // devolver solo el puerto (ultima linea no vacia).
        let t = stub_target(
            "#!/bin/sh\nprintf '* daemon started successfully *\\n39000\\n'\n",
            "forward",
        );
        let port = t.forward("tcp:0", "localabstract:skry").unwrap();
        assert_eq!(port, "39000");
    }

    #[test]
    fn kill_server_treats_exit_1_as_ok() {
        // pkill sin procesos sale con 1; kill_server lo trata como exito.
        let t = stub_target("#!/bin/sh\nexit 1\n", "kill");
        assert!(t.kill_server("com.skry.server.Main").is_ok());
    }

    #[test]
    fn kill_server_propagates_other_failures() {
        let t = stub_target("#!/bin/sh\nexit 2\n", "kill2");
        assert!(t.kill_server("com.skry.server.Main").is_err());
    }
}
