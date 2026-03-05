use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::types::*;
use marauder_grid::{Cell, Color, Grid};

/// Maximum number of results any single compute dispatch will return.
/// Caps GPU buffer allocation to ~512KB instead of scaling with grid size.
const MAX_RESULT_CAP: u32 = 65536;

/// GPU compute engine for text search, URL detection, highlighting, and selection extraction.
///
/// Shares the wgpu `Device` and `Queue` with the renderer when available,
/// or creates its own for standalone usage.
pub struct ComputeEngine {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    search_pipeline: wgpu::ComputePipeline,
    url_detect_pipeline: wgpu::ComputePipeline,
    highlight_pipeline: wgpu::ComputePipeline,
    selection_pipeline: wgpu::ComputePipeline,
    /// Current cell buffer on GPU.
    cell_buffer: Option<wgpu::Buffer>,
    /// Number of cells currently uploaded.
    cell_count: u32,
    /// Grid dimensions for current upload.
    grid_rows: u32,
    grid_cols: u32,
}

impl ComputeEngine {
    /// Create a ComputeEngine borrowing the renderer's device and queue.
    ///
    /// # Safety
    /// - `device_ptr` must be a valid pointer to an `Arc<wgpu::Device>`.
    /// - `queue_ptr` must be a valid pointer to an `Arc<wgpu::Queue>`.
    /// - Both pointees must outlive this `ComputeEngine`.
    pub unsafe fn new_borrowed(
        device_ptr: *const Arc<wgpu::Device>,
        queue_ptr: *const Arc<wgpu::Queue>,
    ) -> Self {
        // SAFETY: Caller guarantees pointers point to valid Arc instances.
        let device = unsafe { (*device_ptr).clone() };
        let queue = unsafe { (*queue_ptr).clone() };

        Self::from_device_queue(device, queue)
    }

