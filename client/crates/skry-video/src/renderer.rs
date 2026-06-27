//! Ventana de render con SDL2. Muestra frames YUV420P (camino software) o NV12
//! (camino hardware, subido directo sin conversión) y maneja eventos (salir,
//! alternar pantalla completa).
//!
//! NV12 se sube con `SDL_UpdateNVTexture` (FFI: el binding seguro de sdl2 0.37 no
//! lo expone), pasando los planos Y/UV de FFmpeg sin copia intermedia. Por eso
//! este módulo habilita `unsafe_code`, que el workspace deja en `warn`.
#![allow(unsafe_code)]

use std::time::{Duration, Instant};

use ffmpeg_next::format::Pixel;
use ffmpeg_next::frame::Video;
use sdl2::event::{Event, WindowEvent};
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;
use sdl2::render::{Canvas, Texture, TextureCreator};
use sdl2::video::{FullscreenType, Window, WindowContext};
use sdl2::Sdl;

/// Tras este tiempo sin mover el mouse, se esconde el puntero (como YouTube).
const CURSOR_IDLE_HIDE: Duration = Duration::from_millis(2500);

/// Alto máximo por defecto de la ventana (fallback si no se puede leer el monitor).
const DEFAULT_WINDOW_HEIGHT: u32 = 900;

/// Fracción del área útil del monitor que ocupa la ventana al abrir. <1 deja un
/// margen para barra de título y bordes, así nunca queda más grande que la pantalla.
const WINDOW_MARGIN: f64 = 0.90;

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer").finish_non_exhaustive()
    }
}

pub struct Renderer {
    canvas: Canvas<Window>,
    // El TextureCreator se filtra a 'static para que la textura pueda guardarse
    // junto al canvas sin caer en una estructura auto-referencial (patrón común
    // con sdl2-rust).
    _texture_creator: &'static TextureCreator<WindowContext>,
    // La textura se crea con el primer frame, según su formato (IYUV o NV12).
    texture: Option<Texture<'static>>,
    tex_format: Option<PixelFormatEnum>,
    width: u32,
    height: u32,
    // true = llenar la ventana recortando el sobrante; false = entrar entero con barras.
    fill: bool,
    sdl: Sdl,
    cursor_visible: bool,
    last_mouse_activity: Instant,
    event_pump: sdl2::EventPump,
}

impl Renderer {
    pub fn new(
        width: u32,
        height: u32,
        fullscreen: bool,
        display: Option<usize>,
        no_vsync: bool,
        fill: bool,
    ) -> Result<Self, String> {
        // Escalado lineal (en vez de nearest): suaviza el redibujo del frame a la
        // ventana, mucho mejor calidad visual. Debe setearse antes de crear la
        // textura/renderer.
        sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "1");

        let sdl = sdl2::init()?;
        let video = sdl.video()?;

        // Diagnóstico: listar monitores con su refresco para saber dónde conviene
        // abrir (y verificar que --display caiga en el de mayor Hz).
        if let Ok(n) = video.num_video_displays() {
            for i in 0..n {
                if let Ok(m) = video.desktop_display_mode(i) {
                    eprintln!("[skry] monitor {i}: {}x{} @ {}Hz", m.w, m.h, m.refresh_rate);
                }
            }
        }

        // Elegir monitor: el pedido (--display) o, por defecto, el de mayor
        // resolución (normalmente el externo, no el panel chico del notebook).
        // Antes abría en el primario, que podía ser el chico: la ventana quedaba
        // más alta que la pantalla (recorte arriba/abajo) y el downscale fuerte se
        // veía borroso.
        let n = video.num_video_displays().unwrap_or(1).max(1) as usize;
        let target = match display {
            Some(i) if i < n => i,
            Some(i) => {
                eprintln!("[skry] monitor {i} no existe (hay {n}); uso el de mayor resolución");
                best_display(&video)
            }
            None => best_display(&video),
        };
        let area = video
            .display_usable_bounds(target as i32)
            .or_else(|_| video.display_bounds(target as i32))
            .ok();

