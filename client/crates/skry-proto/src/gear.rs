use crate::error::{ProtoError, Result};

/// Marcha de fluidez. Cada marcha fija una tasa de refresco objetivo.
///
/// El sistema arranca conservador (`Low`) y sube sólo si el dispositivo y la
/// red lo sostienen; ante inestabilidad baja de marcha automáticamente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Gear {
    Low,
    Mid,
    High,
}

impl Gear {
    /// FPS objetivo de la marcha.
    pub fn target_fps(self) -> u32 {
        match self {
            Gear::Low => 60,
            Gear::Mid => 120,
            Gear::High => 144,
        }
    }

    /// La marcha inmediatamente inferior, o `None` si ya es la más baja.
    pub fn downshift(self) -> Option<Gear> {
        match self {
            Gear::High => Some(Gear::Mid),
            Gear::Mid => Some(Gear::Low),
            Gear::Low => None,
        }
    }

    /// La marcha inmediatamente superior, o `None` si ya es la más alta.
    pub fn upshift(self) -> Option<Gear> {
        match self {
            Gear::Low => Some(Gear::Mid),
            Gear::Mid => Some(Gear::High),
            Gear::High => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Gear::Low => 0,
            Gear::Mid => 1,
            Gear::High => 2,
        }
    }

    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Gear::Low),
            1 => Ok(Gear::Mid),
            2 => Ok(Gear::High),
            _ => Err(ProtoError::UnknownDiscriminant {
                kind: "Gear",
                value: v,
            }),
        }
    }

    /// Mapea un FPS pedido por el usuario a la marcha mínima que lo cubre (techo).
    /// Cualquier valor <= 60 cae en `Low`; <= 120 en `Mid`; el resto en `High`.
    /// Ej.: pedir 70 da `Mid` (120), no `Low`.
    pub fn from_fps(fps: u32) -> Gear {
        if fps <= 60 {
            Gear::Low
        } else if fps <= 120 {
            Gear::Mid
        } else {
            Gear::High
        }
    }
}

impl From<Gear> for u8 {
    fn from(g: Gear) -> u8 {
        g.to_u8()
    }
}

impl TryFrom<u8> for Gear {
    type Error = ProtoError;
    fn try_from(v: u8) -> Result<Gear> {
        Gear::from_u8(v)
    }
}
