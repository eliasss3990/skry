//! Spike 3 (cliente): valida el transporte del stream de skry sobre el túnel ADB.
//!
//! Ejercita de punta a punta los crates ya testeados:
//! - `skry-adb`: resuelve el dispositivo, lanza el server via app_process, abre
//!   el forward y limpia al final.
//! - `skry-proto`: declara el canal de video, lee el handshake y los frames.
//!
//! Por defecto vuelca los payloads a `skry-out.h265`. Con `--pipe`, escribe el
//! H.265 crudo a stdout (logs a stderr) para reproducir EN VIVO con ffplay:
//!
//! ```text
//! transport-spike.exe --pipe | ffplay -f hevc -i -
//! ```
//!
//! Sin FFmpeg/SDL2 propios: valida el transporte y permite ver el video usando
//! ffplay como decoder/render provisorio. Reenvía la salida del server con
//! prefijo `[server]` (a stderr) para diagnosticar fallos del teléfono.

use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

use skry_adb::Adb;
use skry_proto::{read_frame, Handshake, StreamType};

const REMOTE_JAR: &str = "/data/local/tmp/skry-spike.jar";
const MAIN_CLASS: &str = "skry.spike.Spike3Main";
const OUT_PATH: &str = "skry-out.h265";
// A archivo, captura acotada; en --pipe, hasta Ctrl-C o fin de stream.
const FILE_CAPTURE_SECS: u64 = 10;

fn main() {
    let pipe = std::env::args().any(|a| a == "--pipe");
    if let Err(e) = run(pipe) {
        eprintln!("[transport-spike] error: {e}");
        std::process::exit(1);
    }
}

fn run(pipe: bool) -> Result<(), Box<dyn Error>> {
    let adb = Adb::new();
    let target = adb.resolve_target(None)?;
    eprintln!("[transport-spike] dispositivo: {}", target.device().label());

    // Lanzar el server y reenviar su salida (a stderr) con prefijo [server].
    let mut child = target.spawn_app_process(REMOTE_JAR, MAIN_CLASS, &[])?;
    forward_child_output("server", child.stdout.take());
    forward_child_output("server:err", child.stderr.take());

    // Forward del túnel (no requiere que el socket exista todavía).
    let port_str = target.forward("tcp:0", "localabstract:skry")?;
    let port: u16 = port_str.parse()?;
    eprintln!("[transport-spike] forward tcp:{port} -> localabstract:skry");

    let result = stream(port, pipe);

    // Limpieza best-effort: primero cortar el adb shell local, luego el remoto.
    let _ = child.kill();
    let _ = target.kill_server(MAIN_CLASS);
    let _ = target.remove_forward(&format!("tcp:{port}"));

    result
}

/// Lanza un hilo que imprime (a stderr) la salida de un stream del child.
fn forward_child_output<R: Read + Send + 'static>(tag: &'static str, src: Option<R>) {
    if let Some(src) = src {
        thread::spawn(move || {
            for line in BufReader::new(src).lines().map_while(Result::ok) {
                eprintln!("[{tag}] {line}");
            }
        });
    }
}

/// Conecta + handshake reintentando: adb acepta la conexión local aunque el
/// server aún no escuche el localabstract, así que el reintento abarca el
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

fn stream(port: u16, pipe: bool) -> Result<(), Box<dyn Error>> {
    let (mut stream, handshake) = connect_and_handshake(port)?;
    // En vivo (pipe): lectura bloqueante sin timeout. Un mirror sólo emite
    // frames cuando la pantalla cambia, así que un timeout cortaría el stream
    // ante pantalla estática (y un read_exact a medias desincronizaría). Se
    // corta con Ctrl-C. A archivo: timeout para respetar la captura acotada.
    let read_timeout = if pipe {
        None
    } else {
        Some(Duration::from_secs(5))
    };
    stream.set_read_timeout(read_timeout)?;
    eprintln!(
        "[transport-spike] handshake OK: {} {}x{} codec={}",
        handshake.device_name,
        handshake.width,
        handshake.height,
        handshake.codec.as_str()
    );

    // Destino del H.265: stdout (modo pipe, para ffplay) o archivo.
    let mut sink: Box<dyn Write> = if pipe {
        eprintln!("[transport-spike] modo pipe: enviando H.265 a stdout (Ctrl-C para cortar)");
        Box::new(BufWriter::new(io::stdout()))
    } else {
        Box::new(BufWriter::new(File::create(OUT_PATH)?))
    };

    let mut frames = 0u64;
    let mut bytes = 0u64;
    let start = Instant::now();
    loop {
        if !pipe && start.elapsed() >= Duration::from_secs(FILE_CAPTURE_SECS) {
            break;
        }
        match read_frame(&mut stream) {
            Ok((header, payload)) => {
                sink.write_all(&payload)?;
                if pipe {
                    // En vivo: vaciar para minimizar latencia hacia ffplay.
                    sink.flush()?;
                }
                bytes += payload.len() as u64;
                if !header.config {
                    frames += 1;
                }
            }
            Err(e) => {
                eprintln!("[transport-spike] fin de stream: {e}");
                break;
            }
        }
    }
    sink.flush()?;
    let dest = if pipe { "stdout" } else { OUT_PATH };
    eprintln!("[transport-spike] OK: {frames} frames, {bytes} bytes -> {dest}");
    Ok(())
}
