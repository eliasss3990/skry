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

    /// Crear una pantalla virtual INDEPENDIENTE en el teléfono y transmitir ESA
    /// (no el espejo del panel). El teléfono queda libre: lo que se muestre acá
    /// sigue transmitiéndose a la PC aunque uses otra cosa en el celular.
    #[arg(long)]
    new_display: bool,

    /// Tamaño de la pantalla independiente, `ANCHOxALTO` (sólo con --new-display).
    /// Por defecto 1600x900 (16:9, cómodo para contenido).
    #[arg(long, default_value = "1600x900", requires = "new_display", value_parser = parse_display_size)]
    new_display_size: String,

    /// Package de la app a abrir en la pantalla independiente (sólo con
    /// --new-display). Si se omite, abre el launcher (home) del teléfono.
    #[arg(long, requires = "new_display")]
    app: Option<String>,

    /// Conectar directo a la app skry del teléfono por TCP (`HOST:PUERTO`, ej.
    /// `192.168.1.50:7345`), sin adb. La app captura con MediaProjection y se
    /// anuncia por mDNS; este modo no despliega ni lanza nada por adb.
    #[arg(long, conflicts_with_all = ["serial", "new_display", "server_jar", "main_class"])]
    connect: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        eprintln!("[skry] error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn Error>> {
    // Modo directo: la app del teléfono ya es el server (MediaProjection). Sólo
    // conectamos por TCP; nada de adb, despliegue ni forward. Con reconexión: si
    // el celu se cae un rato, reintenta solo hasta que vuelva (a prueba de cortes).
    if let Some(addr) = &cli.connect {
        return connect_loop(addr, cli);
    }

    // Modo adb: desplegar y lanzar el server spike por app_process.
    let adb = Adb::new();
    let target = adb.resolve_target(cli.serial.as_deref())?;
    eprintln!("[skry] dispositivo: {}", target.device().label());

    // Lanzar el server y reenviar su salida (a stderr) para diagnóstico.
    let server_args = build_server_args(cli);
    let mut child = target.spawn_app_process(&cli.server_jar, &cli.main_class, &server_args)?;
    forward_child_output("server", child.stdout.take());
    forward_child_output("server:err", child.stderr.take());

    let port_str = target.forward("tcp:0", "localabstract:skry")?;
    let port: u16 = port_str
        .parse()
        .map_err(|_| format!("adb devolvió un puerto inválido: '{port_str}'"))?;
    eprintln!("[skry] forward tcp:{port} -> localabstract:skry");

    let result = mirror(
        &format!("127.0.0.1:{port}"),
        cli.fullscreen,
        cli.display,
        cli.no_vsync,
        cli.fill,
    );

    // Limpieza best-effort: cortar el adb shell local, matar el server remoto,
    // soltar el forward. No dejar el server huérfano consumiendo batería.
    let _ = child.kill();
    let _ = target.kill_server(&cli.main_class);
    let _ = target.remove_forward(&format!("tcp:{port}"));

    // En modo adb una sola sesión: el motivo de fin (quit o stream) no importa.
    result.map(|_| ())
}

