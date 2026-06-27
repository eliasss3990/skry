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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;

use skry_adb::Adb;
use skry_proto::{read_frame, FrameHeader, Handshake, StreamType};
use skry_video::{DecodedFrame, Decoder, Renderer};

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

    /// Monitor donde abrir la ventana (índice; 0 = principal). Útil para abrir
    /// en un monitor de mayor refresco y conseguir más fps.
    #[arg(long)]
    display: Option<usize>,

    /// Desactivar vsync. Diagnóstico: si el present sube mucho sin vsync, el
    /// cuello era el refresco; si no, es el trabajo por frame (subir el frame).
    #[arg(long)]
    no_vsync: bool,

    /// Llenar la pantalla recortando lo que sobra (en vez de barras negras),
    /// preservando la proporción. Se alterna en vivo con la tecla Z.
    #[arg(long)]
    fill: bool,

    /// Ruta del jar del server en el dispositivo.
    #[arg(long, default_value = "/data/local/tmp/skry-spike.jar")]
    server_jar: String,

    /// Clase principal del server.
    #[arg(long, default_value = "skry.spike.Spike3Main")]
    main_class: String,

    /// Lado máximo de captura en px. Bajarlo reduce mucho el trabajo (decode +
    /// transferencias) y sube los fps; si el monitor no muestra más resolución que
    /// eso, no se pierde calidad visible. 0 = sin límite (panel completo).
    /// 2400 es el punto dulce medido: calidad casi full y ~100fps fluidos.
    #[arg(long, default_value_t = 2400)]
    max_size: u32,
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
    let server_args = [cli.max_size.to_string()];
    let mut child = target.spawn_app_process(&cli.server_jar, &cli.main_class, &server_args)?;
    forward_child_output("server", child.stdout.take());
    forward_child_output("server:err", child.stderr.take());

    let port_str = target.forward("tcp:0", "localabstract:skry")?;
    let port: u16 = port_str
        .parse()
        .map_err(|_| format!("adb devolvió un puerto inválido: '{port_str}'"))?;
    eprintln!("[skry] forward tcp:{port} -> localabstract:skry");

    let result = mirror(port, cli.fullscreen, cli.display, cli.no_vsync, cli.fill);

    // Limpieza best-effort: cortar el adb shell local, matar el server remoto,
    // soltar el forward. No dejar el server huérfano consumiendo batería.
    let _ = child.kill();
    let _ = target.kill_server(&cli.main_class);
    let _ = target.remove_forward(&format!("tcp:{port}"));

    result
}

/// Slot de un único frame: el decoder escribe el más nuevo (pisando el anterior)
/// y el render lo consume. Es la pieza del **frame dropping**: si el render no da
/// abasto, los frames intermedios se descartan y siempre se muestra el último.
type LatestFrame = Arc<Mutex<Option<DecodedFrame>>>;

/// Umbral de backlog para el catch-up: si el decoder junta más de estos payloads
/// pendientes de una, está atrasado y salta al último keyframe disponible.
const CATCHUP_BATCH: usize = 8;