        // Tamaño de ventana: el mayor que entra en ~90% del área útil del monitor,
        // preservando el aspecto del teléfono. Nunca más grande que la pantalla
        // (evita el recorte), y lo más grande posible (evita el downscale borroso).
        let (win_w, win_h) = match area {
            Some(b) => window_size_for(width, height, b.width(), b.height()),
            None => scaled_window(width, height),
        };
        let mut builder = video.window("skry", win_w, win_h);
        builder.resizable().allow_highdpi();
        match area {
            Some(b) => {
                let x = b.x() + (b.width() as i32 - win_w as i32) / 2;
                let y = b.y() + (b.height() as i32 - win_h as i32) / 2;
                builder.position(x, y);
            }
            None => {
                builder.position_centered();
            }
        }
        if fullscreen {
            builder.fullscreen_desktop();
        }
        let window = builder.build().map_err(|e| e.to_string())?;

        // Reportar en qué monitor (y a qué Hz) quedó la ventana, y el vsync.
        if let Ok(idx) = window.display_index() {
            let hz = video
                .desktop_display_mode(idx)
                .map(|m| m.refresh_rate)
                .unwrap_or(0);
            eprintln!(
                "[skry] ventana en monitor {idx} @ {hz}Hz | vsync={}",
                !no_vsync
            );
        }

        let mut canvas_builder = window.into_canvas();
        if !no_vsync {
            canvas_builder = canvas_builder.present_vsync();
        }
        let canvas = canvas_builder.build().map_err(|e| e.to_string())?;

        // El Box::leak es INTENCIONAL y asume **un único Renderer por proceso**
        // (es el caso de skry). El leak hace que el TextureCreator viva hasta el
        // fin del proceso, así la `Texture<'static>` nunca queda colgada. Si
        // alguna vez se instancian varios Renderer, esto acumula memoria.
        let texture_creator: &'static TextureCreator<WindowContext> =
            Box::leak(Box::new(canvas.texture_creator()));

        eprintln!(
            "[skry] controles: F o doble-click = pantalla completa | Esc = salir de \
             pantalla completa (o cerrar) | Z = llenar/entero | Q = salir"
        );

        let event_pump = sdl.event_pump()?;

