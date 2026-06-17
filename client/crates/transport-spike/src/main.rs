//! Spike 3 (cliente): valida el transporte del stream de skry sobre el túnel ADB.
//!
//! Ejercita de punta a punta los crates ya testeados:
//! - `skry-adb`: resuelve el dispositivo, lanza el server via app_process, abre
//!   el forward y limpia al final.
//! - `skry-proto`: declara el canal de video, lee el handshake y los frames.
//!
//! Vuelca los payloads a `skry-out.h265` para abrir con ffplay. Sin FFmpeg/SDL2:
//! valida sólo que el stream fluye y se enmarca bien antes de meter decode/render.
//!
//! Imprime la salida del server (prefijo `[server]`) para diagnosticar fallos
//! del lado del teléfono.

use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::TcpStream;
use std::thread;
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

    // Lanzar el server y reenviar su salida con prefijo [server] para diagnóstico.
    let mut child = target.spawn_app_process(REMOTE_JAR, MAIN_CLASS, &[])?;
    forward_child_output("server", child.stdout.take());
    forward_child_output("server:err", child.stderr.take());

    // Forward del túnel (no requiere que el socket exista todavía).
    let port_str = target.forward("tcp:0", "localabstract:skry")?;
    let port: u16 = port_str.parse()?;
    println!("[transport-spike] forward tcp:{port} -> localabstract:skry");

    let result = stream_to_file(port);

    // Limpieza best-effort: primero cortar el adb shell local, luego el remoto.
    let _ = child.kill();
    let _ = target.kill_server(MAIN_CLASS);
    let _ = target.remove_forward(&format!("tcp:{port}"));

    result
}

/// Lanza un hilo que imprime, línea por línea, la salida de un stream del child.
fn forward_child_output<R: Read + Send + 'static>(tag: &'static str, src: Option<R>) {
    if let Some(src) = src {
        thread::spawn(move || {
            for line in BufReader::new(src).lines().map_while(Result::ok) {
                println!("[{tag}] {line}");
            }
        });
    }
}

/// Conecta + handshake reintentando: adb acepta la conexión local aunque el
/// server aún no escuche el localabstract, así que el reintento debe abarcar el
/// handshake completo, no sólo el connect.
fn connect_and_handshake(port: u16) -> Result<(TcpStream, Handshake), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_err: Option<Box<dyn Error>> = None;
    while Instant::now() < deadline {
        match try_handshake(port) {
            Ok(pair) => return Ok(pair),
            Err(e) => {
                last_err = Some(e);
                thread::sleep(Duration::from_millis(400));
            }
        }
    }
    Err(format!("el server no respondió el handshake en 10s: {last_err:?}").into())
}

fn try_handshake(port: u16) -> Result<(TcpStream, Handshake), Box<dyn Error>> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    StreamType::Video.write(&mut stream)?;
    let handshake = Handshake::read(&mut stream)?;
    Ok((stream, handshake))
}

fn stream_to_file(port: u16) -> Result<(), Box<dyn Error>> {
    let (mut stream, handshake) = connect_and_handshake(port)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    println!(
        "[transport-spike] handshake OK: {} {}x{} codec={}",
        handshake.device_name,
        handshake.width,
        handshake.height,
        handshake.codec.as_str()
    );

    let mut file = BufWriter::new(File::create(OUT_PATH)?);
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
    file.flush()?;
    println!("[transport-spike] OK: {frames} frames, {bytes} bytes -> {OUT_PATH}");
    Ok(())
}