/// Toma el lock recuperándolo si quedó envenenado por un panic de otro hilo. El
/// dato protegido es un `Option<DecodedFrame>` trivialmente consistente, así que
/// un panic ajeno no debe tumbar al resto del pipeline.
fn lock_recover<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Conecta el canal de video, lee el handshake y corre el pipeline en 3 hilos.
///
/// Arquitectura (mirror en vivo, mínima latencia — NO reproducción de archivo):
/// - **lector**: bloquea en `read_frame` y empuja payloads por un canal.
/// - **decoder**: decodifica cada payload y deja el frame en un slot de uno solo,
///   pisando el anterior. Decodificar es secuencial (H.265 referencia frames
///   previos), así que no se puede saltear; el descarte ocurre en este slot.
/// - **principal**: presenta SIEMPRE el último frame disponible y procesa eventos.
///   Sin pacing por pts: se muestra apenas está listo. Si el decode se atrasa, se
///   ve a menor fps pero **en tiempo real** (nunca cámara lenta acumulada).
///
/// Cada segundo reporta `decode fps` vs `present fps` para diagnosticar el límite.
fn mirror(
    port: u16,
    fullscreen: bool,
    display: Option<usize>,
    no_vsync: bool,
    fill: bool,
) -> Result<(), Box<dyn Error>> {
    let (stream, handshake) = connect_and_handshake(port)?;
    eprintln!(
        "[skry] {} {}x{} codec={}",
        handshake.device_name,
        handshake.width,
        handshake.height,
        handshake.codec.as_str()
    );

    let mut renderer = Renderer::new(
        handshake.width as u32,
        handshake.height as u32,
        fullscreen,
        display,
        no_vsync,
        fill,
    )?;

    // Hilo lector: socket -> canal de payloads.
    let (tx, rx) = mpsc::channel::<(FrameHeader, Vec<u8>)>();
    let mut reader_stream = stream.try_clone()?;
    let reader = thread::spawn(move || {
        while let Ok(frame) = read_frame(&mut reader_stream) {
            if tx.send(frame).is_err() {
                break;
            }
        }
    });

    // Hilo decoder: canal de payloads -> slot del último frame. Se crea el Decoder
    // dentro del hilo (no necesita ser Send). Coalesce el backlog y, si está
    // atrasado, salta al último keyframe para no arrastrar latencia.
    let latest: LatestFrame = Arc::new(Mutex::new(None));
    let recv_fps = Arc::new(AtomicU64::new(0));
    let decoded_fps = Arc::new(AtomicU64::new(0));
    let codec = handshake.codec;
    let latest_dec = Arc::clone(&latest);
    let recv_fps_dec = Arc::clone(&recv_fps);
    let decoded_fps_dec = Arc::clone(&decoded_fps);
    let decoder = thread::spawn(move || -> Result<(), String> {
        let mut decoder = Decoder::new(codec).map_err(|e| e.to_string())?;
        let mut frames = Vec::new();
        while let Ok(first) = rx.recv() {
            // Juntar todo lo que ya esté en el canal (no se puede saltear un payload
            // suelto: H.265 referencia frames previos; sólo se salta a un keyframe).
            let mut batch = vec![first];
            while let Ok(more) = rx.try_recv() {
                batch.push(more);
            }
            recv_fps_dec.fetch_add(batch.len() as u64, Ordering::Relaxed);

            // Catch-up: con backlog real, arrancar desde el último keyframe (IDR
            // autocontenido) y descartar lo anterior. Sin backlog se decodifica todo.
            let start = if batch.len() > CATCHUP_BATCH {
                batch.iter().rposition(|(h, _)| h.keyframe).unwrap_or(0)
            } else {
                0
            };

            for (header, payload) in batch.into_iter().skip(start) {
                let pts = i64::try_from(header.pts).unwrap_or(i64::MAX);
                decoder
                    .decode(&payload, pts, &mut frames)
                    .map_err(|e| e.to_string())?;
                let n = frames.len() as u64;
                if let Some(last) = frames.drain(..).last() {
                    decoded_fps_dec.fetch_add(n, Ordering::Relaxed);
                    *lock_recover(&latest_dec) = Some(last);
                }
            }
        }
        Ok(())
    });

    // Hilo principal: presentar el último frame + eventos + reporte de fps.
    let mut present_fps = 0u64;
    let mut last_report = Instant::now();
    loop {
        if !renderer.pump() {
            break;
        }
        // El decoder terminó (server caído, stream cerrado o error): salir en vez
        // de quedar congelado mostrando el último frame para siempre.
        if decoder.is_finished() {
            break;
        }
        let frame = lock_recover(&latest).take();
        if let Some(decoded) = frame {
            renderer.present(&decoded.frame)?;
            present_fps += 1;
        } else {
            // Nada nuevo: no quemar CPU haciendo spin.
            thread::sleep(Duration::from_millis(2));
        }
        if last_report.elapsed() >= Duration::from_secs(1) {
            let recv = recv_fps.swap(0, Ordering::Relaxed);
            let dec = decoded_fps.swap(0, Ordering::Relaxed);
            eprintln!("[skry] recibidos {recv} fps | decode {dec} fps | present {present_fps} fps");
            present_fps = 0;
            last_report = Instant::now();
        }
    }

    // Desbloquear los hilos cerrando el socket y unirlos.
    let _ = stream.shutdown(Shutdown::Both);
    let _ = reader.join();
    match decoder.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            eprintln!("[skry] el hilo decoder paniqueó");
            Ok(())
        }
    }
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