        Ok(Self {
            canvas,
            _texture_creator: texture_creator,
            texture: None,
            tex_format: None,
            width,
            height,
            fill,
            sdl,
            cursor_visible: true,
            last_mouse_activity: Instant::now(),
            event_pump,
        })
    }

    /// Sube el frame a la textura (NV12 o YUV420P, según el formato del frame) y
    /// lo presenta escalado a la ventana.
    pub fn present(&mut self, frame: &Video) -> Result<(), String> {
        let format = sdl_format(frame.format())?;
        self.ensure_texture(format)?;
        let texture = self.texture.as_mut().expect("textura creada arriba");

        match format {
            PixelFormatEnum::NV12 => {
                let y_pitch = i32::try_from(frame.stride(0))
                    .map_err(|_| "stride Y fuera de rango i32".to_string())?;
                let uv_pitch = i32::try_from(frame.stride(1))
                    .map_err(|_| "stride UV fuera de rango i32".to_string())?;
                // SAFETY: SDL_UpdateNVTexture es síncrono y copia en el acto; los
                // planos Y/UV de `frame` viven durante toda la llamada a present().
                // El rect null = "toda la textura". SDL deriva la altura del UV del
                // formato NV12, sólo necesita los pitches.
                let ret = unsafe {
                    sdl2::sys::SDL_UpdateNVTexture(
                        texture.raw(),
                        std::ptr::null(),
                        frame.data(0).as_ptr(),
                        y_pitch,
                        frame.data(1).as_ptr(),
                        uv_pitch,
                    )
                };
                if ret != 0 {
                    return Err(sdl2::get_error());
                }
            }
            _ => {
                // IYUV: tres planos Y/U/V.
                texture
                    .update_yuv(
                        None,
                        frame.data(0),
                        frame.stride(0),
                        frame.data(1),
                        frame.stride(1),
                        frame.data(2),
                        frame.stride(2),
                    )
                    .map_err(|e| e.to_string())?;
            }
        }

        // Destino preservando la relación de aspecto del video: se escala al
        // máximo que entra en la ventana y se centra (barras negras donde sobra).
        // Nunca deforma — clave para un teléfono vertical en un monitor ancho.
        let (win_w, win_h) = self.canvas.output_size()?;
        let dst = if self.fill {
            fill_centered(self.width, self.height, win_w, win_h)
        } else {
            fit_centered(self.width, self.height, win_w, win_h)
        };
        self.canvas.clear();
        self.canvas.copy(texture, None, Some(dst))?;
        self.canvas.present();
        Ok(())
    }

    /// Crea (o recrea, si cambió el formato) la textura streaming del tamaño del
    /// frame. En la práctica el formato es estable durante toda la sesión.
    fn ensure_texture(&mut self, format: PixelFormatEnum) -> Result<(), String> {
        if self.tex_format == Some(format) {
            return Ok(());
        }
        let texture = self
            ._texture_creator
            .create_texture_streaming(format, self.width, self.height)
            .map_err(|e| e.to_string())?;
        self.texture = Some(texture);
        self.tex_format = Some(format);
        Ok(())
    }

    /// Procesa la cola de eventos. Devuelve `false` si se pidió cerrar.
    pub fn pump(&mut self) -> bool {
        // Colectar primero: el iterador de poll_iter mantiene prestado
        // `event_pump`, y procesar abajo necesita `&mut self` (toggle_fullscreen).
        let events: Vec<Event> = self.event_pump.poll_iter().collect();

        // Actividad del mouse: reaparecer el puntero al moverlo/clickear. También
        // al recuperar el foco de la ventana, para no dejarlo oculto al volver.
        let mouse_active = events.iter().any(|e| {
            matches!(
                e,
                Event::MouseMotion { .. }
                    | Event::MouseButtonDown { .. }
                    | Event::MouseWheel { .. }
                    | Event::Window {
                        win_event: WindowEvent::FocusGained,
                        ..
                    }
            )
        });
        if mouse_active {
            self.last_mouse_activity = Instant::now();
            if !self.cursor_visible {
                self.sdl.mouse().show_cursor(true);
                self.cursor_visible = true;
            }
        } else if self.cursor_visible && self.last_mouse_activity.elapsed() >= CURSOR_IDLE_HIDE {
            // Sin movimiento un rato: esconder el puntero (como YouTube).
            self.sdl.mouse().show_cursor(false);
            self.cursor_visible = false;
        }

        for event in events {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Q),
                    ..
                } => return false,
                // Esc: como en YouTube, primero sale de pantalla completa; si ya
                // estás en ventana, cierra.
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    if self.is_fullscreen() {
                        self.exit_fullscreen();
                    } else {
                        return false;
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::F),
                    ..
                } => self.toggle_fullscreen(),
                // Doble click: alternar pantalla completa (igual que YouTube).
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    clicks: 2,
                    ..
                } => self.toggle_fullscreen(),
                Event::KeyDown {
                    keycode: Some(Keycode::Z),
                    ..
                } => self.fill = !self.fill,
                _ => {}
            }
        }
        true
    }

    fn toggle_fullscreen(&mut self) {
        let window = self.canvas.window_mut();
        let next = match window.fullscreen_state() {
            FullscreenType::Off => FullscreenType::Desktop,
            _ => FullscreenType::Off,
        };
        let _ = window.set_fullscreen(next);
    }

    fn is_fullscreen(&self) -> bool {
        !matches!(self.canvas.window().fullscreen_state(), FullscreenType::Off)
    }

    fn exit_fullscreen(&mut self) {
        let _ = self.canvas.window_mut().set_fullscreen(FullscreenType::Off);
    }
}

