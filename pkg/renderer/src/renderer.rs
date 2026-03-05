//! Main renderer: coordinates wgpu surface, pipelines, atlas, and frame production.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use marauder_grid::Grid;
use tracing;

use crate::atlas::GlyphAtlas;
use crate::pipelines;
use crate::types::*;

/// The GPU-accelerated terminal renderer.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    scale_factor: f32,

    // Pipelines
    bg_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    cursor_pipeline: wgpu::RenderPipeline,

    // Bind groups and buffers
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    cursor_uniform_buffer: wgpu::Buffer,
    cursor_bind_group: wgpu::BindGroup,
    bg_instance_buffer: wgpu::Buffer,
    text_instance_buffer: wgpu::Buffer,
    atlas_texture: wgpu::Texture,
    text_bind_group: wgpu::BindGroup,
    text_bind_group_layout: wgpu::BindGroupLayout,

    // State
    atlas: GlyphAtlas,
    config: RendererConfig,
    bg_instance_count: u32,
    text_instance_count: u32,
    start_time: Instant,
    max_cells: usize,
}

/// Maximum cells we preallocate buffers for (resized on demand).
const INITIAL_MAX_CELLS: usize = 250 * 80;

impl Renderer {
    /// Create a new renderer on the given window surface.
    pub async fn new<W: raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle + Send + Sync + 'static>(
        window: Arc<W>,
        width: u32,
        height: u32,
        scale_factor: f32,
        config: RendererConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // SAFETY: The window handle is valid for the lifetime of the surface,
        // which is owned by the Renderer and lives as long as the window.
        let surface = instance.create_surface(window)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("No suitable GPU adapter found")?;

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
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            desired_maximum_frame_latency: 2,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        // --- Uniform buffer + bind group ---
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform_buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

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

        // --- Atlas texture ---
        let mut atlas = GlyphAtlas::new(config.font_size, config.line_height);
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

        // Upload initial atlas
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

        // --- Text bind group (uniforms + atlas texture + sampler) ---
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

        // --- Cursor uniform buffer + bind group ---
        let cursor_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cursor_uniform_buffer"),
            size: std::mem::size_of::<CursorUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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

        // --- Instance buffers ---
        let max_cells = INITIAL_MAX_CELLS;
        let bg_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg_instance_buffer"),
            size: (max_cells * std::mem::size_of::<BgInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let text_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text_instance_buffer"),
            size: (max_cells * std::mem::size_of::<TextInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Render pipelines ---
        let bg_pipeline = pipelines::create_background_pipeline(&device, format, &uniform_bind_group_layout);
        let text_pipeline = pipelines::create_text_pipeline(&device, format, &text_bind_group_layout);
        let cursor_pipeline = pipelines::create_cursor_pipeline(&device, format, &cursor_bind_group_layout);

        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            scale_factor,
            bg_pipeline,
            text_pipeline,
            cursor_pipeline,
            uniform_buffer,
            uniform_bind_group,
            cursor_uniform_buffer,
            cursor_bind_group,
            bg_instance_buffer,
            text_instance_buffer,
            atlas_texture,
            text_bind_group,
            text_bind_group_layout,
            atlas,
            config,
            bg_instance_count: 0,
            text_instance_count: 0,
            start_time: Instant::now(),
            max_cells,
        })
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
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Calculate grid dimensions for the given surface size.
    pub fn grid_dimensions(&self) -> (u16, u16) {
        let (cw, ch) = self.cell_size();
        let cols = (self.surface_config.width as f32 / cw).floor() as u16;
        let rows = (self.surface_config.height as f32 / ch).floor() as u16;
        (rows.max(1), cols.max(1))
    }

