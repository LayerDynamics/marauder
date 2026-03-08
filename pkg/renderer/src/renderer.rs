//! Main renderer: coordinates wgpu surface, pipelines, atlas, and frame production.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use marauder_event_bus::lock_or_log;
use marauder_grid::{Cell, Grid};
use tracing;

use crate::atlas::GlyphAtlas;
use crate::images::{ImageId, ImageInstance, ImageManager};
use crate::pipelines;

/// Block on a future, safely handling both tokio and non-tokio contexts.
///
/// If called inside an active tokio runtime, spawns a blocking task to avoid
/// the panic that `pollster::block_on` triggers within async contexts.
/// Otherwise falls back to `pollster::block_on`.
fn block_on_safe<F: std::future::Future>(fut: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => pollster::block_on(fut),
    }
}
use crate::types::*;

/// GPU resources only needed for on-screen rendering (pipelines + bind groups).
///
/// `None` in headless mode to avoid shader compilation and GPU memory waste.
#[allow(dead_code)]
struct RenderPipelines {
    bg_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    selection_pipeline: wgpu::RenderPipeline,
    cursor_pipeline: wgpu::RenderPipeline,
    uniform_bind_group: wgpu::BindGroup,
    text_bind_group: wgpu::BindGroup,
    text_bind_group_layout: wgpu::BindGroupLayout,
    cursor_bind_group: wgpu::BindGroup,
    /// Subpixel text pipeline (uses RGBA atlas, optional).
    subpixel_text_pipeline: Option<wgpu::RenderPipeline>,
    /// Bind group for the subpixel RGBA atlas texture.
    subpixel_text_bind_group: Option<wgpu::BindGroup>,
    /// Compute overlay pipeline (search highlights, URL underlines, outlines).
    compute_overlay_pipeline: wgpu::RenderPipeline,
    /// Image render pipeline (inline images: Sixel, iTerm2).
    image_pipeline: wgpu::RenderPipeline,
}

/// The GPU-accelerated terminal renderer.
pub struct Renderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: wgpu::SurfaceConfiguration,
    scale_factor: f32,

    // Render pipelines + bind groups (None in headless mode)
    render: Option<RenderPipelines>,

    // Buffers (needed for both headless instance building and rendering)
    uniform_buffer: wgpu::Buffer,
    cursor_uniform_buffer: wgpu::Buffer,
    /// Double-buffered BG instance buffers (ping-pong).
    bg_instance_buffers: [wgpu::Buffer; 2],
    /// Double-buffered text instance buffers (ping-pong) — R8 atlas glyphs.
    text_instance_buffers: [wgpu::Buffer; 2],
    /// Double-buffered subpixel text instance buffers (ping-pong) — RGBA atlas glyphs.
    subpixel_text_instance_buffers: [wgpu::Buffer; 2],
    atlas_texture: wgpu::Texture,
    /// Overflow R8 atlas textures + bind groups for pages beyond page 0.
    overflow_atlas_pages: Vec<(wgpu::Texture, wgpu::BindGroup)>,
    /// Instance buffer for overflow atlas glyphs (shared across overflow pages).
    overflow_text_instance_buffers: [wgpu::Buffer; 2],
    /// Per-page draw ranges for overflow atlas: (overflow_page_idx, count, offset).
    overflow_page_draws: Vec<(usize, u32, u32)>,

    /// Double-buffered selection instance buffers (ping-pong).
    selection_instance_buffers: [wgpu::Buffer; 2],
    /// Current write buffer index (0 or 1). Draw uses 1 - this.
    current_buffer_index: usize,
    /// Subpixel RGBA atlas texture (Rgba8Unorm, created when subpixel_aa enabled).
    #[allow(dead_code)]
    subpixel_atlas_texture: Option<wgpu::Texture>,

    // State
    atlas: GlyphAtlas,
    config: RendererConfig,
    bg_instance_count: u32,
    text_instance_count: u32,
    subpixel_text_instance_count: u32,
    selection_instance_count: u32,
    start_time: Instant,
    max_cells: usize,

    /// Whether the cursor is visible (DECTCEM mode 25).
    cursor_visible: bool,

    /// Registered overlay layers, keyed by layer ID.
    overlays: HashMap<u32, OverlayConfig>,

    /// Pane border quads to render between selection and cursor passes.
    pane_borders: Vec<PaneBorder>,
    pane_border_instance_buffer: wgpu::Buffer,
    pane_border_instance_count: u32,

    /// Cached background instances for incremental dirty-row updates.
    cached_bg_instances: Vec<BgInstance>,
    /// Cached grid dimensions for detecting resize (requiring full rebuild).
    cached_cols: usize,
    cached_rows: usize,

    /// Subpixel vertical scroll offset in pixels for smooth scrolling.
    scroll_y_offset: f32,

    /// Pixel offset to push the grid below opaque chrome (tab bar).
    /// Set via `set_grid_offset(x, y)`.
    grid_offset: [f32; 2],

    /// Last time activity was detected (PTY output, key input, mouse event).
    last_activity: Instant,
    /// Last time a frame was rendered.
    last_frame: Instant,

    /// Custom overlay render pipelines compiled from extension-provided WGSL shaders.
    overlay_pipelines: HashMap<u32, wgpu::RenderPipeline>,

    /// Double-buffered compute overlay instance buffers (ping-pong).
    compute_overlay_buffers: [wgpu::Buffer; 2],
    /// Number of compute overlay instances to draw.
    compute_overlay_count: u32,

    /// Inline image manager (Sixel, iTerm2).
    image_manager: ImageManager,
    /// Image instance buffer (one quad per image).
    image_instance_buffer: wgpu::Buffer,
    /// Number of image instances to draw.
    image_instance_count: u32,

    /// GPU memory allocation tracker for diagnostics.
    gpu_memory: GpuMemoryTracker,
    /// Frame profiler overlay enabled.
    profiler_enabled: bool,
    /// Profiler text instance buffer.
    profiler_text_buffer: wgpu::Buffer,
    /// Number of text instances for profiler overlay.
    profiler_instance_count: u32,
    /// Latest frame statistics.
    frame_stats: FrameStats,
}

/// A pane divider border rendered as a colored quad.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[repr(C)]
pub struct PaneBorder {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: [f32; 4],
}

/// Configuration for an overlay layer (search highlights, selection, extension UI).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct OverlayConfig {
    /// Unique layer identifier.
    pub layer_id: u32,
    /// Whether this overlay is currently visible.
    pub visible: bool,
    /// Whether this overlay comes from a trusted source (required for custom shaders).
    /// Only overlays with `trusted: true` may include `shader_wgsl` in their data.
    #[serde(default)]
    pub trusted: bool,
    /// Overlay-specific configuration (JSON pass-through for extensibility).
    #[serde(default)]
    pub data: serde_json::Value,
}

/// Maximum cells we preallocate buffers for (resized on demand).
const INITIAL_MAX_CELLS: usize = 250 * 80;

