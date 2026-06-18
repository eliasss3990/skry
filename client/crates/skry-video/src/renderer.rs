//! Ventana de render con SDL2. Muestra frames YUV420 y maneja eventos
//! (salir, alternar pantalla completa).

use ffmpeg_next::frame::Video;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture, TextureCreator};
use sdl2::video::{FullscreenType, Window, WindowContext};

/// Alto máximo por defecto de la ventana (la pantalla del teléfono es vertical).
const DEFAULT_WINDOW_HEIGHT: u32 = 900;

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
    texture: Texture<'static>,
    event_pump: sdl2::EventPump,
}

impl Renderer {
    pub fn new(width: u32, height: u32, fullscreen: bool) -> Result<Self, String> {
        // Escalado lineal (en vez de nearest): suaviza el redibujo del frame a la
        // ventana, mucho mejor calidad visual. Debe setearse antes de crear la
        // textura/renderer.
        sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "1");

        let sdl = sdl2::init()?;
        let video = sdl.video()?;

        // Ventana a escala (manteniendo aspecto); el texture full-res se escala
        // al copiar. Evita abrir una ventana de 3120 px de alto.
        let (win_w, win_h) = scaled_window(width, height);
        let mut builder = video.window("skry", win_w, win_h);
        builder.position_centered().allow_highdpi();
        if fullscreen {
            builder.fullscreen_desktop();
        }
        let window = builder.build().map_err(|e| e.to_string())?;

        let canvas = window
            .into_canvas()
            .present_vsync()
            .build()
            .map_err(|e| e.to_string())?;

        // El Box::leak es INTENCIONAL y asume **un único Renderer por proceso**
        // (es el caso de skry). El leak hace que el TextureCreator viva hasta el
        // fin del proceso, así la `Texture<'static>` nunca queda colgada. Si
        // alguna vez se instancian varios Renderer, esto acumula memoria.
        let texture_creator: &'static TextureCreator<WindowContext> =
            Box::leak(Box::new(canvas.texture_creator()));
        let texture = texture_creator
            .create_texture_streaming(PixelFormatEnum::IYUV, width, height)
            .map_err(|e| e.to_string())?;

        let event_pump = sdl.event_pump()?;

        Ok(Self {
            canvas,
            _texture_creator: texture_creator,
            texture,
            event_pump,
        })
    }

    /// Sube el frame YUV420P a la textura y lo presenta (escalado a la ventana).
    pub fn present(&mut self, frame: &Video) -> Result<(), String> {
        self.texture
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
        self.canvas.clear();
        self.canvas.copy(&self.texture, None, None)?;
        self.canvas.present();
        Ok(())
    }

    /// Procesa la cola de eventos. Devuelve `false` si se pidió cerrar.
    pub fn pump(&mut self) -> bool {
        // Colectar primero: el iterador de poll_iter mantiene prestado
        // `event_pump`, y procesar abajo necesita `&mut self` (toggle_fullscreen).
        let events: Vec<Event> = self.event_pump.poll_iter().collect();
        for event in events {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Q),
                    ..
                } => return false,
                Event::KeyDown {
                    keycode: Some(Keycode::F),
                    ..
                } => self.toggle_fullscreen(),
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
}

fn scaled_window(width: u32, height: u32) -> (u32, u32) {
    if height <= DEFAULT_WINDOW_HEIGHT {
        return (width.max(1), height.max(1));
    }
    let w = (width as u64 * DEFAULT_WINDOW_HEIGHT as u64 / height as u64) as u32;
    (w.max(1), DEFAULT_WINDOW_HEIGHT)
}
