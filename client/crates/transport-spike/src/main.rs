//! Spike 3 (cliente): valida el transporte del stream de skry sobre el túnel ADB.
//!
//! Ejercita de punta a punta los crates ya testeados:
//! - `skry-adb`: resuelve el dispositivo, lanza el server via app_process, abre
//!   el forward y limpia al final.
//! - `skry-proto`: declara el canal de video, lee el handshake y los frames.
//!
//! Vuelca los payloads a `skry-out.h265` para abrir con ffplay. Sin FFmpeg/SDL2:
//! valida sólo que el stream fluye y se enmarca bien antes de meter decode/render.

use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use skry_adb::Adb;
use skry_proto::{read_frame, Handshake, StreamType};

const REMOTE_JAR: &str = "/data/local/tmp/skry-spike.jar";
const MAIN_CLASS: &str = "skry.spike.Spike3Main";
const OUT_PATH: &str = "skry-out.h265";
const CAPTURE_SECS: u64 = 10;

fn main() {
    if let Err(e) = run() {
        eprintln!("[transport-spike] error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let adb = Adb::new();
    let target = adb.resolve_target(None)?;
    println!("[transport-spike] dispositivo: {}", target.device().label());

    // Lanzar el server en el teléfono.
    let mut child = target.spawn_app_process(REMOTE_JAR, MAIN_CLASS, &[])?;

    // Forward del túnel (no requiere que el socket exista todavía).
    let port_str = target.forward("tcp:0", "localabstract:skry")?;
    let port: u16 = port_str.parse()?;
    println!("[transport-spike] forward tcp:{port} -> localabstract:skry");

    let result = stream_to_file(port);

    // Limpieza best-effort, pase lo que pase con el stream.
    let _ = target.kill_server(MAIN_CLASS);
    let _ = target.remove_forward(&format!("tcp:{port}"));
    let _ = child.kill();

    result
}

/// Conecta reintentando: el server tarda en abrir el localabstract socket
/// (arranque del JVM en app_process), así que el primer connect puede fallar.
fn connect_with_retry(port: u16) -> Result<TcpStream, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut last_err = None;
    while Instant::now() < deadline {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => return Ok(s),
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }
    Err(format!("no se pudo conectar al server tras 8s: {last_err:?}").into())
}

fn stream_to_file(port: u16) -> Result<(), Box<dyn Error>> {
    let mut stream = connect_with_retry(port)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    // Declarar el canal como video (primer byte) y leer el handshake.
    StreamType::Video.write(&mut stream)?;
    let handshake = Handshake::read(&mut stream)?;
    println!(
        "[transport-spike] handshake: {} {}x{} codec={}",
        handshake.device_name,
        handshake.width,
        handshake.height,
        handshake.codec.as_str()
    );

    let mut file = File::create(OUT_PATH)?;
    let mut frames = 0u64;
    let mut bytes = 0u64;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(CAPTURE_SECS) {
        match read_frame(&mut stream) {
            Ok((header, payload)) => {
                file.write_all(&payload)?;
                bytes += payload.len() as u64;
                if !header.config {
                    frames += 1;
                }
            }
            Err(e) => {
                println!("[transport-spike] fin de stream: {e}");
                break;
            }
        }
    }
    println!("[transport-spike] OK: {frames} frames, {bytes} bytes -> {OUT_PATH}");
    Ok(())
}
