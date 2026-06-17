//! Capa de ejecución sobre el binario `adb`.
//!
//! [`Adb`] localiza y corre adb; [`Target`] ata un serial concreto para que
//! cada comando vaya con `-s <serial>` y no haya ambigüedad. La lógica pura de
//! parseo y selección vive en [`crate::parse`] (testeable sin hardware).

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::error::{AdbError, Result};
use crate::model::Device;
use crate::parse::{parse_devices, select_device};

/// Variable de entorno para sobreescribir la ruta del binario adb.
pub const ADB_ENV: &str = "SKRY_ADB";

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
    /// Mapea la ausencia del binario a [`AdbError::AdbNotFound`].
    fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S> + Clone,
        S: AsRef<OsStr>,
    {
        let output = Command::new(&self.program)
            .args(args.clone())
            .output()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => AdbError::AdbNotFound,
                _ => AdbError::Io(e),
            })?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(AdbError::CommandFailed {
                args: args
                    .into_iter()
                    .map(|s| s.as_ref().to_string_lossy().into_owned())
                    .collect(),
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    /// Lista los dispositivos visibles (`adb devices -l`).
    pub fn devices(&self) -> Result<Vec<Device>> {
        Ok(parse_devices(&self.run(["devices", "-l"])?))
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

    fn base(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.arg("-s").arg(&self.device.serial);
        cmd
    }

    fn run_checked(&self, mut cmd: Command, args_for_err: &[&str]) -> Result<String> {
        let output = cmd.output().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => AdbError::AdbNotFound,
            _ => AdbError::Io(e),
        })?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(AdbError::CommandFailed {
                args: args_for_err.iter().map(|s| s.to_string()).collect(),
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    /// Empuja un archivo local al dispositivo (`adb push`).
    pub fn push(&self, local: &str, remote: &str) -> Result<()> {
        let mut cmd = self.base();
        cmd.arg("push").arg(local).arg(remote);
        self.run_checked(cmd, &["push", local, remote])?;
        Ok(())
    }

    /// Crea un forward `tcp:<local_port>` → `<remote>` (ej. `localabstract:skry`).
    /// Devuelve el puerto local efectivo (útil si se pidió `tcp:0`).
    pub fn forward(&self, local: &str, remote: &str) -> Result<String> {
        let mut cmd = self.base();
        cmd.arg("forward").arg(local).arg(remote);
        let out = self.run_checked(cmd, &["forward", local, remote])?;
        Ok(out.trim().to_string())
    }

    /// Elimina un forward previamente creado.
    pub fn remove_forward(&self, local: &str) -> Result<()> {
        let mut cmd = self.base();
        cmd.arg("forward").arg("--remove").arg(local);
        self.run_checked(cmd, &["forward", "--remove", local])?;
        Ok(())
    }

    /// Corre un comando en el shell del dispositivo y devuelve su stdout.
    pub fn shell(&self, args: &[&str]) -> Result<String> {
        let mut cmd = self.base();
        cmd.arg("shell").args(args);
        let mut for_err = vec!["shell"];
        for_err.extend_from_slice(args);
        self.run_checked(cmd, &for_err)
    }

    /// Lanza el server vía `app_process` y devuelve el [`Child`] para poder
    /// matarlo en el cierre con gracia. stdin/out/err quedan piped para leer los
    /// logs del server y no dejar el proceso huérfano.
    pub fn spawn_app_process(
        &self,
        remote_jar: &str,
        main_class: &str,
        server_args: &[String],
    ) -> Result<Child> {
        let mut cmd = self.base();
        cmd.arg("shell")
            .arg(format!("CLASSPATH={remote_jar}"))
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
}
