use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CellAttributes: u16 {
        const BOLD          = 0b0000_0001;
        const ITALIC        = 0b0000_0010;
        const UNDERLINE     = 0b0000_0100;
        const STRIKETHROUGH = 0b0000_1000;
        const BLINK         = 0b0001_0000;
        const DIM           = 0b0010_0000;
        const INVERSE       = 0b0100_0000;
    }
}

impl Serialize for CellAttributes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.bits().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CellAttributes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bits = u16::deserialize(deserializer)?;
        Self::from_bits(bits).ok_or_else(|| serde::de::Error::custom("invalid CellAttributes bits"))
    }
}

/// Terminal color representation supporting all color models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    /// Default foreground or background (inherited from theme).
    Default,
    /// One of the 16 standard ANSI named colors (0-15).
    Named(u8),
    /// 256-color palette index (0-255, includes named colors).
    Indexed(u8),
    /// True color (24-bit RGB).
    Rgb { r: u8, g: u8, b: u8 },
    /// True color with alpha (for overlay/transparency support).
    Rgba { r: u8, g: u8, b: u8, a: u8 },
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::Rgb { r, g, b }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::Rgba { r, g, b, a }
    }

    /// Convert to RGBA f32 array for GPU upload.
    pub fn to_rgba_f32(self) -> [f32; 4] {
        match self {
            Color::Default => [0.0, 0.0, 0.0, 1.0], // resolved at render time
            Color::Named(idx) | Color::Indexed(idx) => {
                // Placeholder — theme resolves these to actual RGB at render time
                let _ = idx;
                [0.0, 0.0, 0.0, 1.0]
            }
            Color::Rgb { r, g, b } => {
                [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
            }
            Color::Rgba { r, g, b, a } => {
                [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
            }
        }
    }

    // Common named color constants
    pub const WHITE: Self = Self::Rgb { r: 255, g: 255, b: 255 };
    pub const BLACK: Self = Self::Rgb { r: 0, g: 0, b: 0 };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: CellAttributes,
    pub hyperlink_id: Option<u32>,
    pub width: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::Default,
            bg: Color::Default,
            attrs: CellAttributes::empty(),
            hyperlink_id: None,
            width: 1,
        }
    }
}