/// Build a single row's BG instances into the given slice (free function to avoid borrow conflicts).
fn build_row_bg(
    grid: &Grid,
    row: usize,
    cols: usize,
    cw: f32,
    ch: f32,
    opacity: f32,
    grid_offset: [f32; 2],
    scroll_y_offset: f32,
    default_bg: [f32; 4],
    out: &mut [BgInstance],
) {
    // Fast path: direct slice when not scrolled (no Cow overhead).
    // Falls back to visible_row() for scrollback deserialization.
    let cow_storage;
    let row_cells: &[Cell] = if let Some(slice) = grid.visible_row_slice(row) {
        slice
    } else {
        match grid.visible_row(row) {
            Some(cow) => {
                cow_storage = cow;
                &cow_storage
            }
            None => {
                for inst in out.iter_mut() {
                    *inst = BgInstance { pos: [0.0, 0.0], size: [0.0, 0.0], bg_color: [0.0; 4] };
                }
                return;
            }
        }
    };
    for col in 0..cols {
        let px = col as f32 * cw + grid_offset[0];
        let py = row as f32 * ch - scroll_y_offset + grid_offset[1];

        if col < row_cells.len() {
            let cell = &row_cells[col];
            if cell.width == 0 {
                out[col] = BgInstance { pos: [px, py], size: [0.0, 0.0], bg_color: [0.0; 4] };
            } else {
                let cell_bg_width = if cell.width == 2 { cw * 2.0 } else { cw };
                let mut bg_color = cell.bg.to_rgba_f32_or(default_bg);
                bg_color[3] *= opacity;
                out[col] = BgInstance { pos: [px, py], size: [cell_bg_width, ch], bg_color };
            }
        } else {
            let mut bg_color = default_bg;
            bg_color[3] *= opacity;
            out[col] = BgInstance { pos: [px, py], size: [cw, ch], bg_color };
        }
    }
}

