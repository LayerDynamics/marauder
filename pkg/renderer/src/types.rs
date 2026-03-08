//! Shared GPU data types: instance structs, uniforms, configuration.

use std::collections::HashMap;
use std::time::Instant;
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

/// Per-cell selection overlay instance data, uploaded to GPU.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SelectionInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
}

/// Per-cell compute overlay instance (search highlights, URL underlines, etc.).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ComputeOverlayInstance {
    /// Position in pixels (x, y).
    pub pos: [f32; 2],
    /// Size in pixels (width, height).
    pub size: [f32; 2],
    /// RGBA color.
    pub color: [f32; 4],
    /// Render mode: 0=fill, 1=underline, 2=outline.
    pub flags: u32,
    /// Padding to align to 16 bytes.
    pub _pad: [f32; 3],
}

/// Uniforms shared across background and text passes.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub viewport_size: [f32; 2],
    pub cell_size: [f32; 2],
    pub grid_offset: [f32; 2],
    pub scale_factor: f32,
    pub _pad: f32,
}

/// Cursor uniforms for the cursor overlay pass.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CursorUniforms {
    pub viewport_size: [f32; 2],  // offset 0,  8 bytes
    pub cursor_pos: [f32; 2],     // offset 8,  8 bytes
    pub cursor_size: [f32; 2],    // offset 16, 8 bytes
    pub _pad0: [f32; 2],          // offset 24, 8 bytes (align cursor_color to 16)
    pub cursor_color: [f32; 4],   // offset 32, 16 bytes
    pub time: f32,                // offset 48, 4 bytes
    pub blink_rate: f32,          // offset 52, 4 bytes
    pub _pad1: [f32; 2],          // offset 56, 8 bytes
    pub _pad2: [f32; 4],          // offset 64, 16 bytes → total 80
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
    pub url: [f32; 4],
}

impl Default for ThemeColors {
    fn default() -> Self {
        // Catppuccin Mocha
        Self {
            background: [0.067, 0.067, 0.106, 1.0], // #11111b
            foreground: [0.804, 0.839, 0.957, 1.0],  // #cdd6f4
            cursor: [0.537, 0.706, 0.980, 1.0],      // #89b4fa
            selection: [0.224, 0.243, 0.322, 0.5],    // #394060 @ 50%
            url: [0.537, 0.706, 0.980, 1.0],            // #89b4fa (blue, same as cursor)
        }
    }
}

/// Tracks GPU memory allocations for diagnostics.
pub struct GpuMemoryTracker {
    allocations: HashMap<&'static str, u64>,
    last_log: Instant,
}

impl GpuMemoryTracker {
    pub fn new() -> Self {
        Self {
            allocations: HashMap::new(),
            last_log: Instant::now(),
        }
    }

    /// Record an allocation under a label.
    pub fn track(&mut self, label: &'static str, bytes: u64) {
        self.allocations.insert(label, bytes);
    }

    /// Remove a tracked allocation.
    pub fn remove(&mut self, label: &'static str) {
        self.allocations.remove(label);
    }

    /// Total tracked GPU memory in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.allocations.values().sum()
    }

    /// Log memory usage at debug level if at least 10 seconds have elapsed.
    pub fn log_if_due(&mut self) {
        if self.last_log.elapsed().as_secs() >= 10 {
            let total = self.total_bytes();
            tracing::debug!(
                total_mb = format!("{:.2}", total as f64 / (1024.0 * 1024.0)),
                allocations = ?self.allocations,
                "GPU memory usage"
            );
            self.last_log = Instant::now();
        }
    }
}

/// Frame statistics for the profiler overlay.
#[derive(Debug, Clone, Copy)]
pub struct FrameStats {
    pub frame_time_ms: f32,
    pub bg_instances: u32,
    pub text_instances: u32,
    pub atlas_usage_pct: f32,
    pub gpu_memory_bytes: u64,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            frame_time_ms: 0.0,
            bg_instances: 0,
            text_instances: 0,
            atlas_usage_pct: 0.0,
            gpu_memory_bytes: 0,
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
    /// Target FPS when idle (no recent activity). Default 10.
    pub idle_fps: u32,
    /// Target FPS when active (recent PTY output, input, mouse). Default 120.
    pub active_fps: u32,
    /// Background opacity (0.0 = fully transparent, 1.0 = fully opaque). Default 1.0.
    pub opacity: f32,
    /// Seconds of inactivity before switching from active_fps to idle_fps. Default 2.
    pub idle_threshold_secs: u64,
    /// Enable subpixel antialiasing for text rendering.
    /// Default: true on macOS, false elsewhere.
    pub subpixel_aa: bool,
    /// OpenType font features to enable/disable.
    /// Keys are feature tags (e.g. "liga", "calt", "dlig"), values are enabled state.
    pub font_features: FontFeatures,
}

/// OpenType font feature toggles for ligatures and stylistic sets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontFeatures {
    /// Standard ligatures (fi, fl, etc.). Default true.
    pub liga: bool,
    /// Contextual alternates (font-dependent). Default true.
    pub calt: bool,
    /// Discretionary ligatures (decorative). Default false.
    pub dlig: bool,
}

impl Default for FontFeatures {
    fn default() -> Self {
        Self {
            liga: true,
            calt: true,
            dlig: false,
        }
    }
}

impl Default for RendererConfig {
    fn default() -> Self {
        let subpixel_aa = cfg!(target_os = "macos");
        Self {
            font_family: "monospace".into(),
            font_size: 14.0,
            line_height: 1.2,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            theme: ThemeColors::default(),
            idle_fps: 10,
            active_fps: 120,
            opacity: 1.0,
            idle_threshold_secs: 2,
            subpixel_aa,
            font_features: FontFeatures::default(),
        }
    }
}
