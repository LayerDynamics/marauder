use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

/// GPU-side cell representation. Must match WGSL struct layout.
/// 24 bytes per cell, tightly packed.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, Serialize, Deserialize)]
pub struct GpuCell {
    /// Unicode codepoint of the character.
    pub codepoint: u32,
    /// Foreground color packed as RGBA (one byte each).
    pub fg_packed: u32,
    /// Background color packed as RGBA (one byte each).
    pub bg_packed: u32,
    /// Cell attribute flags (bold, italic, underline, etc.).
    pub flags: u32,
    /// Row index in the grid.
    pub row: u32,
    /// Column index in the grid.
    pub col: u32,
}

/// Uniform buffer for search compute shader. Must match WGSL layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SearchParams {
    pub pattern_len: u32,
    pub total_rows: u32,
    pub cols: u32,
    pub max_results: u32,
}

/// Uniform buffer for URL detection compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct UrlDetectParams {
    pub total_rows: u32,
    pub cols: u32,
    pub row_start: u32,
    pub row_end: u32,
    pub max_results: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

/// Uniform buffer for selection extraction compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SelectionParams {
    pub start_row: u32,
    pub start_col: u32,
    pub end_row: u32,
    pub end_col: u32,
    pub cols: u32,
    pub max_output: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

/// Uniform buffer for highlight compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct HighlightParams {
    pub total_rows: u32,
    pub cols: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

/// A search match result returned from the GPU.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub row: u32,
    pub col: u32,
    pub length: u32,
}

/// A detected URL position returned from the GPU.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlMatch {
    pub row: u32,
    pub start_col: u32,
    pub end_col: u32,
}

/// Semantic highlight categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum HighlightCategory {
    None = 0,
    Number = 1,
    FilePath = 2,
    Flag = 3,
    Operator = 4,
}

impl HighlightCategory {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Number,
            2 => Self::FilePath,
            3 => Self::Flag,
            4 => Self::Operator,
            _ => Self::None,
        }
    }
}

/// A highlight result for a cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightResult {
    pub row: u32,
    pub col: u32,
    pub category: HighlightCategory,
}

/// A highlight rule provided by the user/extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightRule {
    pub pattern: String,
    pub category: String,
    pub color: String,
}

/// Pack RGBA bytes into a u32: R in MSB, A in LSB.
pub fn pack_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (r as u32) << 24 | (g as u32) << 16 | (b as u32) << 8 | (a as u32)
}

/// Default foreground color (white) as packed RGBA.
pub const DEFAULT_FG_PACKED: u32 = 0xFF_FF_FF_FF; // white, opaque
/// Default background color (black) as packed RGBA.
pub const DEFAULT_BG_PACKED: u32 = 0x00_00_00_FF; // black, opaque