    /// Create a standalone ComputeEngine with its own device and queue.
    pub fn new_standalone() -> Result<Self, ComputeError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or(ComputeError::NoAdapter)?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("marauder_compute_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            },
            None,
        ))
        .map_err(|e| ComputeError::DeviceRequest(e.to_string()))?;

        Ok(Self::from_device_queue(Arc::new(device), Arc::new(queue)))
    }

    /// Shared GpuCell struct definition prepended to every compute shader.
    const GPU_CELL_WGSL: &'static str = include_str!("../../../resources/shaders/gpu_cell.wgsl");

    /// Concatenate the shared GpuCell definition with a shader body.
    fn assemble_shader(body: &str) -> String {
        format!("{}\n{}", Self::GPU_CELL_WGSL, body)
    }

    fn from_device_queue(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        let search_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("search_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::assemble_shader(include_str!("../../../resources/shaders/search.wgsl")).into()),
        });
        let url_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("url_detect_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::assemble_shader(include_str!("../../../resources/shaders/url_detect.wgsl")).into()),
        });
        let highlight_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("highlight_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::assemble_shader(include_str!("../../../resources/shaders/highlight.wgsl")).into()),
        });
        let selection_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("selection_extract_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::assemble_shader(include_str!("../../../resources/shaders/selection_extract.wgsl")).into()),
        });

        let search_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("search_pipeline"),
            layout: None,
            module: &search_shader,
            entry_point: Some("search_row"),
            compilation_options: Default::default(),
            cache: None,
        });
        let url_detect_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("url_detect_pipeline"),
            layout: None,
            module: &url_shader,
            entry_point: Some("detect_urls"),
            compilation_options: Default::default(),
            cache: None,
        });
        let highlight_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("highlight_pipeline"),
            layout: None,
            module: &highlight_shader,
            entry_point: Some("classify_cells"),
            compilation_options: Default::default(),
            cache: None,
        });
        let selection_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("selection_pipeline"),
            layout: None,
            module: &selection_shader,
            entry_point: Some("extract_selection"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            device,
            queue,
            search_pipeline,
            url_detect_pipeline,
            highlight_pipeline,
            selection_pipeline,
            cell_buffer: None,
            cell_count: 0,
            grid_rows: 0,
            grid_cols: 0,
        }
    }

    /// Convert a Grid's cells to GpuCell format and upload to GPU.
    pub fn upload_cells(&mut self, grid: &Grid) {
        let screen = grid.active_screen();
        let rows = screen.rows.len();
        let cols = screen.cols;
        let mut gpu_cells = Vec::with_capacity(rows * cols);

        for (r, row) in screen.rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                gpu_cells.push(cell_to_gpu(cell, r as u32, c as u32));
            }
        }

        self.upload_cells_raw(&gpu_cells, rows as u32, cols as u32);
    }

    /// Upload pre-built GpuCell data directly.
    pub fn upload_cells_raw(&mut self, cells: &[GpuCell], rows: u32, cols: u32) {
        let data = bytemuck::cast_slice(cells);
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("compute_cell_buffer"),
            contents: data,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        self.cell_buffer = Some(buffer);
        self.cell_count = cells.len() as u32;
        self.grid_rows = rows;
        self.grid_cols = cols;
    }

    /// Search for a pattern across all rows. Returns match positions.
    pub fn search(&self, pattern: &str) -> Result<Vec<SearchResult>, ComputeError> {
        let cell_buffer = self.cell_buffer.as_ref().ok_or(ComputeError::NoCellData)?;
        let pattern_codepoints: Vec<u32> = pattern.chars().map(|c| c as u32).collect();
        if pattern_codepoints.is_empty() {
            return Ok(Vec::new());
        }

        let params = SearchParams {
            pattern_len: pattern_codepoints.len() as u32,
            total_rows: self.grid_rows,
            cols: self.grid_cols,
            max_results: (self.grid_rows * self.grid_cols).min(MAX_RESULT_CAP),
        };

        let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("search_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let pattern_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("search_pattern"),
            contents: bytemuck::cast_slice(&pattern_codepoints),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let max_matches = (self.grid_rows * self.grid_cols).min(MAX_RESULT_CAP) as usize;
        let matches_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("search_matches"),
            size: (max_matches * 2 * 4) as u64, // 2 u32 slots per match (row, col)
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let count_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("search_match_count"),
            contents: &[0u8; 4],
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });

        let bind_group_layout = self.search_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("search_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cell_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: params_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: pattern_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: matches_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: count_buffer.as_entire_binding() },
            ],
        });

        let workgroups = (self.grid_rows + 255) / 256;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.search_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        // Phase 1: dispatch compute, read back only the match count (4 bytes)
        let count_staging = self.create_staging_buffer(4);
        encoder.copy_buffer_to_buffer(&count_buffer, 0, &count_staging, 0, 4);
        self.queue.submit(Some(encoder.finish()));

        let count = self.read_u32(&count_staging)?.min(max_matches as u32) as usize;
        if count == 0 {
            return Ok(Vec::new());
        }

        // Phase 2: copy only the bytes actually written, not the full buffer
        let readback_bytes = (count * 2 * 4) as u64;
        let matches_staging = self.create_staging_buffer(readback_bytes);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&matches_buffer, 0, &matches_staging, 0, readback_bytes);
        self.queue.submit(Some(encoder.finish()));

        let match_data = self.read_u32_vec(&matches_staging, count * 2)?;

        let pattern_len = pattern_codepoints.len() as u32;
        let results = (0..count).map(|i| {
            SearchResult {
                row: match_data[i * 2],
                col: match_data[i * 2 + 1],
                length: pattern_len,
            }
        }).collect();

        Ok(results)
    }

    /// Detect URLs in a range of rows. Returns positions; URL text should be
    /// reconstructed from Grid data on the CPU side.
    pub fn detect_urls(&self, row_start: u32, row_end: u32) -> Result<Vec<UrlMatch>, ComputeError> {
        let cell_buffer = self.cell_buffer.as_ref().ok_or(ComputeError::NoCellData)?;

        // Clamp both bounds to grid_rows; early-return on empty/inverted range
        let row_start = row_start.min(self.grid_rows);
        let row_end = row_end.min(self.grid_rows);
        if row_start >= row_end {
            return Ok(Vec::new());
        }

        let max_results = (((row_end - row_start) * self.grid_cols) as usize).min(MAX_RESULT_CAP as usize);
        let params = UrlDetectParams {
            total_rows: self.grid_rows,
            cols: self.grid_cols,
            row_start,
            row_end,
            max_results: max_results as u32,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("url_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let results_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("url_results"),
            size: (max_results * 3 * 4) as u64, // 3 u32 per result (row, start_col, end_col)
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let count_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("url_count"),
            contents: &[0u8; 4],
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });

        let bind_group_layout = self.url_detect_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("url_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cell_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: params_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: results_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: count_buffer.as_entire_binding() },
            ],
        });

        let num_rows = row_end - row_start; // safe: early-return guarantees row_start < row_end
        let workgroups = (num_rows + 255) / 256;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.url_detect_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        // Phase 1: dispatch compute, read back only the result count (4 bytes)
        let count_staging = self.create_staging_buffer(4);
        encoder.copy_buffer_to_buffer(&count_buffer, 0, &count_staging, 0, 4);
        self.queue.submit(Some(encoder.finish()));

        let count = (self.read_u32(&count_staging)? as usize).min(max_results);
        if count == 0 {
            return Ok(Vec::new());
        }

        // Phase 2: copy only the bytes actually written
        let readback_bytes = (count * 3 * 4) as u64;
        let results_staging = self.create_staging_buffer(readback_bytes);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&results_buffer, 0, &results_staging, 0, readback_bytes);
        self.queue.submit(Some(encoder.finish()));

        let raw = self.read_u32_vec(&results_staging, count * 3)?;

        let results = (0..count).map(|i| {
            UrlMatch {
                row: raw[i * 3],
                start_col: raw[i * 3 + 1],
                end_col: raw[i * 3 + 2],
            }
        }).collect();

        Ok(results)
    }

    /// Classify cells into highlight categories (Number, FilePath, Flag, Operator).
    pub fn highlight_cells(&self) -> Result<Vec<HighlightResult>, ComputeError> {
        let cell_buffer = self.cell_buffer.as_ref().ok_or(ComputeError::NoCellData)?;

        let params = HighlightParams {
            total_rows: self.grid_rows,
            cols: self.grid_cols,
            _pad0: 0,
            _pad1: 0,
        };
        let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("highlight_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let categories_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("highlight_categories"),
            size: (self.cell_count as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let bind_group_layout = self.highlight_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("highlight_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cell_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: params_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: categories_buffer.as_entire_binding() },
            ],
        });

        let workgroups = (self.grid_rows + 255) / 256;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.highlight_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        let staging = self.create_staging_buffer((self.cell_count as u64) * 4);
        encoder.copy_buffer_to_buffer(&categories_buffer, 0, &staging, 0, (self.cell_count as u64) * 4);
        self.queue.submit(Some(encoder.finish()));

        let cat_data = self.read_u32_vec(&staging, self.cell_count as usize)?;

        let mut results = Vec::new();
        for (i, &cat) in cat_data.iter().enumerate() {
            if cat != 0 {
                let row = (i as u32) / self.grid_cols;
                let col = (i as u32) % self.grid_cols;
                results.push(HighlightResult {
                    row,
                    col,
                    category: HighlightCategory::from_u32(cat),
                });
            }
        }

        Ok(results)
    }

    /// Extract text from a selection range using the GPU.
    pub fn extract_selection(
        &self,
        start_row: u32,
        start_col: u32,
        end_row: u32,
        end_col: u32,
    ) -> Result<String, ComputeError> {
        let cell_buffer = self.cell_buffer.as_ref().ok_or(ComputeError::NoCellData)?;

        let params = SelectionParams {
            start_row,
            start_col,
            end_row,
            end_col,
            cols: self.grid_cols,
            max_output: (end_row.saturating_sub(start_row) + 1) * self.grid_cols + end_row.saturating_sub(start_row),
            _pad0: 0,
            _pad1: 0,
        };
        let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        // Max output: all cells in range + newlines
        let num_rows = end_row.saturating_sub(start_row) + 1;
        let max_output = (num_rows * self.grid_cols + num_rows) as usize;
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("selection_output"),
            size: (max_output * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let len_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection_output_len"),
            contents: &[0u8; 4],
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });

        let bind_group_layout = self.selection_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("selection_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cell_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: params_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: output_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: len_buffer.as_entire_binding() },
            ],
        });

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.selection_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let sel_workgroups = (num_rows + 255) / 256;
            pass.dispatch_workgroups(sel_workgroups, 1, 1);
        }

        let len_staging = self.create_staging_buffer(4);
        let output_staging = self.create_staging_buffer((max_output * 4) as u64);
        encoder.copy_buffer_to_buffer(&len_buffer, 0, &len_staging, 0, 4);
        encoder.copy_buffer_to_buffer(&output_buffer, 0, &output_staging, 0, (max_output * 4) as u64);
        self.queue.submit(Some(encoder.finish()));

        let len = self.read_u32(&len_staging)? as usize;
        let codepoints = self.read_u32_vec(&output_staging, len.min(max_output))?;

        let text: String = codepoints.iter()
            .filter_map(|&cp| char::from_u32(cp))
            .collect();

        Ok(text)
    }

    // --- Helper methods ---

    fn create_staging_buffer(&self, size: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn read_u32(&self, buffer: &wgpu::Buffer) -> Result<u32, ComputeError> {
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|_| ComputeError::ReadbackFailed)?
            .map_err(|e| ComputeError::BufferMap(e.to_string()))?;
        let data = slice.get_mapped_range();
        let value = u32::from_ne_bytes(data[..4].try_into().unwrap());
        drop(data);
        buffer.unmap();
        Ok(value)
    }

    fn read_u32_vec(&self, buffer: &wgpu::Buffer, count: usize) -> Result<Vec<u32>, ComputeError> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let byte_len = (count * 4) as u64;
        let slice = buffer.slice(..byte_len);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|_| ComputeError::ReadbackFailed)?
            .map_err(|e| ComputeError::BufferMap(e.to_string()))?;
        let data = slice.get_mapped_range();
        let values: Vec<u32> = bytemuck::cast_slice(&data[..count * 4]).to_vec();
        drop(data);
        buffer.unmap();
        Ok(values)
    }
}

