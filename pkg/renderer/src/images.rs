//! Image manager for inline images (Sixel, iTerm2).
//!
//! Manages GPU textures for decoded images and provides instance data
//! for the image render pass.

use std::collections::HashMap;

/// Unique ID for each image placed in the terminal grid.
pub type ImageId = u32;

/// A placed image in the terminal grid.
pub struct ImageEntry {
    /// wgpu texture holding the RGBA pixel data.
    pub texture: wgpu::Texture,
    /// Texture view for binding.
    pub texture_view: wgpu::TextureView,
    /// Bind group for the image pipeline.
    pub bind_group: wgpu::BindGroup,
    /// Grid position (row, col) of the top-left cell.
    pub grid_row: u32,
    pub grid_col: u32,
    /// Size in grid cells (width_cells, height_cells).
    pub width_cells: u32,
    pub height_cells: u32,
    /// Original pixel dimensions.
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// Per-image instance data for the GPU (one quad per image).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ImageInstance {
    /// Pixel position (x, y) of the top-left corner.
    pub pos: [f32; 2],
    /// Pixel size (width, height).
    pub size: [f32; 2],
    /// UV rect (u, v, w, h) — always (0,0,1,1) for full texture.
    pub uv_rect: [f32; 4],
}

/// Manages all inline images for a terminal pane.
pub struct ImageManager {
    images: HashMap<ImageId, ImageEntry>,
    next_id: ImageId,
    /// Bind group layout shared by all image entries.
    bind_group_layout: wgpu::BindGroupLayout,
    /// Sampler for image textures.
    sampler: wgpu::Sampler,
}

impl ImageManager {
    /// Create a new image manager.
    pub fn new(device: &wgpu::Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            images: HashMap::new(),
            next_id: 1,
            bind_group_layout,
            sampler,
        }
    }

    /// Get the bind group layout (needed for pipeline creation).
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    /// Add an image from RGBA pixel data. Returns the image ID.
    pub fn add_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pixels: &[u8],
        width: u32,
        height: u32,
        grid_row: u32,
        grid_col: u32,
        width_cells: u32,
        height_cells: u32,
    ) -> ImageId {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("inline_image"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let id = self.next_id;
        self.next_id += 1;

        self.images.insert(id, ImageEntry {
            texture,
            texture_view,
            bind_group,
            grid_row,
            grid_col,
            width_cells,
            height_cells,
            pixel_width: width,
            pixel_height: height,
        });

        id
    }

    /// Remove an image by ID.
    pub fn remove_image(&mut self, id: ImageId) -> bool {
        self.images.remove(&id).is_some()
    }

    /// Get all current images for rendering.
    pub fn entries(&self) -> impl Iterator<Item = (&ImageId, &ImageEntry)> {
        self.images.iter()
    }

    /// Number of images.
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Build image instances for the render pass.
    ///
    /// `grid_offset` shifts images to account for UI chrome (e.g., tab bar).
    /// Positions are in logical pixels; the shader applies `scale_factor`.
    pub fn build_instances(
        &self,
        cell_width: f32,
        cell_height: f32,
        grid_offset: (f32, f32),
    ) -> Vec<ImageInstance> {
        self.images.values().map(|entry| {
            ImageInstance {
                pos: [
                    entry.grid_col as f32 * cell_width + grid_offset.0,
                    entry.grid_row as f32 * cell_height + grid_offset.1,
                ],
                size: [
                    entry.width_cells as f32 * cell_width,
                    entry.height_cells as f32 * cell_height,
                ],
                uv_rect: [0.0, 0.0, 1.0, 1.0],
            }
        }).collect()
    }
}
