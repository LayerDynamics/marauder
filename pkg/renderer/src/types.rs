//! Shared GPU data types: instance structs, uniforms, configuration.

use serde::{Deserialize, Serialize};

/// Per-cell background instance data, uploaded to GPU.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BgInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub bg_color: [f32; 4],
}

/// Per-glyph text instance data, uploaded to GPU.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub fg_color: [f32; 4],
    pub uv_rect: [f32; 4],
    pub glyph_offset: [f32; 2],
}

/// Uniforms shared across background and text passes.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub viewport_size: [f32; 2],
    pub cell_size: [f32; 2],
    pub grid_offset: [f32; 2],
    pub _pad: [f32; 2],
}

/// Cursor uniforms for the cursor overlay pass.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CursorUniforms {
    pub viewport_size: [f32; 2],
    pub cursor_pos: [f32; 2],
    pub cursor_size: [f32; 2],
    pub cursor_color: [f32; 4],
    pub time: f32,
    pub blink_rate: f32,
    pub _pad: [f32; 2],
}

/// Cursor rendering style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

impl Default for CursorStyle {
    fn default() -> Self {
        Self::Block
    }
}

/// Theme colors for terminal rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeColors {
    pub background: [f32; 4],
    pub foreground: [f32; 4],
    pub cursor: [f32; 4],
    pub selection: [f32; 4],
}

impl Default for ThemeColors {
    fn default() -> Self {
        // Catppuccin Mocha
        Self {
            background: [0.067, 0.067, 0.106, 1.0], // #11111b
            foreground: [0.804, 0.839, 0.957, 1.0],  // #cdd6f4
            cursor: [0.537, 0.706, 0.980, 1.0],      // #89b4fa
            selection: [0.224, 0.243, 0.322, 0.5],    // #394060 @ 50%
        }
    }
}

/// Renderer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendererConfig {
    pub font_family: String,
    pub font_size: f32,
    pub line_height: f32,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub theme: ThemeColors,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            font_family: "monospace".into(),
            font_size: 14.0,
            line_height: 1.2,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            theme: ThemeColors::default(),
        }
    }
}
