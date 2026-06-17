//! Decode (FFmpeg) y render (SDL2) del stream de skry.
//!
//! El [`PresentationClock`] convierte los `pts` (µs del server) en tiempos de
//! pared, de modo que cada frame se muestra a su ritmo real — esto es lo que
//! hace que el video se vea exactamente como el teléfono (sin la deriva de
//! reproducir un stream de ritmo variable a una tasa fija).

pub mod decoder;
pub mod renderer;

pub use decoder::{DecodedFrame, Decoder};
pub use renderer::Renderer;

use std::time::{Duration, Instant};

/// Reloj de presentación: agenda cada frame según su `pts` relativo al primero.
#[derive(Debug, Default)]
pub struct PresentationClock {
    base_wall: Option<Instant>,
    base_pts_us: i64,
}

impl PresentationClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bloquea hasta el momento de presentación del frame con el `pts` dado.
    /// El primer frame fija el origen; los siguientes esperan su offset real.
    pub fn wait_for(&mut self, pts_us: i64) {
        match self.base_wall {
            None => {
                self.base_wall = Some(Instant::now());
                self.base_pts_us = pts_us;
            }
            Some(base) => {
                let offset = (pts_us - self.base_pts_us).max(0) as u64;
                let target = base + Duration::from_micros(offset);
                let now = Instant::now();
                if target > now {
                    std::thread::sleep(target - now);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_frame_is_immediate() {
        // El primer frame fija el origen y no debe esperar.
        let mut clock = PresentationClock::new();
        let start = Instant::now();
        clock.wait_for(5_000_000);
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn backwards_pts_does_not_hang() {
        // Antiregresión del clamp: un pts ANTERIOR al base no debe producir un
        // underflow i64->u64 que duerma "para siempre". Debe volver enseguida.
        let mut clock = PresentationClock::new();
        clock.wait_for(1_000_000); // fija el base
        let start = Instant::now();
        clock.wait_for(0); // pts en el pasado: clamp a 0
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn future_pts_waits_approximately() {
        // Un pts adelantado ~30ms debe esperar (cota inferior holgada para no
        // ser flaky); la cota superior queda amplia para CI cargada.
        let mut clock = PresentationClock::new();
        clock.wait_for(0); // base
        let start = Instant::now();
        clock.wait_for(30_000); // 30 ms en el futuro
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(20), "esperó {elapsed:?}");
        assert!(
            elapsed < Duration::from_secs(2),
            "esperó demasiado {elapsed:?}"
        );
    }
}