impl Renderer {
    /// Create a new renderer on the given window surface.
    pub async fn new<W: raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle + Send + Sync + 'static>(
        window: Arc<W>,
        width: u32,
        height: u32,
        scale_factor: f32,
        config: RendererConfig,
    ) -> Result<Self, crate::RendererError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // SAFETY: The window handle is valid for the lifetime of the surface,
        // which is owned by the Renderer and lives as long as the window.
        let surface = instance.create_surface(window)
            .map_err(|e| crate::RendererError::SurfaceCreation(e.to_string()))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),

                force_fallback_adapter: false,
            })
            .await
            .ok_or(crate::RendererError::NoAdapter)?;

        tracing::info!(
            adapter = adapter.get_info().name,
            backend = ?adapter.get_info().backend,
            "GPU adapter selected"
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("marauder_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
            }, None)
            .await
            .map_err(|e| crate::RendererError::DeviceRequest(e.to_string()))?;
        // Use non-fatal error handling so validation errors log instead of panic.
        device.on_uncaptured_error(Box::new(|err| {
            tracing::error!("wgpu device error: {}", err);
        }));

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        // Prefer PreMultiplied alpha for proper transparency compositing with Tauri's
        // transparent webview on macOS. Fallback to first available mode.
        let alpha_mode = surface_caps
            .alpha_modes
            .iter()
            .find(|m| **m == wgpu::CompositeAlphaMode::PreMultiplied)
            .or_else(|| surface_caps.alpha_modes.iter().find(|m| **m == wgpu::CompositeAlphaMode::PostMultiplied))
            .copied()
            .unwrap_or(surface_caps.alpha_modes[0]);
        tracing::info!(?alpha_mode, available_modes = ?surface_caps.alpha_modes, "Surface alpha mode selected");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            desired_maximum_frame_latency: 2,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        Self::build(device, queue, Some(surface), surface_config, scale_factor, format, config)
    }

    /// Get the cell dimensions in pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        self.atlas.cell_size()
    }

    /// Resize the wgpu surface.
    pub fn resize_surface(&mut self, width: u32, height: u32, scale_factor: f32) {
        if width == 0 || height == 0 {
            return;
        }
        self.scale_factor = scale_factor;
        self.surface_config.width = width;
        self.surface_config.height = height;
        if let Some(ref surface) = self.surface {
            surface.configure(&self.device, &self.surface_config);
        }
    }

    /// Calculate grid dimensions for the given surface size.
    pub fn grid_dimensions(&self) -> (u16, u16) {
        let (cw, ch) = self.cell_size();
        let cols = (self.surface_config.width as f32 / cw).floor() as u16;
        let rows = (self.surface_config.height as f32 / ch).floor() as u16;
        (rows.max(1), cols.max(1))
    }

    /// Update instance buffers from the grid state, then render a frame.
    pub fn render_frame(&mut self, grid: &Arc<Mutex<Grid>>) -> Result<(), crate::RendererError> {
        let frame_start = Instant::now();

        // Lock grid, build instance data, release lock before GPU work
        let (text_instances, subpixel_text_instances, selection_instances, cursor_row, cursor_col, cursor_visible) = {
            let mut grid = lock_or_log(&grid, "renderer::render_frame");
            let result = self.build_instances(&grid);
            grid.clear_dirty();
            result
        };

        self.upload_instances(&text_instances, &subpixel_text_instances, &selection_instances, cursor_row, cursor_col, cursor_visible);

        // Re-upload atlas if new glyphs were rasterized
        if self.atlas.is_dirty() {
            self.upload_atlas();
            self.atlas.clear_dirty();
        }
        if self.atlas.is_subpixel_dirty() {
            self.upload_subpixel_atlas();
            self.atlas.clear_subpixel_dirty();
        }

        // Build profiler overlay if enabled
        if self.profiler_enabled {
            self.build_profiler_overlay();
        }

        let result = self.encode_and_present();

        // Update frame stats
        self.frame_stats = FrameStats {
            frame_time_ms: frame_start.elapsed().as_secs_f32() * 1000.0,
            bg_instances: self.bg_instance_count,
            text_instances: self.text_instance_count,
            atlas_usage_pct: self.atlas_usage_percent(),
            gpu_memory_bytes: self.gpu_memory.total_bytes(),
        };
        self.gpu_memory.log_if_due();

        result
    }

    /// Enable or disable the frame profiler overlay.
    pub fn set_profiler_enabled(&mut self, enabled: bool) {
        self.profiler_enabled = enabled;
        if !enabled {
            self.profiler_instance_count = 0;
        }
    }

    /// Whether the profiler overlay is currently enabled.
    pub fn profiler_enabled(&self) -> bool {
        self.profiler_enabled
    }

    /// Get the latest frame statistics.
    pub fn frame_stats(&self) -> &FrameStats {
        &self.frame_stats
    }

    /// Calculate atlas texture usage as a percentage.
    fn atlas_usage_percent(&self) -> f32 {
        let total = self.atlas.atlas_size() as f32;
        let used_y = self.atlas.pack_y() as f32 + self.atlas.row_height() as f32;
        (used_y / total * 100.0).min(100.0)
    }

    /// Build profiler text instances for the stats overlay.
    fn build_profiler_overlay(&mut self) {
        let fps = if self.frame_stats.frame_time_ms > 0.0 {
            (1000.0 / self.frame_stats.frame_time_ms) as u32
        } else {
            0
        };
        let idle_threshold = std::time::Duration::from_secs(self.config.idle_threshold_secs);
        let is_active = self.last_activity.elapsed() < idle_threshold;
        let target_fps = if is_active { self.config.active_fps } else { self.config.idle_fps };
        let stats_text = format!(
            "FPS:{}/{} Frame:{:.1}ms BG:{} Text:{} Ovl:{} Atlas:{:.0}% VRAM:{:.1}MB",
            fps,
            target_fps,
            self.frame_stats.frame_time_ms,
            self.frame_stats.bg_instances,
            self.frame_stats.text_instances,
            self.compute_overlay_count,
            self.frame_stats.atlas_usage_pct,
            self.frame_stats.gpu_memory_bytes as f64 / (1024.0 * 1024.0),
        );

        let (cw, _ch) = self.cell_size();
        let ascent = self.atlas.ascent();
        let viewport_w = self.surface_config.width as f32;
        let start_x = viewport_w - (stats_text.len() as f32 * cw) - 4.0;
        let start_y = 4.0;

        let mut profiler_instances = Vec::with_capacity(stats_text.len());
        for (i, c) in stats_text.chars().enumerate() {
            if c == ' ' {
                continue;
            }
            if let Some(glyph) = self.atlas.get_or_insert(c) {
                let px = start_x + i as f32 * cw;
                profiler_instances.push(TextInstance {
                    pos: [px, start_y],
                    size: glyph.pixel_size,
                    fg_color: [0.0, 1.0, 0.0, 0.8], // green overlay text
                    uv_rect: glyph.uv,
                    glyph_offset: [glyph.offset[0], glyph.offset[1] + ascent],
                });
            }
        }

        // Clamp to buffer capacity (128 instances) to prevent wgpu validation error
        let max_profiler_instances = 128;
        if profiler_instances.len() > max_profiler_instances {
            profiler_instances.truncate(max_profiler_instances);
        }
        if !profiler_instances.is_empty() {
            self.queue.write_buffer(
                &self.profiler_text_buffer,
                0,
                bytemuck::cast_slice(&profiler_instances),
            );
        }
        self.profiler_instance_count = profiler_instances.len() as u32;
    }

    /// Build background, text, and selection instance data from the grid (public for FFI).
    pub fn build_instances_from(&mut self, grid: &Grid) -> (Vec<TextInstance>, Vec<TextInstance>, Vec<SelectionInstance>, usize, usize, bool) {
        self.build_instances(grid)
    }

    /// Build background, text, and selection instance data from the grid.
    ///
    /// BG instances use a dense layout (rows * cols) for incremental dirty-row updates.
    /// Only dirty rows are rebuilt when grid dimensions haven't changed.
    /// Text instances are always fully rebuilt (sparse, cheap).
    /// Returns separate text instance vecs for R8 atlas (standard) and RGBA atlas (subpixel).
    fn build_instances(&mut self, grid: &Grid) -> (Vec<TextInstance>, Vec<TextInstance>, Vec<SelectionInstance>, usize, usize, bool) {
        let rows = grid.rows();
        let cols = grid.cols();
        let (cw, ch) = self.cell_size();
        let ascent = self.atlas.ascent();

        let default_bg = self.config.theme.background;
        let default_fg = self.config.theme.foreground;

        let dirty = grid.get_dirty_rows();
        let dimensions_changed = rows != self.cached_rows || cols != self.cached_cols;

        // --- BG instances: incremental update ---
        if dimensions_changed {
            // Full rebuild into dense layout (rows * cols)
            self.cached_bg_instances.resize(rows * cols, BgInstance { pos: [0.0, 0.0], size: [0.0, 0.0], bg_color: [0.0; 4] });
            for row in 0..rows {
                let offset = row * cols;
                build_row_bg(grid, row, cols, cw, ch, self.config.opacity, self.grid_offset, self.scroll_y_offset, default_bg, &mut self.cached_bg_instances[offset..offset + cols]);
            }
            self.cached_rows = rows;
            self.cached_cols = cols;
        } else {
            // Incremental: only rebuild dirty rows
            for (row, &is_dirty) in dirty.iter().enumerate() {
                if is_dirty && row < rows {
                    let offset = row * cols;
                    build_row_bg(grid, row, cols, cw, ch, self.config.opacity, self.grid_offset, self.scroll_y_offset, default_bg, &mut self.cached_bg_instances[offset..offset + cols]);
                }
            }
        }

        // --- Text instances: always full rebuild (sparse, ~30-50% of cells) ---
        // Includes ligature lookahead for common operator sequences.
        let mut text_instances = Vec::with_capacity(rows * cols / 2);
        // Overflow atlas instances keyed by page index (page 1+).
        let mut overflow_text_instances: HashMap<u16, Vec<TextInstance>> = HashMap::new();
        let mut subpixel_text_instances: Vec<TextInstance> = if self.config.subpixel_aa {
            Vec::with_capacity(rows * cols / 2)
        } else {
            Vec::new()
        };

        // Common ligature-forming operator sequences (2-3 chars)
        const LIGATURE_PAIRS: &[&str] = &[
            "==", "!=", "=>", "->", "<-", "<=", ">=", "&&", "||",
            "++", "--", "**", "::", "<<", ">>", "..", "//", "/*",
            "*/", "??", "?.", "=~", "!~", "<>", "|>", "<|",
            "===", "!==", "==>", "-->", "<--", "<=>", ">>>", "<<<",
            "...", "///",
        ];

        for row in 0..rows {
            // Fast path: direct slice avoids Cow discriminant on every cell access.
            let cow_storage;
            let row_cells: &[Cell] = if let Some(slice) = grid.visible_row_slice(row) {
                slice
            } else {
                match grid.visible_row(row) {
                    Some(cow) => { cow_storage = cow; &cow_storage }
                    None => break,
                }
            };
            let mut col = 0usize;
            while col < cols {
                if col >= row_cells.len() {
                    break;
                }
                let cell = &row_cells[col];

                if cell.width == 0 {
                    col += 1;
                    continue;
                }

                // Text (skip spaces and control chars)
                if cell.c != ' ' && !cell.c.is_control() {
                    let px = col as f32 * cw + self.grid_offset[0];
                    let py = row as f32 * ch - self.scroll_y_offset + self.grid_offset[1];
                    let fg_color = cell.fg.to_rgba_f32_or(default_fg);

                    // Try ligature lookahead (up to 3 chars)
                    let mut ligature_matched = false;
                    if col + 1 < row_cells.len() {
                        // Build candidate string from next 2-3 chars
                        let max_look = 3.min(row_cells.len() - col);
                        let chars: Vec<char> = (0..max_look).map(|i| row_cells[col + i].c).collect();

                        // Check 3-char ligatures first, then 2-char
                        for len in (2..=max_look).rev() {
                            let candidate: String = chars[..len].iter().collect();
                            if LIGATURE_PAIRS.iter().any(|&p| p == candidate) {
                                if let Some(glyph) = self.atlas.get_or_insert_ligature(&chars[..len]) {
                                    let inst = TextInstance {
                                        pos: [px, py],
                                        size: glyph.pixel_size,
                                        fg_color,
                                        uv_rect: glyph.uv,
                                        glyph_offset: [glyph.offset[0], glyph.offset[1] + ascent],
                                    };
                                    if glyph.page == 0 {
                                        text_instances.push(inst);
                                    } else {
                                        overflow_text_instances.entry(glyph.page).or_default().push(inst);
                                    }
                                    col += len;
                                    ligature_matched = true;
                                    break;
                                }
                            }
                        }
                    }

                    if !ligature_matched {
                        // Route color emoji to the RGBA subpixel atlas, all other
                        // glyphs to the R8 atlas (or RGBA when subpixel_aa is on).
                        let is_emoji = GlyphAtlas::is_color_emoji(cell.c);
                        let use_subpixel = is_emoji || self.config.subpixel_aa;

                        let glyph_opt = if use_subpixel {
                            self.atlas.get_or_insert_subpixel(cell.c)
                        } else {
                            self.atlas.get_or_insert(cell.c)
                        };

                        if let Some(glyph) = glyph_opt {
                            // CJK wide chars (width=2): scale glyph to span two cells
                            let cell_span = cell.width.max(1) as f32;
                            let glyph_size = if cell.width == 2 {
                                [glyph.pixel_size[0].max(cw * cell_span), glyph.pixel_size[1]]
                            } else {
                                glyph.pixel_size
                            };

                            let inst = TextInstance {
                                pos: [px, py],
                                size: glyph_size,
                                fg_color,
                                uv_rect: glyph.uv,
                                glyph_offset: [glyph.offset[0], glyph.offset[1] + ascent],
                            };
                            if use_subpixel {
                                subpixel_text_instances.push(inst);
                            } else if glyph.page == 0 {
                                text_instances.push(inst);
                            } else {
                                overflow_text_instances.entry(glyph.page).or_default().push(inst);
                            }
                        }
                        col += cell.width.max(1) as usize;
                    }
                } else {
                    col += 1;
                }
            }
        }

        // Selection overlay instances
        let mut selection_instances = Vec::new();
        if let Some(sel) = grid.selection() {
            let sel_color = self.config.theme.selection;
            let (sr, sc) = (sel.start_row, sel.start_col);
            let (er, ec) = (sel.end_row, sel.end_col);
            // Normalize so sr <= er
            let (sr, sc, er, ec) = if sr < er || (sr == er && sc <= ec) {
                (sr, sc, er, ec)
            } else {
                (er, ec, sr, sc)
            };
            for r in sr..=er {
                if r >= rows {
                    break;
                }
                let col_start = if r == sr { sc } else { 0 };
                let col_end = if r == er { ec } else { cols.saturating_sub(1) };
                for c in col_start..=col_end {
                    if c >= cols {
                        break;
                    }
                    selection_instances.push(SelectionInstance {
                        pos: [c as f32 * cw + self.grid_offset[0], r as f32 * ch - self.scroll_y_offset + self.grid_offset[1]],
                        size: [cw, ch],
                        color: sel_color,
                    });
                }
            }
        }

        // Build overflow page draw commands and upload overflow instances.
        if !overflow_text_instances.is_empty() {
            let mut all_overflow: Vec<TextInstance> = Vec::new();
            let mut draws: Vec<(usize, u32, u32)> = Vec::new();
            let mut sorted_pages: Vec<u16> = overflow_text_instances.keys().copied().collect();
            sorted_pages.sort();
            for page in sorted_pages {
                let instances = overflow_text_instances.remove(&page).unwrap();
                let overflow_idx = (page - 1) as usize; // page 1 → overflow index 0
                let offset = all_overflow.len() as u32;
                let count = instances.len() as u32;
                draws.push((overflow_idx, count, offset));
                all_overflow.extend(instances);
            }
            if !all_overflow.is_empty() {
                let write_idx = self.current_buffer_index;
                let data = bytemuck::cast_slice(&all_overflow);
                let buf_size = self.overflow_text_instance_buffers[write_idx].size() as usize;
                if data.len() <= buf_size {
                    self.queue.write_buffer(&self.overflow_text_instance_buffers[write_idx], 0, data);
                }
            }
            self.overflow_page_draws = draws;
        } else {
            self.overflow_page_draws.clear();
        }

        (text_instances, subpixel_text_instances, selection_instances, grid.cursor.row, grid.cursor.col, grid.cursor.visible)
    }

    /// Upload instance data and uniforms to GPU buffers.
    pub fn upload_instances(
        &mut self,
        text_instances: &[TextInstance],
        subpixel_text_instances: &[TextInstance],
        selection_instances: &[SelectionInstance],
        cursor_row: usize,
        cursor_col: usize,
        cursor_visible: bool,
    ) {
        self.cursor_visible = cursor_visible;
        let total_cells = self.cached_bg_instances.len();

        // Grow double buffers if needed (tracks GPU memory)
        if total_cells > self.max_cells {
            self.max_cells = total_cells * 2;
            let bg_size = (self.max_cells * std::mem::size_of::<BgInstance>()) as u64;
            let text_size = (self.max_cells * std::mem::size_of::<TextInstance>()) as u64;
            let sel_size = (self.max_cells * std::mem::size_of::<SelectionInstance>()) as u64;
            self.gpu_memory.track("bg_instance_buffers", bg_size * 2);
            self.gpu_memory.track("text_instance_buffers", text_size * 2);
            self.gpu_memory.track("subpixel_text_instance_buffers", text_size * 2);
            self.gpu_memory.track("selection_instance_buffers", sel_size * 2);
            for i in 0..2 {
                self.bg_instance_buffers[i] = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("bg_instance_buffer"),
                    size: bg_size,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.text_instance_buffers[i] = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("text_instance_buffer"),
                    size: text_size,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.subpixel_text_instance_buffers[i] = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("subpixel_text_instance_buffer"),
                    size: text_size,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.selection_instance_buffers[i] = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("selection_instance_buffer"),
                    size: sel_size,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
        }

        let write_idx = self.current_buffer_index;

        // Upload BG instance data to write buffer (reads from cached_bg_instances directly)
        if !self.cached_bg_instances.is_empty() {
            self.queue.write_buffer(
                &self.bg_instance_buffers[write_idx],
                0,
                bytemuck::cast_slice(&self.cached_bg_instances),
            );
        }
        self.bg_instance_count = self.cached_bg_instances.len() as u32;

        if !text_instances.is_empty() {
            self.queue.write_buffer(
                &self.text_instance_buffers[write_idx],
                0,
                bytemuck::cast_slice(text_instances),
            );
        }
        self.text_instance_count = text_instances.len() as u32;

        if !subpixel_text_instances.is_empty() {
            self.queue.write_buffer(
                &self.subpixel_text_instance_buffers[write_idx],
                0,
                bytemuck::cast_slice(subpixel_text_instances),
            );
        }
        self.subpixel_text_instance_count = subpixel_text_instances.len() as u32;

        if !selection_instances.is_empty() {
            self.queue.write_buffer(
                &self.selection_instance_buffers[write_idx],
                0,
                bytemuck::cast_slice(selection_instances),
            );
        }
        self.selection_instance_count = selection_instances.len() as u32;

        // Upload uniforms
        let (cw, ch) = self.cell_size();
        let uniforms = Uniforms {
            viewport_size: [
                self.surface_config.width as f32,
                self.surface_config.height as f32,
            ],
            cell_size: [cw, ch],
            grid_offset: [0.0, 0.0],
            scale_factor: self.scale_factor,
            _pad: 0.0,
        };
        self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Upload cursor uniforms (skip if cursor is hidden — draw call is already gated)
        if !self.cursor_visible {
            return;
        }
        let elapsed = self.start_time.elapsed().as_secs_f32();
        let (cursor_w, cursor_h) = match self.config.cursor_style {
            CursorStyle::Block => (cw, ch),
            CursorStyle::Underline => (cw, 2.0),
            CursorStyle::Bar => (2.0, ch),
        };
        let cursor_y_offset = match self.config.cursor_style {
            CursorStyle::Underline => ch - 2.0,
            _ => 0.0,
        };

        let cursor_uniforms = CursorUniforms {
            viewport_size: [
                self.surface_config.width as f32,
                self.surface_config.height as f32,
            ],
            cursor_pos: [
                cursor_col as f32 * cw + self.grid_offset[0],
                cursor_row as f32 * ch + cursor_y_offset - self.scroll_y_offset + self.grid_offset[1],
            ],
            cursor_size: [cursor_w, cursor_h],
            _pad0: [0.0, 0.0],
            cursor_color: self.config.theme.cursor,
            time: elapsed,
            blink_rate: if self.config.cursor_blink { 1.0 } else { 0.0 },
            _pad1: [0.0, 0.0],
            _pad2: [0.0, 0.0, 0.0, 0.0],
        };
        self.queue.write_buffer(
            &self.cursor_uniform_buffer,
            0,
            bytemuck::bytes_of(&cursor_uniforms),
        );
    }

    /// Re-upload the glyph atlas texture(s) to GPU, including overflow pages.
    fn upload_atlas(&mut self) {
        // Upload page 0 (primary atlas).
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.atlas.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.atlas.atlas_size()),
                rows_per_image: Some(self.atlas.atlas_size()),
            },
            wgpu::Extent3d {
                width: self.atlas.atlas_size(),
                height: self.atlas.atlas_size(),
                depth_or_array_layers: 1,
            },
        );

        // Create and upload overflow pages (page 1+).
        let atlas_page_count = self.atlas.page_count();
        for page_idx in 1..atlas_page_count {
            let overflow_idx = page_idx - 1;
            let page_pixels = match self.atlas.page_pixels(page_idx) {
                Some(p) => p,
                None => continue,
            };

            // Create texture + bind group for new overflow pages.
            if overflow_idx >= self.overflow_atlas_pages.len() {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(&format!("overflow_atlas_page_{}", page_idx)),
                    size: wgpu::Extent3d {
                        width: self.atlas.atlas_size(),
                        height: self.atlas.atlas_size(),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some(&format!("overflow_atlas_sampler_{}", page_idx)),
                    mag_filter: wgpu::FilterMode::Nearest,
                    min_filter: wgpu::FilterMode::Nearest,
                    ..Default::default()
                });

                // Create bind group using the same layout as the primary text bind group.
                if let Some(ref rp) = self.render {
                    let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("overflow_text_bind_group_{}", page_idx)),
                        layout: &rp.text_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: self.uniform_buffer.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(&sampler),
                            },
                        ],
                    });
                    self.overflow_atlas_pages.push((texture, bind_group));
                } else {
                    // Headless mode — no bind group needed, but track the texture.
                    let dummy_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout: &self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                            label: None,
                            entries: &[],
                        }),
                        entries: &[],
                    });
                    self.overflow_atlas_pages.push((texture, dummy_bind_group));
                }
            }

            // Upload pixel data to the overflow texture.
            let (ref tex, _) = self.overflow_atlas_pages[overflow_idx];
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                page_pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.atlas.atlas_size()),
                    rows_per_image: Some(self.atlas.atlas_size()),
                },
                wgpu::Extent3d {
                    width: self.atlas.atlas_size(),
                    height: self.atlas.atlas_size(),
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Re-upload the subpixel RGBA atlas texture to GPU.
    fn upload_subpixel_atlas(&mut self) {
        let tex = match self.subpixel_atlas_texture.as_ref() {
            Some(t) => t,
            None => return,
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.atlas.subpixel_pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.atlas.atlas_size() * 4),
                rows_per_image: Some(self.atlas.atlas_size()),
            },
            wgpu::Extent3d {
                width: self.atlas.atlas_size(),
                height: self.atlas.atlas_size(),
                depth_or_array_layers: 1,
            },
        );
        // Rebuild subpixel text bind group with updated texture view
        if let Some(ref mut rp) = self.render {
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("subpixel_atlas_sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            rp.subpixel_text_bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("subpixel_text_bind_group"),
                layout: &rp.text_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            }));
        }
    }

    /// Re-upload the glyph atlas to GPU if new glyphs were rasterized.
    pub fn flush_atlas_if_dirty(&mut self) {
        if self.atlas.is_dirty() {
            self.upload_atlas();
            self.atlas.clear_dirty();
        }
        if self.atlas.is_subpixel_dirty() {
            self.upload_subpixel_atlas();
            self.atlas.clear_subpixel_dirty();
        }
    }

    /// Get a reference to the wgpu device.
    pub fn device(&self) -> &wgpu::Device {
        &*self.device
    }

    /// Get a reference to the wgpu queue.
    pub fn queue(&self) -> &wgpu::Queue {
        &*self.queue
    }

    /// Get an Arc-cloned device handle (for sharing with compute).
    /// The returned Arc keeps the device alive independently of the renderer.
    pub fn device_arc(&self) -> Arc<wgpu::Device> {
        Arc::clone(&self.device)
    }

    /// Get an Arc-cloned queue handle (for sharing with compute).
    /// The returned Arc keeps the queue alive independently of the renderer.
    pub fn queue_arc(&self) -> Arc<wgpu::Queue> {
        Arc::clone(&self.queue)
    }

    /// Set the theme colors.
    pub fn set_theme(&mut self, theme: ThemeColors) {
        self.config.theme = theme;
    }

    /// Set the cursor style and blink.
    pub fn set_cursor_style(&mut self, style: CursorStyle, blink: bool) {
        self.config.cursor_style = style;
        self.config.cursor_blink = blink;
    }

    /// Add an inline image. Returns the image ID.
    pub fn add_image(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
        grid_row: u32,
        grid_col: u32,
        width_cells: u32,
        height_cells: u32,
    ) -> ImageId {
        let id = self.image_manager.add_image(
            &self.device, &self.queue, pixels, width, height,
            grid_row, grid_col, width_cells, height_cells,
        );
        self.rebuild_image_instances();
        id
    }

    /// Remove an inline image by ID. Returns true if it existed.
    pub fn remove_image(&mut self, id: ImageId) -> bool {
        let removed = self.image_manager.remove_image(id);
        if removed {
            self.rebuild_image_instances();
        }
        removed
    }

    /// Get a reference to the image manager.
    pub fn image_manager(&self) -> &ImageManager {
        &self.image_manager
    }

    /// Get a mutable reference to the image manager.
    pub fn image_manager_mut(&mut self) -> &mut ImageManager {
        &mut self.image_manager
    }

    /// Rebuild image instance buffer from current image manager state.
    fn rebuild_image_instances(&mut self) {
        let (cw, ch) = self.cell_size();
        let instances = self.image_manager.build_instances(
            cw, ch, (self.grid_offset[0], self.grid_offset[1]),
        );

        // Grow buffer if needed
        let required = (instances.len().max(1) * std::mem::size_of::<ImageInstance>()) as u64;
        if required > self.image_instance_buffer.size() {
            let new_size = required * 2;
            self.image_instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("image_instance_buffer"),
                size: new_size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.gpu_memory.track("image_instance_buffer", new_size);
        }

        if !instances.is_empty() {
            self.queue.write_buffer(
                &self.image_instance_buffer,
                0,
                bytemuck::cast_slice(&instances),
            );
        }
        self.image_instance_count = instances.len() as u32;
    }

    /// Set the font family, size, and line height. Rebuilds the glyph atlas.
    pub fn set_font(&mut self, family: &str, size: f32, line_height: f32) {
        if !family.is_empty() {
            self.config.font_family = family.to_string();
        }
        self.config.font_size = size;
        self.config.line_height = line_height;
        let scaled_size = size * self.scale_factor;
        self.atlas = GlyphAtlas::new(&self.config.font_family, scaled_size, line_height);
        self.atlas.prewarm_ascii();
        self.upload_atlas();
        self.atlas.clear_dirty();
    }

    /// Encode and submit a render pass using already-uploaded instance data.
    ///
    /// This is the FFI counterpart to `render_frame` — call after `upload_instances`
    /// to encode the command buffer and present. Skips instance building.
    pub fn submit_frame(&mut self) -> Result<(), crate::RendererError> {
        if self.atlas.is_dirty() {
            self.upload_atlas();
            self.atlas.clear_dirty();
        }
        self.encode_and_present()
    }

    /// Encode the render pass and present. Requires surface + pipelines.
    fn encode_and_present(&mut self) -> Result<(), crate::RendererError> {
        let surface = self.surface.as_ref().ok_or(wgpu::SurfaceError::Lost)?;
        let rp = self.render.as_ref().ok_or(wgpu::SurfaceError::Lost)?;

        let output = surface.get_current_texture()?;

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render_encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.config.theme.background[0] as f64,
                            g: self.config.theme.background[1] as f64,
                            b: self.config.theme.background[2] as f64,
                            a: (self.config.theme.background[3] * self.config.opacity) as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw from the current buffer index (write and draw same index per frame,
            // toggle at end so GPU processes this frame while next writes to other buffer)
            let draw_idx = self.current_buffer_index;

            if self.bg_instance_count > 0 {
                pass.set_pipeline(&rp.bg_pipeline);
                pass.set_bind_group(0, &rp.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.bg_instance_buffers[draw_idx].slice(..));
                pass.draw(0..6, 0..self.bg_instance_count);
            }

            // Draw standard text glyphs (R8 atlas, page 0)
            if self.text_instance_count > 0 {
                pass.set_pipeline(&rp.text_pipeline);
                pass.set_bind_group(0, &rp.text_bind_group, &[]);
                pass.set_vertex_buffer(0, self.text_instance_buffers[draw_idx].slice(..));
                pass.draw(0..6, 0..self.text_instance_count);
            }

            // Draw overflow atlas page glyphs (R8, pages 1+)
            for &(overflow_idx, count, offset) in &self.overflow_page_draws {
                if count > 0 && overflow_idx < self.overflow_atlas_pages.len() {
                    let (_, ref bind_group) = self.overflow_atlas_pages[overflow_idx];
                    pass.set_pipeline(&rp.text_pipeline);
                    pass.set_bind_group(0, bind_group, &[]);
                    pass.set_vertex_buffer(0, self.overflow_text_instance_buffers[draw_idx].slice(..));
                    pass.draw(0..6, offset..offset + count);
                }
            }

            // Draw subpixel text glyphs (RGBA atlas) — separate draw call with its own texture
            if self.subpixel_text_instance_count > 0 {
                if let (Some(ref sp_pipeline), Some(ref sp_bind_group)) =
                    (&rp.subpixel_text_pipeline, &rp.subpixel_text_bind_group)
                {
                    pass.set_pipeline(sp_pipeline);
                    pass.set_bind_group(0, sp_bind_group, &[]);
                    pass.set_vertex_buffer(0, self.subpixel_text_instance_buffers[draw_idx].slice(..));
                    pass.draw(0..6, 0..self.subpixel_text_instance_count);
                }
            }

            // Draw inline images (each image has its own texture bind group)
            if !self.image_manager.is_empty() {
                pass.set_pipeline(&rp.image_pipeline);
                pass.set_bind_group(0, &rp.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.image_instance_buffer.slice(..));
                let mut instance_idx = 0u32;
                for (_id, entry) in self.image_manager.entries() {
                    pass.set_bind_group(1, &entry.bind_group, &[]);
                    pass.draw(0..6, instance_idx..instance_idx + 1);
                    instance_idx += 1;
                }
            }

            if self.selection_instance_count > 0 {
                pass.set_pipeline(&rp.selection_pipeline);
                pass.set_bind_group(0, &rp.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.selection_instance_buffers[draw_idx].slice(..));
                pass.draw(0..6, 0..self.selection_instance_count);
            }

            // Pane borders (reuse selection pipeline — same vertex layout)
            if self.pane_border_instance_count > 0 {
                pass.set_pipeline(&rp.selection_pipeline);
                pass.set_bind_group(0, &rp.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.pane_border_instance_buffer.slice(..));
                pass.draw(0..6, 0..self.pane_border_instance_count);
            }

            // Compute overlay instances (search highlights, URL underlines, etc.)
            if self.compute_overlay_count > 0 {
                pass.set_pipeline(&rp.compute_overlay_pipeline);
                pass.set_bind_group(0, &rp.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.compute_overlay_buffers[draw_idx].slice(..));
                pass.draw(0..6, 0..self.compute_overlay_count);
            }

            // Custom overlay pipelines (extension-provided shaders)
            for (layer_id, pipeline) in &self.overlay_pipelines {
                if let Some(overlay) = self.overlays.values().find(|o| o.layer_id == *layer_id && o.visible) {
                    let _ = overlay; // overlay config available for future bind group data
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &rp.uniform_bind_group, &[]);
                    pass.draw(0..6, 0..1);
                }
            }

            if self.cursor_visible {
                pass.set_pipeline(&rp.cursor_pipeline);
                pass.set_bind_group(0, &rp.cursor_bind_group, &[]);
                pass.draw(0..6, 0..1);
                tracing::trace!("CURSOR DRAW CALL EXECUTED");
            } else {
                tracing::trace!("CURSOR DRAW SKIPPED (not visible)");
            }

            // Profiler overlay (drawn last, on top of everything)
            if self.profiler_enabled && self.profiler_instance_count > 0 {
                pass.set_pipeline(&rp.text_pipeline);
                pass.set_bind_group(0, &rp.text_bind_group, &[]);
                pass.set_vertex_buffer(0, self.profiler_text_buffer.slice(..));
                pass.draw(0..6, 0..self.profiler_instance_count);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        self.mark_frame_rendered();

        // Toggle double-buffer index for next frame
        self.current_buffer_index = 1 - self.current_buffer_index;

        Ok(())
    }

    /// Apply compute results (search, URL, highlight) as overlay instances.
    ///
    /// Converts compute results into `ComputeOverlayInstance` data and uploads
    /// to the compute overlay GPU buffer. The overlays are drawn in the next frame.
    pub fn apply_compute_results(
        &mut self,
        search_results: &[marauder_compute::SearchResult],
        url_matches: &[marauder_compute::UrlMatch],
        highlight_results: &[marauder_compute::HighlightResult],
    ) {
        let (cw, ch) = self.cell_size();
        let mut instances = Vec::with_capacity(
            search_results.len() + url_matches.len() + highlight_results.len(),
        );

        let search_color = [1.0f32, 0.8, 0.0, 0.35]; // yellow highlight
        let url_color = self.config.theme.url;
        let highlight_colors: [[f32; 4]; 5] = [
            [0.0, 0.0, 0.0, 0.0],           // None
            [0.6, 0.8, 1.0, 0.2],            // Number - light blue
            [0.5, 1.0, 0.5, 0.2],            // FilePath - light green
            [1.0, 0.6, 0.3, 0.2],            // Flag - orange
            [0.9, 0.5, 0.9, 0.2],            // Operator - purple
        ];

        // Search matches → filled highlights
        for s in search_results {
            for i in 0..s.length {
                instances.push(ComputeOverlayInstance {
                    pos: [(s.col + i) as f32 * cw + self.grid_offset[0],
                          s.row as f32 * ch - self.scroll_y_offset + self.grid_offset[1]],
                    size: [cw, ch],
                    color: search_color,
                    flags: 0, // fill
                    _pad: [0.0; 3],
                });
            }
        }

        // URL matches → underlines
        for u in url_matches {
            for col in u.start_col..=u.end_col {
                instances.push(ComputeOverlayInstance {
                    pos: [col as f32 * cw + self.grid_offset[0],
                          u.row as f32 * ch - self.scroll_y_offset + self.grid_offset[1]],
                    size: [cw, ch],
                    color: url_color,
                    flags: 1, // underline
                    _pad: [0.0; 3],
                });
            }
        }

        // Highlight results → filled highlights with category colors
        for h in highlight_results {
            let cat_idx = h.category.to_index();
            let color = if cat_idx < highlight_colors.len() {
                highlight_colors[cat_idx]
            } else {
                continue;
            };
            if color[3] == 0.0 {
                continue;
            }
            instances.push(ComputeOverlayInstance {
                pos: [h.col as f32 * cw + self.grid_offset[0],
                      h.row as f32 * ch - self.scroll_y_offset + self.grid_offset[1]],
                size: [cw, ch],
                color,
                flags: 0, // fill
                _pad: [0.0; 3],
            });
        }

        // Upload to write buffer
        let write_idx = self.current_buffer_index;
        if !instances.is_empty() {
            // Grow buffer if needed
            let required = (instances.len() * std::mem::size_of::<ComputeOverlayInstance>()) as u64;
            if required > self.compute_overlay_buffers[0].size() {
                let new_size = required * 2;
                for i in 0..2 {
                    self.compute_overlay_buffers[i] = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("compute_overlay_buffer"),
                        size: new_size,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }
                self.gpu_memory.track("compute_overlay_buffers", new_size * 2);
            }
            self.queue.write_buffer(
                &self.compute_overlay_buffers[write_idx],
                0,
                bytemuck::cast_slice(&instances),
            );
        }
        self.compute_overlay_count = instances.len() as u32;
        tracing::debug!(
            search = search_results.len(),
            urls = url_matches.len(),
            highlights = highlight_results.len(),
            total_instances = instances.len(),
            "Compute overlay instances updated"
        );
    }

    /// Check if the renderer has a presentable surface.
    pub fn has_surface(&self) -> bool {
        self.surface.is_some()
    }

    /// Register an overlay layer. Replaces any existing overlay with the same ID.
    /// If `config.data["shader_wgsl"]` contains a string, compiles a custom render pipeline.
    pub fn add_overlay(&mut self, config: OverlayConfig) {
        let layer_id = config.layer_id;
        // Compile custom shader pipeline only from trusted sources
        if let Some(wgsl) = config.data.get("shader_wgsl").and_then(|v| v.as_str()) {
            if !config.trusted {
                tracing::warn!(layer_id, "Ignoring shader_wgsl from untrusted overlay source");
            } else if let Err(e) = self.add_overlay_pipeline(layer_id, wgsl) {
                tracing::warn!(layer_id, error = ?e, "Failed to compile overlay shader, registering without pipeline");
            }
        }
        tracing::debug!(layer_id, "overlay added");
        self.overlays.insert(layer_id, config);
    }

    /// Remove an overlay layer by ID. Returns true if it existed.
    pub fn remove_overlay(&mut self, layer_id: u32) -> bool {
        let removed = self.overlays.remove(&layer_id).is_some();
        if removed {
            self.overlay_pipelines.remove(&layer_id);
            tracing::debug!(layer_id, "overlay removed");
        }
        removed
    }

    /// Get a reference to the registered overlays.
    pub fn overlays(&self) -> &HashMap<u32, OverlayConfig> {
        &self.overlays
    }

    /// Set pane border quads for split pane dividers.
    /// Borders are rendered as colored quads (reusing the selection pipeline).
    pub fn set_pane_borders(&mut self, borders: Vec<PaneBorder>) {
        let instances: Vec<SelectionInstance> = borders
            .iter()
            .map(|b| SelectionInstance {
                pos: [b.x, b.y],
                size: [b.width, b.height],
                color: b.color,
            })
            .collect();

        // Grow buffer if needed
        let required_size = (instances.len().max(1) * std::mem::size_of::<SelectionInstance>()) as u64;
        if required_size > self.pane_border_instance_buffer.size() {
            self.pane_border_instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pane_border_instance_buffer"),
                size: required_size * 2,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        if !instances.is_empty() {
            self.queue.write_buffer(
                &self.pane_border_instance_buffer,
                0,
                bytemuck::cast_slice(&instances),
            );
        }
        self.pane_border_instance_count = instances.len() as u32;
        self.pane_borders = borders;
        tracing::debug!(count = self.pane_border_instance_count, "pane borders updated");
    }

    // -----------------------------------------------------------------------
    // Smooth scrolling
    // -----------------------------------------------------------------------

    /// Set the subpixel vertical scroll offset (in pixels).
    /// Used for smooth scrolling — the fractional part between grid lines.
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.scroll_y_offset = if offset.is_finite() {
            offset.clamp(-4096.0, 4096.0)
        } else {
            0.0
        };
    }

    /// Get the current subpixel scroll offset.
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_y_offset
    }

    /// Set the grid offset in physical pixels (e.g. to push below tab bar).
    pub fn set_grid_offset(&mut self, x: f32, y: f32) {
        self.grid_offset = [x, y];
        tracing::info!(x, y, "Grid offset set");
    }

    // -----------------------------------------------------------------------
    // Adaptive frame rate
    // -----------------------------------------------------------------------

    /// Mark recent activity (PTY output, key input, mouse event).
    /// Resets the idle timer so the renderer uses `active_fps`.
    pub fn mark_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check whether enough time has elapsed since the last frame to render again.
    /// Returns `true` if a frame should be rendered based on the current FPS target.
    pub fn should_render(&self) -> bool {
        let idle_threshold = std::time::Duration::from_secs(self.config.idle_threshold_secs);
        let fps = if self.last_activity.elapsed() < idle_threshold {
            self.config.active_fps
        } else {
            self.config.idle_fps
        };
        let frame_interval = std::time::Duration::from_secs_f64(1.0 / fps.max(1) as f64);
        self.last_frame.elapsed() >= frame_interval
    }

    /// Record that a frame was just rendered (call after present).
    pub fn mark_frame_rendered(&mut self) {
        self.last_frame = Instant::now();
    }

    // -----------------------------------------------------------------------
    // Opacity
    // -----------------------------------------------------------------------

    /// Set background opacity (0.0 = fully transparent, 1.0 = fully opaque).
    pub fn set_opacity(&mut self, opacity: f32) {
        self.config.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Get the current background opacity.
    pub fn opacity(&self) -> f32 {
        self.config.opacity
    }

    // -----------------------------------------------------------------------
    // Overlay shader pipelines
    // -----------------------------------------------------------------------

    /// Compile and register a custom overlay render pipeline from WGSL source.
    /// Associates it with the given `layer_id`. Requires a surface (pipelines need a format).
    pub fn add_overlay_pipeline(&mut self, layer_id: u32, wgsl_source: &str) -> Result<(), crate::RendererError> {
        let rp = self.render.as_ref().ok_or(crate::RendererError::PipelineCreation("No render pipelines (headless mode)".to_string()))?;
        let pipeline = pipelines::create_overlay_pipeline(
            &self.device,
            self.surface_config.format,
            wgsl_source,
            &rp.text_bind_group_layout,
        ).map_err(|e| crate::RendererError::ShaderCompilation(e))?;
        self.overlay_pipelines.insert(layer_id, pipeline);
        tracing::debug!(layer_id, "Custom overlay pipeline compiled");
        Ok(())
    }

    /// Remove a custom overlay pipeline by layer ID.
    pub fn remove_overlay_pipeline(&mut self, layer_id: u32) -> bool {
        self.overlay_pipelines.remove(&layer_id).is_some()
    }

    /// Create a headless renderer (no window surface) for FFI / config queries.
    pub fn new_headless(
        width: u32,
        height: u32,
        scale_factor: f32,
        config: RendererConfig,
    ) -> Result<Self, crate::RendererError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = block_on_safe(
            instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            }),
        )
        .ok_or(crate::RendererError::NoAdapter)?;

        tracing::info!(
            adapter = adapter.get_info().name,
            backend = ?adapter.get_info().backend,
            "GPU adapter selected (headless)"
        );

        let (device, queue) = block_on_safe(
            adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("marauder_device_headless"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
            }, None),
        ).map_err(|e| crate::RendererError::DeviceRequest(e.to_string()))?;
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        // Use a default sRGB format for headless
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            desired_maximum_frame_latency: 2,
            view_formats: vec![],
        };

        Self::build(device, queue, None, surface_config, scale_factor, format, config)
    }

    /// Shared construction: creates all GPU resources (buffers, pipelines, atlas)
    /// from an already-obtained device, queue, surface, and format.
    fn build(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface: Option<wgpu::Surface<'static>>,
        surface_config: wgpu::SurfaceConfiguration,
        scale_factor: f32,
        format: wgpu::TextureFormat,
        config: RendererConfig,
    ) -> Result<Self, crate::RendererError> {
        // --- Uniform buffer (needed for upload_instances in both headless and windowed) ---
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform_buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Atlas texture ---
        // Scale font size by DPI factor so glyphs match the physical pixel surface.
        // Without this, text is tiny on Retina/HiDPI displays.
        let scaled_font_size = config.font_size * scale_factor;
        tracing::info!(
            raw_font_size = config.font_size,
            scale_factor,
            scaled_font_size,
            "Font size scaled for DPI"
        );
        let mut atlas = GlyphAtlas::new(&config.font_family, scaled_font_size, config.line_height);
        atlas.prewarm_ascii();

        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: atlas.atlas_size(),
                height: atlas.atlas_size(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            atlas.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas.atlas_size()),
                rows_per_image: Some(atlas.atlas_size()),
            },
            wgpu::Extent3d {
                width: atlas.atlas_size(),
                height: atlas.atlas_size(),
                depth_or_array_layers: 1,
            },
        );
        atlas.clear_dirty();

        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // --- Cursor uniform buffer ---
        let cursor_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cursor_uniform_buffer"),
            size: std::mem::size_of::<CursorUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Double-buffered instance buffers ---
        let max_cells = INITIAL_MAX_CELLS;
        let bg_size = (max_cells * std::mem::size_of::<BgInstance>()) as u64;
        let text_size = (max_cells * std::mem::size_of::<TextInstance>()) as u64;
        let sel_size = (max_cells * std::mem::size_of::<SelectionInstance>()) as u64;

        let create_buf = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };

        let bg_instance_buffers = [create_buf("bg_instance_buffer_0", bg_size), create_buf("bg_instance_buffer_1", bg_size)];
        let text_instance_buffers = [create_buf("text_instance_buffer_0", text_size), create_buf("text_instance_buffer_1", text_size)];
        let subpixel_text_instance_buffers = [create_buf("subpixel_text_instance_buffer_0", text_size), create_buf("subpixel_text_instance_buffer_1", text_size)];
        let selection_instance_buffers = [create_buf("selection_instance_buffer_0", sel_size), create_buf("selection_instance_buffer_1", sel_size)];

        // --- Render pipelines + bind groups (only when a surface is present) ---
        let render = if surface.is_some() {
            let uniform_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("uniform_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

            let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("uniform_bind_group"),
                layout: &uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                }],
            });

            let text_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("text_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

            let text_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("text_bind_group"),
                layout: &text_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&atlas_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                    },
                ],
            });

            let cursor_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("cursor_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

            let cursor_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cursor_bind_group"),
                layout: &cursor_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cursor_uniform_buffer.as_entire_binding(),
                }],
            });

            let bg_pipeline = pipelines::create_background_pipeline(&device, format, &uniform_bind_group_layout);
            let text_pipeline = pipelines::create_text_pipeline(&device, format, &text_bind_group_layout);
            let selection_pipeline = pipelines::create_selection_pipeline(&device, format, &uniform_bind_group_layout);
            let cursor_pipeline = pipelines::create_cursor_pipeline(&device, format, &cursor_bind_group_layout);

            // Subpixel text pipeline (created eagerly, used only when subpixel_aa is enabled)
            let subpixel_text_pipeline = Some(pipelines::create_subpixel_text_pipeline(&device, format, &text_bind_group_layout));

            // Compute overlay pipeline (search highlights, URL underlines, outlines)
            let compute_overlay_pipeline = pipelines::create_compute_overlay_pipeline(&device, format, &uniform_bind_group_layout);

            // Image pipeline (inline images: Sixel, iTerm2)
            // Uses a temporary ImageManager just to get the bind group layout for pipeline creation.
            // The actual ImageManager is created below and stored on Renderer.
            let tmp_image_manager = ImageManager::new(&device);
            let image_pipeline = pipelines::create_image_pipeline(&device, format, &uniform_bind_group_layout, tmp_image_manager.bind_group_layout());
            drop(tmp_image_manager);

            Some(RenderPipelines {
                bg_pipeline,
                text_pipeline,
                selection_pipeline,
                cursor_pipeline,
                uniform_bind_group,
                text_bind_group,
                text_bind_group_layout,
                cursor_bind_group,
                subpixel_text_pipeline,
                subpixel_text_bind_group: None, // created lazily when subpixel_aa enabled
                compute_overlay_pipeline,
                image_pipeline,
            })
        } else {
            None
        };

        let pane_border_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pane_border_instance_buffer"),
            size: (64 * std::mem::size_of::<SelectionInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Compute overlay buffers (double-buffered)
        let overlay_size = (INITIAL_MAX_CELLS * std::mem::size_of::<ComputeOverlayInstance>()) as u64;
        let compute_overlay_buffers = [
            create_buf("compute_overlay_buffer_0", overlay_size),
            create_buf("compute_overlay_buffer_1", overlay_size),
        ];

        // Image manager and instance buffer
        let image_manager = ImageManager::new(&device);
        let image_buf_size = (64 * std::mem::size_of::<ImageInstance>()) as u64;
        let image_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image_instance_buffer"),
            size: image_buf_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Subpixel RGBA atlas texture (created when subpixel_aa is enabled in config)
        let subpixel_atlas_texture = if config.subpixel_aa && surface.is_some() {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("subpixel_glyph_atlas"),
                size: wgpu::Extent3d {
                    width: atlas.atlas_size(),
                    height: atlas.atlas_size(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            // Upload initial subpixel atlas data
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                atlas.subpixel_pixels(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(atlas.atlas_size() * 4),
                    rows_per_image: Some(atlas.atlas_size()),
                },
                wgpu::Extent3d {
                    width: atlas.atlas_size(),
                    height: atlas.atlas_size(),
                    depth_or_array_layers: 1,
                },
            );
            Some(tex)
        } else {
            None
        };

        // Profiler overlay buffer (up to 128 text instances for stats text)
        let profiler_text_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("profiler_text_buffer"),
            size: (128 * std::mem::size_of::<TextInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // GPU memory tracking
        let mut gpu_memory = GpuMemoryTracker::new();
        gpu_memory.track("bg_instance_buffers", bg_size * 2);
        gpu_memory.track("text_instance_buffers", text_size * 2);
        gpu_memory.track("subpixel_text_instance_buffers", text_size * 2);
        gpu_memory.track("selection_instance_buffers", sel_size * 2);
        gpu_memory.track("atlas_texture", (atlas.atlas_size() * atlas.atlas_size()) as u64);
        gpu_memory.track("uniform_buffer", std::mem::size_of::<Uniforms>() as u64);
        gpu_memory.track("cursor_uniform_buffer", std::mem::size_of::<CursorUniforms>() as u64);
        gpu_memory.track("profiler_text_buffer", (128 * std::mem::size_of::<TextInstance>()) as u64);
        gpu_memory.track("compute_overlay_buffers", overlay_size * 2);
        gpu_memory.track("image_instance_buffer", image_buf_size);

        // Overflow text instance buffers (created before Self to avoid borrow-after-move)
        let overflow_text_instance_buffers = [
            create_buf("overflow_text_instance_buf_0", (max_cells * std::mem::size_of::<TextInstance>()) as u64),
            create_buf("overflow_text_instance_buf_1", (max_cells * std::mem::size_of::<TextInstance>()) as u64),
        ];

        let now = Instant::now();
        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            scale_factor,
            render,
            uniform_buffer,
            cursor_uniform_buffer,
            bg_instance_buffers,
            text_instance_buffers,
            subpixel_text_instance_buffers,
            selection_instance_buffers,
            current_buffer_index: 0,
            subpixel_atlas_texture,
            atlas_texture,
            overflow_atlas_pages: Vec::new(),
            overflow_text_instance_buffers,
            overflow_page_draws: Vec::new(),
            atlas,
            config,
            bg_instance_count: 0,
            text_instance_count: 0,
            subpixel_text_instance_count: 0,
            selection_instance_count: 0,
            start_time: now,
            max_cells,
            cursor_visible: true,
            overlays: HashMap::new(),
            pane_borders: Vec::new(),
            pane_border_instance_buffer,
            pane_border_instance_count: 0,
            cached_bg_instances: Vec::new(),
            cached_cols: 0,
            cached_rows: 0,
            scroll_y_offset: 0.0,
            grid_offset: [0.0, 0.0],
            last_activity: now,
            last_frame: now,
            overlay_pipelines: HashMap::new(),
            compute_overlay_buffers,
            compute_overlay_count: 0,
            image_manager,
            image_instance_buffer,
            image_instance_count: 0,
            gpu_memory,
            profiler_enabled: false,
            profiler_text_buffer,
            profiler_instance_count: 0,
            frame_stats: FrameStats::default(),
        })
    }
}