/// Modo --connect: mantiene el espejo vivo a través de cortes. Si el stream se
/// corta (el celu se durmió, blip de red), reconecta a la misma dirección; si no
/// hay conexión, reintenta con backoff. Sólo termina cuando el usuario cierra.
fn connect_loop(addr: &str, cli: &Cli) -> Result<(), Box<dyn Error>> {
    const INITIAL_BACKOFF: Duration = Duration::from_millis(500);
    const MAX_BACKOFF: Duration = Duration::from_secs(5);
    eprintln!("[skry] conectando a {addr}");
    let mut backoff = INITIAL_BACKOFF;
    loop {
        match mirror(addr, cli.fullscreen, cli.display, cli.no_vsync, cli.fill) {
            Ok(EndReason::UserQuit) => return Ok(()),
            Ok(EndReason::StreamEnded) => {
                eprintln!("[skry] stream cortado; reconectando a {addr}...");
                // Reconexión exitosa previa: volver al backoff mínimo.
                backoff = INITIAL_BACKOFF;
                thread::sleep(backoff);
            }
            Err(e) => {
                eprintln!(
                    "[skry] sin conexión ({e}); reintento en {:.1}s",
                    backoff.as_secs_f32()
                );
                thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

/// Valida que el tamaño venga como `ANCHOxALTO` con ambos enteros positivos.
/// Atrapa basura (`abc`, `1600`, `1600x`) en el cliente, con un mensaje claro,
/// en vez de mandar un `nd-size` mal formado al server.
fn parse_display_size(s: &str) -> Result<String, String> {
    let bad = || format!("formato inválido '{s}': esperado ANCHOxALTO (ej. 1600x900)");
    let (w, h) = s.split_once('x').ok_or_else(bad)?;
    let w: u32 = w.parse().map_err(|_| bad())?;
    let h: u32 = h.parse().map_err(|_| bad())?;
    if w == 0 || h == 0 {
        return Err(bad());
    }
    Ok(s.to_string())
}

/// Arma los args `clave=valor` que recibe el server. Mantiene `max-size` siempre
/// y agrega las opciones de pantalla independiente sólo cuando se pidió.
fn build_server_args(cli: &Cli) -> Vec<String> {
    let mut args = vec![format!("max-size={}", cli.max_size)];
    if cli.new_display {
        args.push("new-display=1".to_string());
        args.push(format!("nd-size={}", cli.new_display_size));
        if let Some(app) = &cli.app {
            args.push(format!("app={app}"));
        }
    }
    args
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
/// Por qué terminó una sesión de espejado: el usuario cerró la ventana, o el
/// stream se cortó (server caído, red, el teléfono se durmió). En modo --connect
/// el primer caso termina el programa y el segundo dispara una reconexión.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndReason {
    UserQuit,
    StreamEnded,
}

fn mirror(
    addr: &str,
    fullscreen: bool,
    display: Option<usize>,
    no_vsync: bool,
    fill: bool,
) -> Result<EndReason, Box<dyn Error>> {
    let (stream, handshake) = connect_and_handshake(addr)?;
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
    let reason = loop {
        if !renderer.pump() {
            break EndReason::UserQuit;
        }
        // El decoder terminó (server caído, stream cerrado o error): salir en vez
        // de quedar congelado mostrando el último frame para siempre.
        if decoder.is_finished() {
            break EndReason::StreamEnded;
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
    };

    // Desbloquear los hilos cerrando el socket y unirlos.
    let _ = stream.shutdown(Shutdown::Both);
    let _ = reader.join();
    match decoder.join() {
        Ok(Ok(())) => Ok(reason),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            eprintln!("[skry] el hilo decoder paniqueó");
            Ok(reason)
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

/// Conecta + handshake reintentando: tanto adb (acepta la conexión local aunque
/// el server aún no escuche) como la app remota pueden tardar en estar listos,
/// así que el reintento abarca el handshake completo.
fn connect_and_handshake(addr: &str) -> Result<(TcpStream, Handshake), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_err: Option<Box<dyn Error>> = None;
    while Instant::now() < deadline {
        match try_handshake(addr) {
            Ok(pair) => return Ok(pair),
            Err(e) => {
                last_err = Some(e);
                thread::sleep(Duration::from_millis(400));
            }
        }
    }
    Err(format!("el server no respondió el handshake en 10s: {last_err:?}").into())
}

fn try_handshake(addr: &str) -> Result<(TcpStream, Handshake), Box<dyn Error>> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    StreamType::Video.write(&mut stream)?;
    let handshake = Handshake::read(&mut stream)?;
    // Streaming: lectura bloqueante (un mirror sólo emite frames al cambiar la
    // pantalla; un timeout cortaría ante pantalla estática).
    stream.set_read_timeout(None)?;
    Ok((stream, handshake))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_size_valido() {
        assert_eq!(parse_display_size("1600x900").unwrap(), "1600x900");
        assert_eq!(parse_display_size("1x1").unwrap(), "1x1");
    }

    #[test]
    fn display_size_invalido() {
        for malo in [
            "1600", "abc", "1600x", "x900", "0x900", "1600x0", "16x0x9", "",
        ] {
            assert!(
                parse_display_size(malo).is_err(),
                "deberia rechazar '{malo}'"
            );
        }
    }

    /// Parsea una CLI mínima válida y aplica overrides para no repetir flags.
    fn cli_from(extra: &[&str]) -> Cli {
        let mut argv = vec!["skry"];
        argv.extend_from_slice(extra);
        Cli::try_parse_from(argv).expect("CLI valida")
    }

    #[test]
    fn server_args_mirror_solo_lleva_max_size() {
        let args = build_server_args(&cli_from(&[]));
        assert_eq!(args, vec!["max-size=2400".to_string()]);
    }

    #[test]
    fn server_args_new_display_con_app() {
        let args = build_server_args(&cli_from(&[
            "--new-display",
            "--new-display-size",
            "1920x1080",
            "--app",
            "com.netflix.mediaclient",
        ]));
        assert_eq!(
            args,
            vec![
                "max-size=2400".to_string(),
                "new-display=1".to_string(),
                "nd-size=1920x1080".to_string(),
                "app=com.netflix.mediaclient".to_string(),
            ]
        );
    }

    #[test]
    fn server_args_new_display_sin_app_es_home() {
        let args = build_server_args(&cli_from(&["--new-display"]));
        assert_eq!(
            args,
            vec![
                "max-size=2400".to_string(),
                "new-display=1".to_string(),
                "nd-size=1600x900".to_string(),
            ]
        );
    }

    #[test]
    fn app_sin_new_display_lo_rechaza_clap() {
        // requires = "new_display": pasar --app sin --new-display es error de CLI.
        assert!(Cli::try_parse_from(["skry", "--app", "com.foo"]).is_err());
    }

    #[test]
    fn connect_parsea_direccion() {
        let cli = cli_from(&["--connect", "192.168.1.50:7345"]);
        assert_eq!(cli.connect.as_deref(), Some("192.168.1.50:7345"));
    }

    #[test]
    fn connect_conflictua_con_modo_adb() {
        // --connect (app directa) no convive con flags del modo adb/spike.
        for otro in [
            ["--connect", "1.2.3.4:7345", "--new-display"].as_slice(),
            ["--connect", "1.2.3.4:7345", "--serial", "abc123"].as_slice(),
        ] {
            let mut argv = vec!["skry"];
            argv.extend_from_slice(otro);
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "deberia conflictuar: {otro:?}"
            );
        }
    }
}
