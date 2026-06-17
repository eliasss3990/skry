//! `skry`: espejá la pantalla de tu Android en la PC, con baja latencia.
//!
//! Orquesta los crates ya probados en device:
//! - `skry-adb`: resuelve el dispositivo, despliega y lanza el server, abre el
//!   túnel y limpia al final.
//! - `skry-proto`: handshake y framing del stream.
//! - `skry-video`: decode (FFmpeg) + render (SDL2), presentando cada frame por
//!   su `pts` para que el video se vea exactamente como el teléfono.

use std::error::Error;
use std::io::{BufRead, BufReader, Read};
use std::net::{Shutdown, TcpStream};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;

use skry_adb::Adb;
use skry_proto::{read_frame, FrameHeader, Handshake, StreamType};
use skry_video::{Decoder, PresentationClock, Renderer};

/// Espejado de pantalla Android → PC, baja latencia, desde la terminal.
#[derive(Parser, Debug)]
#[command(name = "skry", version, about)]
struct Cli {
    /// Serial del dispositivo (necesario si hay más de uno conectado).
    #[arg(long)]
    serial: Option<String>,

    /// Arrancar en pantalla completa (también se alterna con la tecla F).
    #[arg(long)]
    fullscreen: bool,

    /// Ruta del jar del server en el dispositivo.
    #[arg(long, default_value = "/data/local/tmp/skry-spike.jar")]
    server_jar: String,

    /// Clase principal del server.
    #[arg(long, default_value = "skry.spike.Spike3Main")]
    main_class: String,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        eprintln!("[skry] error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let adb = Adb::new();
    let target = adb.resolve_target(cli.serial.as_deref())?;
    eprintln!("[skry] dispositivo: {}", target.device().label());

    // Lanzar el server y reenviar su salida (a stderr) para diagnóstico.
    let mut child = target.spawn_app_process(&cli.server_jar, &cli.main_class, &[])?;
    forward_child_output("server", child.stdout.take());
    forward_child_output("server:err", child.stderr.take());

    let port_str = target.forward("tcp:0", "localabstract:skry")?;
    let port: u16 = port_str
        .parse()
        .map_err(|_| format!("adb devolvió un puerto inválido: '{port_str}'"))?;
    eprintln!("[skry] forward tcp:{port} -> localabstract:skry");

    let result = mirror(port, cli.fullscreen);

    // Limpieza best-effort: cortar el adb shell local, matar el server remoto,
    // soltar el forward. No dejar el server huérfano consumiendo batería.
    let _ = child.kill();
    let _ = target.kill_server(&cli.main_class);
    let _ = target.remove_forward(&format!("tcp:{port}"));

    result
}

/// Conecta el canal de video, lee el handshake y corre el lazo decode→present.
///
/// La lectura de la red corre en un hilo aparte que empuja frames por un canal;
/// el hilo principal sólo decodifica, presenta y procesa eventos. Así el cierre
/// (tecla Q / cerrar ventana) responde aunque no lleguen frames (pantalla
/// estática), porque el principal nunca se bloquea leyendo de la red.
fn mirror(port: u16, fullscreen: bool) -> Result<(), Box<dyn Error>> {
    let (stream, handshake) = connect_and_handshake(port)?;
    eprintln!(
        "[skry] {} {}x{} codec={}",
        handshake.device_name,
        handshake.width,
        handshake.height,
        handshake.codec.as_str()
    );

    let mut decoder = Decoder::new(handshake.codec)?;
    let mut renderer = Renderer::new(handshake.width as u32, handshake.height as u32, fullscreen)?;
    let mut clock = PresentationClock::new();
    let mut frames = Vec::new();

    // Hilo lector: bloquea en read_frame y manda los frames por el canal.
    let (tx, rx) = mpsc::channel::<(FrameHeader, Vec<u8>)>();
    let mut reader_stream = stream.try_clone()?;
    let reader = thread::spawn(move || {
        // Sale al cerrarse el stream (Err) o al cerrar el principal el canal.
        while let Ok(frame) = read_frame(&mut reader_stream) {
            if tx.send(frame).is_err() {
                break;
            }
        }
    });

    'main: loop {
        if !renderer.pump() {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(15)) {
            Ok((header, payload)) => {
                decoder.decode(&payload, header.pts as i64, &mut frames)?;
                for decoded in frames.drain(..) {
                    clock.wait_for(decoded.pts_us);
                    renderer.present(&decoded.frame)?;
                    if !renderer.pump() {
                        break 'main;
                    }
                }
            }
            // Sin frames (pantalla estática): seguir procesando eventos.
            Err(RecvTimeoutError::Timeout) => {}
            // El server cerró el stream.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    // Desbloquear el hilo lector cerrando el socket y unirlo.
    let _ = stream.shutdown(Shutdown::Both);
    let _ = reader.join();
    Ok(())
}

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
/// server aún no escuche, así que el reintento abarca el handshake completo.
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
    // Streaming: lectura bloqueante (un mirror sólo emite frames al cambiar la
    // pantalla; un timeout cortaría ante pantalla estática).
    stream.set_read_timeout(None)?;
    Ok((stream, handshake))
}