/// Mapea el formato de píxel de FFmpeg al de SDL. Solo soporta los dos caminos
/// del decoder: YUV420P (software) y NV12 (hardware 8-bit).
fn sdl_format(pixel: Pixel) -> Result<PixelFormatEnum, String> {
    match pixel {
        Pixel::YUV420P => Ok(PixelFormatEnum::IYUV),
        Pixel::NV12 => Ok(PixelFormatEnum::NV12),
        other => Err(format!(
            "formato de píxel no soportado por el renderer: {other:?} \
             (¿decode 10-bit HDR? hoy sólo NV12 8-bit y YUV420P)"
        )),
    }
}

/// Rectángulo destino que mete (src_w x src_h) dentro de (win_w x win_h) al
/// máximo posible sin deformar, centrado.
fn fit_centered(src_w: u32, src_h: u32, win_w: u32, win_h: u32) -> Rect {
    if src_w == 0 || src_h == 0 {
        return Rect::new(0, 0, win_w.max(1), win_h.max(1));
    }
    // Punto fijo, sin floats. La dimensión que limita queda clavada al borde y la
    // otra se deriva de ella con un solo truncado -> aspecto consistente.
    let (dst_w, dst_h) = if win_w as u64 * src_h as u64 <= win_h as u64 * src_w as u64 {
        // Limitado por ancho.
        (win_w, (win_w as u64 * src_h as u64 / src_w as u64) as u32)
    } else {
        // Limitado por alto.
        ((win_h as u64 * src_w as u64 / src_h as u64) as u32, win_h)
    };
    let x = (win_w as i32 - dst_w as i32) / 2;
    let y = (win_h as i32 - dst_h as i32) / 2;
    Rect::new(x, y, dst_w.max(1), dst_h.max(1))
}

/// Rectángulo destino que CUBRE la ventana (src llena win en ambos ejes,
/// recortando el sobrante), preservando la proporción, centrado. El recorte lo
/// hace SDL al clipear el rect contra la ventana.
fn fill_centered(src_w: u32, src_h: u32, win_w: u32, win_h: u32) -> Rect {
    if src_w == 0 || src_h == 0 {
        return Rect::new(0, 0, win_w.max(1), win_h.max(1));
    }
    // Cubrir = la escala mayor; clava el eje que primero llenaría y deja el otro
    // sobresalir (se recorta). Un solo truncado para proporción consistente.
    let (dst_w, dst_h) = if win_w as u64 * src_h as u64 >= win_h as u64 * src_w as u64 {
        (win_w, (win_w as u64 * src_h as u64 / src_w as u64) as u32)
    } else {
        ((win_h as u64 * src_w as u64 / src_h as u64) as u32, win_h)
    };
    let x = (win_w as i32 - dst_w as i32) / 2;
    let y = (win_h as i32 - dst_h as i32) / 2;
    Rect::new(x, y, dst_w.max(1), dst_h.max(1))
}

/// Monitor con más píxeles: heurística de "el mejor". Suele ser el externo y no
/// el panel chico del notebook. Abrir ahí por defecto evita el recorte y el
/// downscale fuerte que se veían en una pantalla chica. Si algo falla, 0 (primario).
fn best_display(video: &sdl2::VideoSubsystem) -> usize {
    let n = video.num_video_displays().unwrap_or(1).max(1);
    (0..n)
        .max_by_key(|&i| {
            video
                .display_bounds(i)
                .map(|b| u64::from(b.width()) * u64::from(b.height()))
                .unwrap_or(0)
        })
        .unwrap_or(0) as usize
}