/// Convert a grid Cell to GpuCell format.
fn cell_to_gpu(cell: &Cell, row: u32, col: u32) -> GpuCell {
    GpuCell {
        codepoint: cell.c as u32,
        fg_packed: color_to_packed(cell.fg),
        bg_packed: color_to_packed(cell.bg),
        flags: cell.attrs.bits() as u32,
        row,
        col,
    }
}

/// Convert a grid Color to packed RGBA u32.
fn color_to_packed(color: Color) -> u32 {
    match color.to_rgba_f32() {
        Some([r, g, b, a]) => {
            pack_rgba(
                (r * 255.0) as u8,
                (g * 255.0) as u8,
                (b * 255.0) as u8,
                (a * 255.0) as u8,
            )
        }
        None => DEFAULT_FG_PACKED, // Default color
    }
}

/// Errors from compute operations.
#[derive(Debug, thiserror::Error)]
pub enum ComputeError {
    #[error("no suitable GPU adapter found")]
    NoAdapter,
    #[error("failed to request GPU device: {0}")]
    DeviceRequest(String),
    #[error("no cell data uploaded — call upload_cells first")]
    NoCellData,
    #[error("GPU readback failed")]
    ReadbackFailed,
    #[error("buffer map error: {0}")]
    BufferMap(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_gpu_cells(text: &str, cols: u32) -> Vec<GpuCell> {
        let chars: Vec<char> = text.chars().collect();
        let rows = (chars.len() as u32 + cols - 1) / cols;
        let total = (rows * cols) as usize;
        let mut cells = Vec::with_capacity(total);
        for i in 0..total {
            let cp = if i < chars.len() { chars[i] as u32 } else { 32 }; // space pad
            cells.push(GpuCell {
                codepoint: cp,
                fg_packed: DEFAULT_FG_PACKED,
                bg_packed: DEFAULT_BG_PACKED,
                flags: 0,
                row: (i as u32) / cols,
                col: (i as u32) % cols,
            });
        }
        cells
    }

    #[test]
    fn test_standalone_creation() {
        // This test requires a GPU; skip gracefully if unavailable
        match ComputeEngine::new_standalone() {
            Ok(engine) => {
                assert_eq!(engine.cell_count, 0);
                assert_eq!(engine.grid_rows, 0);
            }
            Err(ComputeError::NoAdapter) => {
                eprintln!("No GPU adapter available, skipping test");
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn test_search() {
        let engine = match ComputeEngine::new_standalone() {
            Ok(e) => e,
            Err(ComputeError::NoAdapter) => {
                eprintln!("No GPU adapter available, skipping test");
                return;
            }
            Err(e) => panic!("unexpected error: {e}"),
        };

        let cols = 20u32;
        let text = "hello world test    goodbye world again ";
        let cells = make_gpu_cells(text, cols);
        let rows = (cells.len() as u32) / cols;

        let mut engine = engine;
        engine.upload_cells_raw(&cells, rows, cols);

        let results = engine.search("world").unwrap();
        assert_eq!(results.len(), 2, "expected 2 matches for 'world'");

        // "hello world test    " → row 0, col 6
        // "goodbye world again " → row 1, col 8
        let mut positions: Vec<(u32, u32)> = results.iter().map(|r| (r.row, r.col)).collect();
        positions.sort();
        assert_eq!(positions[0], (0, 6), "first 'world' at row 0 col 6");
        assert_eq!(positions[1], (1, 8), "second 'world' at row 1 col 8");
        for r in &results {
            assert_eq!(r.length, 5, "pattern length should be 5");
        }
    }

    #[test]
    fn test_empty_search() {
        let engine = match ComputeEngine::new_standalone() {
            Ok(e) => e,
            Err(ComputeError::NoAdapter) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };

        let mut engine = engine;
        let cells = make_gpu_cells("hello", 10);
        engine.upload_cells_raw(&cells, 1, 10);

        let results = engine.search("").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_highlight() {
        let engine = match ComputeEngine::new_standalone() {
            Ok(e) => e,
            Err(ComputeError::NoAdapter) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };

        let mut engine = engine;
        // "ls -la /tmp 42"
        let text = "ls -la /tmp 42    ";
        let cols = 18u32;
        let cells = make_gpu_cells(text, cols);
        engine.upload_cells_raw(&cells, 1, cols);

        let results = engine.highlight_cells().unwrap();
        // Should find flags (-la), file path (/tmp), and numbers (42)
        assert!(!results.is_empty(), "highlight should find at least one category");

        let has_flag = results.iter().any(|r| r.category == HighlightCategory::Flag);
        let has_path = results.iter().any(|r| r.category == HighlightCategory::FilePath);
        let has_number = results.iter().any(|r| r.category == HighlightCategory::Number);
        assert!(has_flag, "should detect flag (-la)");
        assert!(has_path, "should detect file path (/tmp)");
        assert!(has_number, "should detect number (42)");

        // Verify positions: '-' at col 3, '/' at col 7, '4' at col 12
        let flag_cols: Vec<u32> = results.iter()
            .filter(|r| r.category == HighlightCategory::Flag)
            .map(|r| r.col)
            .collect();
        assert!(flag_cols.contains(&3), "flag '-' should be at col 3, got {:?}", flag_cols);

        let path_cols: Vec<u32> = results.iter()
            .filter(|r| r.category == HighlightCategory::FilePath)
            .map(|r| r.col)
            .collect();
        assert!(path_cols.contains(&7), "path '/' should be at col 7, got {:?}", path_cols);

        let num_cols: Vec<u32> = results.iter()
            .filter(|r| r.category == HighlightCategory::Number)
            .map(|r| r.col)
            .collect();
        assert!(num_cols.contains(&12), "number '4' should be at col 12, got {:?}", num_cols);
    }

    #[test]
    fn test_selection_extract() {
        let engine = match ComputeEngine::new_standalone() {
            Ok(e) => e,
            Err(ComputeError::NoAdapter) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };

        let mut engine = engine;
        let cols = 10u32;
        let text = "0123456789abcdefghij";
        let cells = make_gpu_cells(text, cols);
        engine.upload_cells_raw(&cells, 2, cols);

        // Single-row extraction: row 0, cols 3..=7 → "34567"
        let result = engine.extract_selection(0, 3, 0, 7).unwrap();
        assert_eq!(result, "34567", "single-row selection should extract cols 3-7");

        // Multi-row extraction: row 0 col 8 through row 1 col 1 → "89\nab"
        let result = engine.extract_selection(0, 8, 1, 1).unwrap();
        assert_eq!(result, "89\nab", "multi-row selection should include newline");

        // Full row extraction: row 1, all cols → "abcdefghij"
        let result = engine.extract_selection(1, 0, 1, 9).unwrap();
        assert_eq!(result, "abcdefghij", "full row selection");
    }

    #[test]
    fn test_search_no_match() {
        let engine = match ComputeEngine::new_standalone() {
            Ok(e) => e,
            Err(ComputeError::NoAdapter) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };

        let mut engine = engine;
        let cells = make_gpu_cells("hello world         ", 20);
        engine.upload_cells_raw(&cells, 1, 20);

        let results = engine.search("xyz").unwrap();
        assert!(results.is_empty(), "search for absent pattern should return empty");
    }

    #[test]
    fn test_search_positions_multirow() {
        let engine = match ComputeEngine::new_standalone() {
            Ok(e) => e,
            Err(ComputeError::NoAdapter) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };

        let mut engine = engine;
        // 3 rows of 10 cols each, "ab" at row0:col2, row1:col4, row2:col7
        let text = "xxabxxxxxxxxxxabxxxxxxxxxxxabx";
        let cells = make_gpu_cells(text, 10);
        engine.upload_cells_raw(&cells, 3, 10);

        let mut results = engine.search("ab").unwrap();
        assert_eq!(results.len(), 3, "expected 3 matches for 'ab'");

        results.sort_by_key(|r| (r.row, r.col));
        assert_eq!((results[0].row, results[0].col), (0, 2), "row 0 col 2");
        assert_eq!((results[1].row, results[1].col), (1, 4), "row 1 col 4");
        assert_eq!((results[2].row, results[2].col), (2, 7), "row 2 col 7");
        for r in &results {
            assert_eq!(r.length, 2);
        }
    }

    #[test]
    fn test_detect_urls() {
        let engine = match ComputeEngine::new_standalone() {
            Ok(e) => e,
            Err(ComputeError::NoAdapter) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };

        let mut engine = engine;
        let cols = 40u32;
        let text = "visit https://example.com/path for info ";
        let cells = make_gpu_cells(text, cols);
        let rows = (cells.len() as u32) / cols;
        engine.upload_cells_raw(&cells, rows, cols);

        let results = engine.detect_urls(0, rows).unwrap();
        assert!(!results.is_empty(), "should detect at least one URL");
        let url = &results[0];
        assert_eq!(url.row, 0);
        assert_eq!(url.start_col, 6);
        assert!(url.end_col > url.start_col);
    }

    #[test]
    fn test_detect_urls_no_urls() {
        let engine = match ComputeEngine::new_standalone() {
            Ok(e) => e,
            Err(ComputeError::NoAdapter) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };

        let mut engine = engine;
        let cols = 20u32;
        let text = "no urls here at all ";
        let cells = make_gpu_cells(text, cols);
        engine.upload_cells_raw(&cells, 1, cols);

        let results = engine.detect_urls(0, 1).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_detect_urls_no_cell_data() {
        let engine = match ComputeEngine::new_standalone() {
            Ok(e) => e,
            Err(ComputeError::NoAdapter) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };

        let result = engine.detect_urls(0, 1);
        assert!(matches!(result, Err(ComputeError::NoCellData)));
    }
}