    /// Update instance buffers from the grid state, then render a frame.
    pub fn render_frame(&mut self, grid: &Arc<Mutex<Grid>>) -> Result<(), wgpu::SurfaceError> {
        // Lock grid, build instance data, release lock before GPU work
        let (bg_instances, text_instances, cursor_row, cursor_col) = {
            let mut grid = grid.lock().unwrap_or_else(|e| e.into_inner());
            let result = self.build_instances(&grid);
            grid.clear_dirty();
            result
        };

        self.upload_instances(&bg_instances, &text_instances, cursor_row, cursor_col);

        // Re-upload atlas if new glyphs were rasterized
        if self.atlas.is_dirty() {
            self.upload_atlas();
            self.atlas.clear_dirty();
        }

        // Render
        let output = self.surface.get_current_texture()?;
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
                            a: self.config.theme.background[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Background pass
            if self.bg_instance_count > 0 {
                pass.set_pipeline(&self.bg_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.bg_instance_buffer.slice(..));
                pass.draw(0..6, 0..self.bg_instance_count);
            }

            // Text pass
            if self.text_instance_count > 0 {
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(0, &self.text_bind_group, &[]);
                pass.set_vertex_buffer(0, self.text_instance_buffer.slice(..));
                pass.draw(0..6, 0..self.text_instance_count);
            }

            // Cursor pass
            pass.set_pipeline(&self.cursor_pipeline);
            pass.set_bind_group(0, &self.cursor_bind_group, &[]);
            pass.draw(0..6, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Build background and text instance data from the grid.
    fn build_instances(&mut self, grid: &Grid) -> (Vec<BgInstance>, Vec<TextInstance>, usize, usize) {
        let rows = grid.rows();
        let cols = grid.cols();
        let (cw, ch) = self.cell_size();
        let ascent = self.atlas.ascent();
        let screen = grid.active_screen();

        let default_bg = self.config.theme.background;
        let default_fg = self.config.theme.foreground;

        let mut bg_instances = Vec::with_capacity(rows * cols);
        let mut text_instances = Vec::with_capacity(rows * cols);

        for row in 0..rows {
            if row >= screen.rows.len() {
                break;
            }
            for col in 0..cols {
                if col >= screen.rows[row].len() {
                    break;
                }
                let cell = &screen.rows[row][col];

                let px = col as f32 * cw;
                let py = row as f32 * ch;

                // Background
                let bg_color = cell.bg.to_rgba_f32_or(default_bg);
                bg_instances.push(BgInstance {
                    pos: [px, py],
                    size: [cw, ch],
                    bg_color,
                });

                // Text (skip spaces and control chars)
                if cell.c != ' ' && !cell.c.is_control() {
                    if let Some(glyph) = self.atlas.get_or_insert(cell.c) {
                        let fg_color = cell.fg.to_rgba_f32_or(default_fg);
                        text_instances.push(TextInstance {
                            pos: [px, py],
                            size: glyph.pixel_size,
                            fg_color,
                            uv_rect: glyph.uv,
                            glyph_offset: [glyph.offset[0], glyph.offset[1] + ascent],
                        });
                    }
                }
            }
        }

        (bg_instances, text_instances, grid.cursor.row, grid.cursor.col)
    }

    /// Upload instance data and uniforms to GPU buffers.
    fn upload_instances(
        &mut self,
        bg_instances: &[BgInstance],
        text_instances: &[TextInstance],
        cursor_row: usize,
        cursor_col: usize,
    ) {
        let total_cells = bg_instances.len();

        // Grow buffers if needed
        if total_cells > self.max_cells {
            self.max_cells = total_cells * 2;
            self.bg_instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bg_instance_buffer"),
                size: (self.max_cells * std::mem::size_of::<BgInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.text_instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("text_instance_buffer"),
                size: (self.max_cells * std::mem::size_of::<TextInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        // Upload instance data
        if !bg_instances.is_empty() {
            self.queue.write_buffer(
                &self.bg_instance_buffer,
                0,
                bytemuck::cast_slice(bg_instances),
            );
        }
        self.bg_instance_count = bg_instances.len() as u32;

        if !text_instances.is_empty() {
            self.queue.write_buffer(
                &self.text_instance_buffer,
                0,
                bytemuck::cast_slice(text_instances),
            );
        }
        self.text_instance_count = text_instances.len() as u32;

        // Upload uniforms
        let (cw, ch) = self.cell_size();
        let uniforms = Uniforms {
            viewport_size: [
                self.surface_config.width as f32,
                self.surface_config.height as f32,
            ],
            cell_size: [cw, ch],
            grid_offset: [0.0, 0.0],
            _pad: [0.0, 0.0],
        };
        self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Upload cursor uniforms
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
                cursor_col as f32 * cw,
                cursor_row as f32 * ch + cursor_y_offset,
            ],
            cursor_size: [cursor_w, cursor_h],
            cursor_color: self.config.theme.cursor,
            time: elapsed,
            blink_rate: if self.config.cursor_blink { 1.0 } else { 0.0 },
            _pad: [0.0, 0.0],
        };
        self.queue.write_buffer(
            &self.cursor_uniform_buffer,
            0,
            bytemuck::bytes_of(&cursor_uniforms),
        );
    }

    /// Re-upload the glyph atlas texture to GPU.
    fn upload_atlas(&self) {
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
        // Rebuild text bind group with new atlas view
        // (The texture object is the same, so the existing bind group still works)
    }

    /// Get a reference to the wgpu device (for sharing with compute).
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Get a reference to the wgpu queue (for sharing with compute).
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}
