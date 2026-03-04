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
        const HIDDEN        = 0b1000_0000;
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

/// Standard 256-color ANSI palette.
/// 0-7: standard colors, 8-15: bright colors, 16-231: 6x6x6 color cube, 232-255: grayscale ramp.
const ANSI_256_PALETTE: [[u8; 3]; 256] = {
    let mut palette = [[0u8; 3]; 256];
    // Standard colors (0-7)
    palette[0] = [0, 0, 0];       // black
    palette[1] = [205, 0, 0];     // red
    palette[2] = [0, 205, 0];     // green
    palette[3] = [205, 205, 0];   // yellow
    palette[4] = [0, 0, 238];     // blue
    palette[5] = [205, 0, 205];   // magenta
    palette[6] = [0, 205, 205];   // cyan
    palette[7] = [229, 229, 229]; // white
    // Bright colors (8-15)
    palette[8]  = [127, 127, 127]; // bright black
    palette[9]  = [255, 0, 0];     // bright red
    palette[10] = [0, 255, 0];     // bright green
    palette[11] = [255, 255, 0];   // bright yellow
    palette[12] = [92, 92, 255];   // bright blue
    palette[13] = [255, 0, 255];   // bright magenta
    palette[14] = [0, 255, 255];   // bright cyan
    palette[15] = [255, 255, 255]; // bright white
    // 6x6x6 color cube (16-231)
    let mut i = 16;
    let mut r = 0u8;
    while r < 6 {
        let mut g = 0u8;
        while g < 6 {
            let mut b = 0u8;
            while b < 6 {
                let rv = if r == 0 { 0 } else { 55 + 40 * r };
                let gv = if g == 0 { 0 } else { 55 + 40 * g };
                let bv = if b == 0 { 0 } else { 55 + 40 * b };
                palette[i] = [rv, gv, bv];
                i += 1;
                b += 1;
            }
            g += 1;
        }
        r += 1;
    }
    // Grayscale ramp (232-255)
    let mut j = 0u8;
    while j < 24 {
        let v = 8 + 10 * j;
        palette[232 + j as usize] = [v, v, v];
        j += 1;
    }
    palette
};

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
    /// Named and Indexed colors are resolved against the standard ANSI 256-color palette.
    /// Default returns `None` — the caller must resolve against the active theme.
    pub fn to_rgba_f32(self) -> Option<[f32; 4]> {
        match self {
            Color::Default => None,
            Color::Named(idx) => {
                let [r, g, b] = ANSI_256_PALETTE[idx.min(15) as usize];
                Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
            }
            Color::Indexed(idx) => {
                let [r, g, b] = ANSI_256_PALETTE[idx as usize];
                Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
            }
            Color::Rgb { r, g, b } => {
                Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
            }
            Color::Rgba { r, g, b, a } => {
                Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0])
            }
        }
    }

    /// Convert to RGBA f32 with a fallback for Default color.
    pub fn to_rgba_f32_or(self, default: [f32; 4]) -> [f32; 4] {
        self.to_rgba_f32().unwrap_or(default)
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