/// Tamaño de ventana = el mayor que entra en [WINDOW_MARGIN] de (avail_w, avail_h)
/// preservando el aspecto del teléfono. Adaptarse al monitor real (no a un alto
/// fijo) es lo que hace que se vea bien en CUALQUIER pantalla: nunca más grande
/// que el monitor (no recorta) y lo más grande posible (no se ve borroso).
fn window_size_for(phone_w: u32, phone_h: u32, avail_w: u32, avail_h: u32) -> (u32, u32) {
    let max_w = (f64::from(avail_w) * WINDOW_MARGIN) as u32;
    let max_h = (f64::from(avail_h) * WINDOW_MARGIN) as u32;
    let r = fit_centered(phone_w.max(1), phone_h.max(1), max_w.max(1), max_h.max(1));
    (r.width().max(1), r.height().max(1))
}

fn scaled_window(width: u32, height: u32) -> (u32, u32) {
    if height <= DEFAULT_WINDOW_HEIGHT {
        return (width.max(1), height.max(1));
    }
    let w = (width as u64 * DEFAULT_WINDOW_HEIGHT as u64 / height as u64) as u32;
    (w.max(1), DEFAULT_WINDOW_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::fit_centered;

    #[test]
    fn telefono_vertical_en_monitor_ancho_pone_barras_laterales() {
        // 1440x3120 (vertical) en 1920x1080 (ancho): limitado por alto, centrado en x.
        let r = fit_centered(1440, 3120, 1920, 1080);
        assert_eq!(r.height(), 1080);
        assert_eq!(r.width(), 498); // 1080 * 1440/3120
        assert!(r.x() > 0);
        assert_eq!(r.y(), 0);
    }

    #[test]
    fn preserva_aspecto_sin_estirar() {
        // aspecto 1:2 en una ventana cuadrada -> 500x1000, no 1000x1000.
        let r = fit_centered(1000, 2000, 1000, 1000);
        assert_eq!((r.width(), r.height()), (500, 1000));
    }

    #[test]
    fn dimensiones_cero_no_paniquean() {
        let r = fit_centered(0, 0, 800, 600);
        assert_eq!((r.width(), r.height()), (800, 600));
    }

    #[test]
    fn fill_cubre_y_recorta() {
        // video más ancho que la ventana -> llena el alto, sobresale y recorta los lados.
        let r = super::fill_centered(2400, 1108, 2560, 1440);
        assert_eq!(r.height(), 1440);
        assert!(r.width() > 2560);
        assert!(r.x() < 0);
    }

    #[test]
    fn fill_aspecto_exacto_sin_recorte() {
        let r = super::fill_centered(1600, 900, 3200, 1800);
        assert_eq!((r.width(), r.height()), (3200, 1800));
    }

    #[test]
    fn ventana_entra_en_monitor_chico_sin_recortar() {
        // El bug: 1440x3120 en un monitor de 1536x864 abría a 900px de alto (más
        // que la pantalla) -> recorte arriba/abajo. Ahora entra en 90% del alto.
        let (w, h) = super::window_size_for(1440, 3120, 1536, 864);
        assert!(h <= 864, "no debe superar el alto del monitor");
        assert!(w <= 1536, "no debe superar el ancho del monitor");
        assert_eq!(h, (864.0 * super::WINDOW_MARGIN) as u32); // limitado por alto
    }

    #[test]
    fn ventana_aprovecha_monitor_grande() {
        // En 2560x1440 la ventana es mucho mayor que la vieja (415x900): menos
        // downscale, más nítido.
        let (_w, h) = super::window_size_for(1440, 3120, 2560, 1440);
        assert_eq!(h, (1440.0 * super::WINDOW_MARGIN) as u32);
    }

    #[test]
    fn fill_telefono_vertical_en_monitor_ancho_recorta_arriba_abajo() {
        // 1080x1920 (vertical) en 1920x1080 (ancho): llena el ancho, sobresale en
        // alto -> se recorta arriba/abajo, centrado.
        let r = super::fill_centered(1080, 1920, 1920, 1080);
        assert_eq!(r.width(), 1920);
        assert!(r.height() > 1080);
        assert!(r.y() < 0);
        assert_eq!(r.x(), 0);
    }
}
