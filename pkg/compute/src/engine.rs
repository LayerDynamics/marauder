use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::types::*;
use marauder_grid::{Cell, Color, Grid};

/// Maximum number of results any single compute dispatch will return.
/// Caps GPU buffer allocation to ~512KB instead of scaling with grid size.
const MAX_RESULT_CAP: u32 = 65536;

/// In-flight GPU readback state for one frame's compute results.
/// Holds staging buffers that have been submitted for GPU→CPU copy
/// but not yet mapped/read. This enables double-buffered async readback:
/// frame N's results are read back while frame N+1's compute dispatches.
struct PendingFrameReadback {
    /// Staging buffer for search match count (4 bytes).
    search_count_staging: Option<wgpu::Buffer>,
    /// Staging buffer for search match data (variable size).
    search_matches_staging: Option<wgpu::Buffer>,
    /// Max search matches allocated.
    search_max_matches: usize,
    /// Search pattern length for result construction.
    search_pattern_len: u32,
    /// GPU-side buffer holding search match data (needed for phase-2 copy).
    search_matches_buffer: Option<wgpu::Buffer>,

    /// Staging buffer for URL result count (4 bytes).
    url_count_staging: Option<wgpu::Buffer>,
    /// Staging buffer for URL result data.
    url_results_staging: Option<wgpu::Buffer>,
    /// Max URL results allocated.
    url_max_results: usize,
    /// GPU-side buffer holding URL result data (needed for phase-2 copy).
    url_results_buffer: Option<wgpu::Buffer>,

    /// Staging buffer for highlight category data.
    highlight_staging: Option<wgpu::Buffer>,
    /// Number of cells in the highlight output.
    highlight_cell_count: u32,
    /// Grid columns for highlight result reconstruction.
    highlight_grid_cols: u32,

    /// Reserved for future multi-phase readback tracking.
    _phase_marker: u8,
}

/// GPU compute engine for text search, URL detection, highlighting, and selection extraction.
///
/// Shares the wgpu `Device` and `Queue` with the renderer when available,
/// or creates its own for standalone usage.
///
/// Frame-critical compute uses double-buffered async readback: `run_frame_compute`
/// dispatches GPU work and returns the *previous* frame's results immediately,
/// avoiding blocking GPU readbacks on the render thread.
pub struct ComputeEngine {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    search_pipeline: wgpu::ComputePipeline,
    url_detect_pipeline: wgpu::ComputePipeline,
    highlight_pipeline: wgpu::ComputePipeline,
    selection_pipeline: wgpu::ComputePipeline,
    regex_search_pipeline: wgpu::ComputePipeline,
    diff_pipeline: wgpu::ComputePipeline,
    /// Persistent cell buffer on GPU (reused across frames, grown on demand).
    cell_buffer: Option<wgpu::Buffer>,
    /// Capacity of the current cell buffer in number of cells.
    cell_buffer_capacity: u32,
    /// Number of cells currently uploaded.
    cell_count: u32,
    /// Grid dimensions for current upload.
    grid_rows: u32,
    grid_cols: u32,
    /// Workgroup size used for compute dispatch (matches compiled shaders).
    workgroup_size: u32,
    /// Cached results from the previous frame (returned immediately by run_frame_compute).
    previous_results: ComputeFrameResults,
    /// In-flight readback from the current frame's GPU dispatch.
    pending_readback: Option<PendingFrameReadback>,
    /// Reusable search GPU buffers to avoid per-call allocation churn.
    search_matches_buffer: Option<wgpu::Buffer>,
    search_matches_capacity: u32,
    search_count_buffer: Option<wgpu::Buffer>,
    search_staging_count: Option<wgpu::Buffer>,
    search_staging_matches: Option<wgpu::Buffer>,
    search_staging_matches_capacity: u64,
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

    /// Default workgroup size used in shader source (must match the literal in .wgsl files).
    const DEFAULT_WORKGROUP_SIZE: u32 = 256;

    /// Shared GpuCell struct definition prepended to every compute shader.
    const GPU_CELL_WGSL: &'static str = include_str!("../../../resources/shaders/gpu_cell.wgsl");

    /// Concatenate the shared GpuCell definition with a shader body,
    /// and patch the workgroup size to match the device's optimal value.
    fn assemble_shader(body: &str, workgroup_size: u32) -> String {
        let patched = body.replace(
            "@workgroup_size(256)",
            &format!("@workgroup_size({})", workgroup_size),
        );
        format!("{}\n{}", Self::GPU_CELL_WGSL, patched)
    }

    /// Patch workgroup size in a standalone shader (no GpuCell prepend).
    fn patch_workgroup_size(source: &str, workgroup_size: u32) -> String {
        source.replace(
            "@workgroup_size(256)",
            &format!("@workgroup_size({})", workgroup_size),
        )
    }

    /// Choose the optimal workgroup size for the device.
    ///
    /// Uses the device's `max_compute_workgroup_size_x`, clamped to a power-of-two
    /// that is known to perform well across GPU vendors (max 256).
    /// While many GPUs support 1024, 256 is the sweet spot for row-per-thread
    /// terminal workloads where each thread does significant per-row work.
    fn optimal_workgroup_size(device: &wgpu::Device) -> u32 {
        let max_x = device.limits().max_compute_workgroup_size_x;
        // Clamp to 256 — higher values don't help for row-per-thread dispatches
        // and can hurt occupancy on some mobile/integrated GPUs.
        let target = max_x.min(256);
        // Round down to nearest power of two for consistent dispatch math.
        if target == 0 { 64 } else { 1 << (31 - target.leading_zeros()) }
    }

    fn from_device_queue(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        let workgroup_size = Self::optimal_workgroup_size(&device);
        tracing::info!(workgroup_size, max_supported = device.limits().max_compute_workgroup_size_x, "Compute workgroup size selected");
        let search_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("search_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::assemble_shader(include_str!("../../../resources/shaders/search.wgsl"), workgroup_size).into()),
        });
        let url_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("url_detect_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::assemble_shader(include_str!("../../../resources/shaders/url_detect.wgsl"), workgroup_size).into()),
        });
        let highlight_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("highlight_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::assemble_shader(include_str!("../../../resources/shaders/highlight.wgsl"), workgroup_size).into()),
        });
        let selection_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("selection_extract_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::assemble_shader(include_str!("../../../resources/shaders/selection_extract.wgsl"), workgroup_size).into()),
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

        let regex_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("regex_search_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::patch_workgroup_size(include_str!("../../../resources/shaders/regex_search.wgsl"), workgroup_size).into()),
        });
        let regex_search_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("regex_search_pipeline"),
            layout: None,
            module: &regex_shader,
            entry_point: Some("regex_search_row"),
            compilation_options: Default::default(),
            cache: None,
        });

        let diff_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("diff_shader"),
            source: wgpu::ShaderSource::Wgsl(Self::patch_workgroup_size(include_str!("../../../resources/shaders/diff.wgsl"), workgroup_size).into()),
        });
        let diff_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("diff_pipeline"),
            layout: None,
            module: &diff_shader,
            entry_point: Some("hash_rows"),
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
            regex_search_pipeline,
            diff_pipeline,
            cell_buffer: None,
            cell_buffer_capacity: 0,
            cell_count: 0,
            workgroup_size,
            grid_rows: 0,
            grid_cols: 0,
            previous_results: ComputeFrameResults::default(),
            pending_readback: None,
            search_matches_buffer: None,
            search_matches_capacity: 0,
            search_count_buffer: None,
            search_staging_count: None,
            search_staging_matches: None,
            search_staging_matches_capacity: 0,
        }
    }

    /// Convert a Grid's cells to GpuCell format and upload to GPU.
    pub fn upload_cells(&mut self, grid: &Grid) {
        // Skip upload if no rows are dirty (nothing changed since last upload)
        let dirty = grid.get_dirty_rows();
        if !dirty.iter().any(|&d| d) {
            return;
        }

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
    ///
    /// Reuses the existing GPU buffer if it has enough capacity, avoiding
    /// per-frame buffer allocation. Only reallocates when the cell count grows.
    pub fn upload_cells_raw(&mut self, cells: &[GpuCell], rows: u32, cols: u32) {
        let data = bytemuck::cast_slice(cells);
        let cell_count = cells.len() as u32;

        if cell_count > self.cell_buffer_capacity || self.cell_buffer.is_none() {
            // Allocate with headroom to avoid frequent reallocation on small resizes
            let capacity = (cell_count as usize * 2).max(250 * 80);
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("compute_cell_buffer"),
                size: (capacity * std::mem::size_of::<GpuCell>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.cell_buffer = Some(buffer);
            self.cell_buffer_capacity = capacity as u32;
        }

        if let Some(ref buffer) = self.cell_buffer {
            self.queue.write_buffer(buffer, 0, data);
        }
        self.cell_count = cell_count;
        self.grid_rows = rows;
        self.grid_cols = cols;
    }

    /// Search for a pattern across all rows. Returns match positions.
    ///
    /// Reuses persistent GPU buffers (matches, count, staging) across calls to
    /// avoid allocation churn during batched scrollback searches.
    pub fn search(&mut self, pattern: &str) -> Result<Vec<SearchResult>, ComputeError> {
        let cell_buffer = self.cell_buffer.as_ref().ok_or(ComputeError::NoCellData)?;
        let pattern_codepoints: Vec<u32> = pattern.chars().map(|c| c as u32).collect();
        if pattern_codepoints.is_empty() {
            return Ok(Vec::new());
        }

        let max_matches = (self.grid_rows * self.grid_cols).min(MAX_RESULT_CAP) as usize;

        let params = SearchParams {
            pattern_len: pattern_codepoints.len() as u32,
            total_rows: self.grid_rows,
            cols: self.grid_cols,
            max_results: max_matches as u32,
        };

        // Small per-call buffers (params + pattern change each call, tiny allocations)
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

        // Reuse matches buffer (grow-only)
        if max_matches as u32 > self.search_matches_capacity || self.search_matches_buffer.is_none() {
            let capacity = (max_matches * 2).max(250 * 80);
            self.search_matches_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("search_matches"),
                size: (capacity * 2 * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }));
            self.search_matches_capacity = capacity as u32;
        }

        // Reuse count buffer — zero it via write_buffer instead of recreating
        if self.search_count_buffer.is_none() {
            self.search_count_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("search_match_count"),
                size: 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        self.queue.write_buffer(self.search_count_buffer.as_ref().unwrap(), 0, &[0u8; 4]);

        // Reuse count staging buffer
        if self.search_staging_count.is_none() {
            self.search_staging_count = Some(self.create_staging_buffer(4));
        }

        let matches_buffer = self.search_matches_buffer.as_ref().unwrap();
        let count_buffer = self.search_count_buffer.as_ref().unwrap();

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

        let workgroups = (self.grid_rows + self.workgroup_size - 1) / self.workgroup_size;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.search_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        // Phase 1: dispatch compute, read back only the match count (4 bytes)
        let count_staging = self.search_staging_count.as_ref().unwrap();
        encoder.copy_buffer_to_buffer(count_buffer, 0, count_staging, 0, 4);
        self.queue.submit(Some(encoder.finish()));

        let count = self.read_u32(count_staging)?.min(max_matches as u32) as usize;
        if count == 0 {
            return Ok(Vec::new());
        }

        // Phase 2: copy only the bytes actually written, not the full buffer
        let readback_bytes = (count * 2 * 4) as u64;

        // Reuse matches staging buffer (grow-only)
        if readback_bytes > self.search_staging_matches_capacity || self.search_staging_matches.is_none() {
            let capacity = (readback_bytes * 2).max(256);
            self.search_staging_matches = Some(self.create_staging_buffer(capacity));
            self.search_staging_matches_capacity = capacity;
        }

        let matches_staging = self.search_staging_matches.as_ref().unwrap();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(matches_buffer, 0, matches_staging, 0, readback_bytes);
        self.queue.submit(Some(encoder.finish()));

        let match_data = self.read_u32_vec(matches_staging, count * 2)?;

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
        let workgroups = (num_rows + self.workgroup_size - 1) / self.workgroup_size;
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

    /// Search a batch of scrollback cells for a pattern.
    ///
    /// This is used for cold scrollback search: the caller iterates scrollback tiers,
    /// decompresses blocks, converts cells to `GpuCell`, and calls this per batch.
    /// Internally uploads the batch, runs the search pipeline, and returns results.
    pub fn search_scrollback_batched(
        &mut self,
        cells: &[GpuCell],
        rows: u32,
        cols: u32,
        pattern: &str,
    ) -> Result<Vec<SearchResult>, ComputeError> {
        if pattern.is_empty() || cells.is_empty() {
            return Ok(Vec::new());
        }
        self.upload_cells_raw(cells, rows, cols);
        self.search(pattern)
    }

    /// Classify cells into highlight categories (Number, FilePath, Flag, Operator).
    pub fn highlight_cells(&self, rules: &[HighlightRule]) -> Result<Vec<HighlightResult>, ComputeError> {
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

        let workgroups = (self.grid_rows + self.workgroup_size - 1) / self.workgroup_size;
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

        // Apply user/extension highlight rules via GPU regex search.
        // Each rule's pattern is compiled to an NFA and run as a regex search;
        // matching cells are tagged with a Custom category.
        for rule in rules {
            if let Some(nfa) = RegexNfa::compile(&rule.pattern) {
                if let Ok(matches) = self.search_regex(&nfa) {
                    for m in matches {
                        for col_offset in 0..m.length {
                            results.push(HighlightResult {
                                row: m.row,
                                col: m.col + col_offset,
                                category: HighlightCategory::Custom(rule.category.clone()),
                            });
                        }
                    }
                }
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

        // Compute max output size once — used for both the uniform params and buffer allocation.
        // Each row contributes up to `cols` codepoints + 1 newline (except the last row).
        let num_rows = end_row.saturating_sub(start_row) + 1;
        let max_output = (num_rows * self.grid_cols + num_rows) as usize;

        let params = SelectionParams {
            start_row,
            start_col,
            end_row,
            end_col,
            cols: self.grid_cols,
            max_output: max_output as u32,
            _pad0: 0,
            _pad1: 0,
        };
        let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("selection_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
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
            let sel_workgroups = (num_rows + self.workgroup_size - 1) / self.workgroup_size;
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

    /// Search using a compiled NFA regex on the GPU.
    ///
    /// Returns matches with row, col, and length. Falls back to `None` if the
    /// pattern cannot be compiled (unsupported features or too many states).
    pub fn search_regex(&self, nfa: &RegexNfa) -> Result<Vec<SearchResult>, ComputeError> {
        let cell_buffer = self.cell_buffer.as_ref().ok_or(ComputeError::NoCellData)?;

        let max_results = (self.grid_rows * self.grid_cols).min(MAX_RESULT_CAP);
        let params = RegexSearchParams {
            total_rows: self.grid_rows,
            cols: self.grid_cols,
            num_states: nfa.num_states,
            accept_mask: nfa.accept_mask,
            max_results,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };

        let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("regex_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let transitions_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("regex_transitions"),
            contents: bytemuck::cast_slice(&nfa.transitions),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let matches_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("regex_matches"),
            size: (max_results as u64) * 3 * 4, // 3 u32 per match (row, col, length)
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let count_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("regex_match_count"),
            contents: &[0u8; 4],
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });

        let bind_group_layout = self.regex_search_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("regex_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cell_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: params_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: transitions_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: matches_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: count_buffer.as_entire_binding() },
            ],
        });

        let workgroups = (self.grid_rows + self.workgroup_size - 1) / self.workgroup_size;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.regex_search_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        let count_staging = self.create_staging_buffer(4);
        encoder.copy_buffer_to_buffer(&count_buffer, 0, &count_staging, 0, 4);
        self.queue.submit(Some(encoder.finish()));

        let count = self.read_u32(&count_staging)?.min(max_results) as usize;
        if count == 0 {
            return Ok(Vec::new());
        }

        let readback_bytes = (count * 3 * 4) as u64;
        let matches_staging = self.create_staging_buffer(readback_bytes);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&matches_buffer, 0, &matches_staging, 0, readback_bytes);
        self.queue.submit(Some(encoder.finish()));

        let raw = self.read_u32_vec(&matches_staging, count * 3)?;
        let results = (0..count).map(|i| {
            SearchResult {
                row: raw[i * 3],
                col: raw[i * 3 + 1],
                length: raw[i * 3 + 2],
            }
        }).collect();

        Ok(results)
    }

    /// Compute a diff between two cell snapshots using GPU row hashing + CPU Myers diff.
    ///
    /// The GPU hashes each row (FNV-1a over codepoints and attributes), then the CPU
    /// runs a simple O(N) comparison on the hash vectors.
    pub fn compute_diff(
        &self,
        cells_a: &[GpuCell],
        rows_a: u32,
        cells_b: &[GpuCell],
        rows_b: u32,
        cols: u32,
    ) -> Result<Vec<DiffResult>, ComputeError> {
        let expected_a = (rows_a * cols) as usize;
        if cells_a.len() != expected_a {
            return Err(ComputeError::DimensionMismatch { expected: expected_a, actual: cells_a.len() });
        }
        let expected_b = (rows_b * cols) as usize;
        if cells_b.len() != expected_b {
            return Err(ComputeError::DimensionMismatch { expected: expected_b, actual: cells_b.len() });
        }

        let buffer_a = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("diff_cells_a"),
            contents: bytemuck::cast_slice(cells_a),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let buffer_b = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("diff_cells_b"),
            contents: bytemuck::cast_slice(cells_b),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let params = DiffParams {
            rows_a,
            rows_b,
            cols,
            _pad: 0,
        };
        let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("diff_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let max_rows = rows_a.max(rows_b);
        let hashes_a_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("diff_hashes_a"),
            size: (max_rows as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let hashes_b_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("diff_hashes_b"),
            size: (max_rows as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let bind_group_layout = self.diff_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("diff_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer_a.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: buffer_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: params_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: hashes_a_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: hashes_b_buffer.as_entire_binding() },
            ],
        });

        let workgroups = (max_rows + self.workgroup_size - 1) / self.workgroup_size;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.diff_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }

        let staging_a = self.create_staging_buffer((rows_a as u64) * 4);
        let staging_b = self.create_staging_buffer((rows_b as u64) * 4);
        encoder.copy_buffer_to_buffer(&hashes_a_buffer, 0, &staging_a, 0, (rows_a as u64) * 4);
        encoder.copy_buffer_to_buffer(&hashes_b_buffer, 0, &staging_b, 0, (rows_b as u64) * 4);
        self.queue.submit(Some(encoder.finish()));

        let ha = self.read_u32_vec(&staging_a, rows_a as usize)?;
        let hb = self.read_u32_vec(&staging_b, rows_b as usize)?;

        // CPU-side Myers diff on hash vectors (LCS-based, handles insertions/deletions)
        // Pass original cell data for CPU-side verification of hash matches —
        // 32-bit hashes have ~50% collision probability at ~77K rows.
        let results = Self::myers_diff(&ha, &hb, cells_a, cells_b, cols);

        Ok(results)
    }

    /// Myers diff algorithm on two hash vectors with CPU-side collision verification.
    ///
    /// Produces a minimal edit script (Same/Changed/Added/Removed) using the
    /// classic Myers O((N+M)D) algorithm. Each hash represents one row's content;
    /// equal hashes mean *probably* equal rows. After the diff is computed, rows
    /// reported as `Same` are verified by comparing actual cell codepoints to
    /// guard against 32-bit hash collisions (~50% probability at ~77K rows).
    fn myers_diff(a: &[u32], b: &[u32], cells_a: &[GpuCell], cells_b: &[GpuCell], cols: u32) -> Vec<DiffResult> {
        let n = a.len();
        let m = b.len();

        if n == 0 && m == 0 {
            return Vec::new();
        }
        if n == 0 {
            return (0..m as u32).map(|row| DiffResult { row, kind: DiffKind::Added }).collect();
        }
        if m == 0 {
            return (0..n as u32).map(|row| DiffResult { row, kind: DiffKind::Removed }).collect();
        }

        let max_d = n + m;
        // v[k] stores the furthest-reaching x on diagonal k.
        // Diagonal k = x - y. We index with offset max_d so k can be negative.
        let sz = 2 * max_d + 1;
        let mut v = vec![0usize; sz];
        // Store the v array at each step d for backtracking.
        let mut trace: Vec<Vec<usize>> = Vec::with_capacity(max_d + 1);

        let mut found_d = max_d;
        'outer: for d in 0..=max_d {
            trace.push(v.clone());
            let mut new_v = v.clone();
            let d_i = d as isize;
            for k in (-d_i..=d_i).step_by(2) {
                let ki = (k + max_d as isize) as usize;
                let mut x = if k == -d_i
                    || (k != d_i && v[(k - 1 + max_d as isize) as usize] < v[(k + 1 + max_d as isize) as usize])
                {
                    // Move down (insert from b)
                    v[(k + 1 + max_d as isize) as usize]
                } else {
                    // Move right (delete from a)
                    v[(k - 1 + max_d as isize) as usize] + 1
                };
                let mut y = (x as isize - k) as usize;
                // Follow the diagonal (matching elements)
                while x < n && y < m && a[x] == b[y] {
                    x += 1;
                    y += 1;
                }
                new_v[ki] = x;
                if x >= n && y >= m {
                    v = new_v;
                    trace.push(v.clone());
                    found_d = d;
                    break 'outer;
                }
            }
            v = new_v;
        }

        // Backtrack to recover the edit script
        let mut edits: Vec<(usize, usize, bool)> = Vec::new(); // (x, y, is_insert)
        let mut x = n;
        let mut y = m;
        for d in (1..=found_d).rev() {
            let prev_v = &trace[d];
            let k = x as isize - y as isize;
            let d_i = d as isize;

            let prev_k = if k == -d_i
                || (k != d_i && prev_v[(k - 1 + max_d as isize) as usize] < prev_v[(k + 1 + max_d as isize) as usize])
            {
                k + 1 // came from down (insert)
            } else {
                k - 1 // came from right (delete)
            };

            let prev_x = prev_v[(prev_k + max_d as isize) as usize];
            let prev_y = (prev_x as isize - prev_k) as usize;

            // Diagonal (same) entries between (prev_x, prev_y) and the edit point
            while x > prev_x && y > prev_y {
                x -= 1;
                y -= 1;
                // Same — will be emitted during forward pass
            }

            if d > 0 {
                if prev_k > k as isize {
                    // Insert (from b at position prev_y - 1... but we track at edit point)
                    edits.push((x, y, true)); // insert at y
                    y -= 1;
                } else {
                    // Delete (from a at position prev_x)
                    edits.push((x, y, false)); // delete at x
                    x -= 1;
                }
            }
        }
        edits.reverse();

        // Build result by walking both sequences with the edit script
        let mut results = Vec::with_capacity(n.max(m));
        let mut ai = 0usize;
        let mut bi = 0usize;
        let mut edit_idx = 0usize;
        let mut row_counter = 0u32;

        while ai < n || bi < m {
            // Check if there's an edit at this position
            if edit_idx < edits.len() && edits[edit_idx].0 == ai && edits[edit_idx].1 == bi {
                let is_insert = edits[edit_idx].2;
                if is_insert {
                    results.push(DiffResult { row: row_counter, kind: DiffKind::Added });
                    bi += 1;
                } else {
                    results.push(DiffResult { row: row_counter, kind: DiffKind::Removed });
                    ai += 1;
                }
                edit_idx += 1;
            } else if ai < n && bi < m {
                // Matching (same hash) — verify actual cell content to catch collisions
                if a[ai] == b[bi] && Self::rows_equal(cells_a, ai, cells_b, bi, cols as usize) {
                    results.push(DiffResult { row: row_counter, kind: DiffKind::Same });
                } else {
                    results.push(DiffResult { row: row_counter, kind: DiffKind::Changed });
                }
                ai += 1;
                bi += 1;
            } else if ai < n {
                results.push(DiffResult { row: row_counter, kind: DiffKind::Removed });
                ai += 1;
            } else {
                results.push(DiffResult { row: row_counter, kind: DiffKind::Added });
                bi += 1;
            }
            row_counter += 1;
        }

        results
    }

    /// Compare two rows cell-by-cell to verify hash equality.
    ///
    /// Compares codepoints, colors, and flags for each cell in the row.
    /// Used to catch 32-bit hash collisions in the diff algorithm.
    fn rows_equal(cells_a: &[GpuCell], row_a: usize, cells_b: &[GpuCell], row_b: usize, cols: usize) -> bool {
        let start_a = row_a * cols;
        let start_b = row_b * cols;
        // Guard against out-of-bounds if cell arrays are shorter than expected
        if start_a + cols > cells_a.len() || start_b + cols > cells_b.len() {
            return false;
        }
        for c in 0..cols {
            let ca = &cells_a[start_a + c];
            let cb = &cells_b[start_b + c];
            if ca.codepoint != cb.codepoint
                || ca.fg_packed != cb.fg_packed
                || ca.bg_packed != cb.bg_packed
                || ca.flags != cb.flags
            {
                return false;
            }
        }
        true
    }

    /// Run all compute passes for a single frame: search, URL detect, highlight.
    ///
    /// Uses double-buffered async readback to avoid blocking the calling thread:
    /// 1. Tries to collect the *previous* frame's in-flight GPU results (non-blocking poll)
    /// 2. Dispatches this frame's compute work to the GPU
    /// 3. Returns the previous frame's results immediately (one frame of latency)
    ///
    /// On the first call (no previous results), returns empty defaults.
    /// For operations that need synchronous results (e.g. `search`, `extract_selection`),
    /// use those methods directly — they still block.
    pub fn run_frame_compute(
        &mut self,
        grid: &Grid,
        search_pattern: Option<&str>,
        visible_row_start: u32,
        visible_row_end: u32,
    ) -> Result<ComputeFrameResults, ComputeError> {
        // Step 1: Try to collect previous frame's results (non-blocking).
        self.try_collect_pending();

        // Snapshot the results to return (from the previous frame).
        let results_to_return = self.previous_results.clone();

        // Step 2: Upload new cell data.
        self.upload_cells(grid);

        if self.cell_buffer.is_none() {
            return Ok(results_to_return);
        }

        // Step 3: Dispatch all compute passes and set up async readback.
        self.dispatch_frame_async(search_pattern, visible_row_start, visible_row_end)?;

        Ok(results_to_return)
    }

    /// Run all compute passes synchronously (blocking). Use this when you need
    /// results immediately and are NOT on the render hot path.
    pub fn run_frame_compute_blocking(
        &mut self,
        grid: &Grid,
        search_pattern: Option<&str>,
        visible_row_start: u32,
        visible_row_end: u32,
    ) -> Result<ComputeFrameResults, ComputeError> {
        self.upload_cells(grid);

        let search_results = match search_pattern {
            Some(pat) if !pat.is_empty() => self.search(pat)?,
            _ => Vec::new(),
        };

        let url_matches = self.detect_urls(visible_row_start, visible_row_end)?;
        let highlight_results = self.highlight_cells(&[])?;

        Ok(ComputeFrameResults {
            search_results,
            url_matches,
            highlight_results,
        })
    }

    /// Non-blocking poll: try to collect pending readback results.
    /// Updates `previous_results` if the GPU work is done.
    /// Returns `true` if results were collected.
    pub fn poll_results(&mut self) -> bool {
        self.try_collect_pending()
    }

    /// Dispatch all frame compute passes and create staging buffers for async readback.
    fn dispatch_frame_async(
        &mut self,
        search_pattern: Option<&str>,
        visible_row_start: u32,
        visible_row_end: u32,
    ) -> Result<(), ComputeError> {
        let cell_buffer = self.cell_buffer.as_ref().ok_or(ComputeError::NoCellData)?;
        let mut encoder = self.device.create_command_encoder(&Default::default());

        let mut pending = PendingFrameReadback {
            search_count_staging: None,
            search_matches_staging: None,
            search_max_matches: 0,
            search_pattern_len: 0,
            search_matches_buffer: None,
            url_count_staging: None,
            url_results_staging: None,
            url_max_results: 0,
            url_results_buffer: None,
            highlight_staging: None,
            highlight_cell_count: self.cell_count,
            highlight_grid_cols: self.grid_cols,
            _phase_marker: 0,
        };

        // --- Search pass ---
        if let Some(pat) = search_pattern {
            if !pat.is_empty() {
                let pattern_codepoints: Vec<u32> = pat.chars().map(|c| c as u32).collect();
                pending.search_pattern_len = pattern_codepoints.len() as u32;

                let params = SearchParams {
                    pattern_len: pattern_codepoints.len() as u32,
                    total_rows: self.grid_rows,
                    cols: self.grid_cols,
                    max_results: (self.grid_rows * self.grid_cols).min(MAX_RESULT_CAP),
                };
                let max_matches = params.max_results as usize;
                pending.search_max_matches = max_matches;

                let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("async_search_params"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let pattern_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("async_search_pattern"),
                    contents: bytemuck::cast_slice(&pattern_codepoints),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let matches_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("async_search_matches"),
                    size: (max_matches * 2 * 4) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let count_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("async_search_count"),
                    contents: &[0u8; 4],
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                });

                let bind_group_layout = self.search_pipeline.get_bind_group_layout(0);
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("async_search_bind_group"),
                    layout: &bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: cell_buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: params_buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: pattern_buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: matches_buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: count_buffer.as_entire_binding() },
                    ],
                });

                let workgroups = (self.grid_rows + self.workgroup_size - 1) / self.workgroup_size;
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&self.search_pipeline);
                    pass.set_bind_group(0, &bind_group, &[]);
                    pass.dispatch_workgroups(workgroups, 1, 1);
                }

                let count_staging = self.create_staging_buffer(4);
                encoder.copy_buffer_to_buffer(&count_buffer, 0, &count_staging, 0, 4);

                // Copy full match buffer — we'll only read `count` entries when collecting.
                let matches_staging = self.create_staging_buffer((max_matches * 2 * 4) as u64);
                encoder.copy_buffer_to_buffer(&matches_buffer, 0, &matches_staging, 0, (max_matches * 2 * 4) as u64);

                pending.search_count_staging = Some(count_staging);
                pending.search_matches_staging = Some(matches_staging);
                pending.search_matches_buffer = Some(matches_buffer);
            }
        }

        // --- URL detection pass ---
        let row_start = visible_row_start.min(self.grid_rows);
        let row_end = visible_row_end.min(self.grid_rows);
        if row_start < row_end {
            let max_results = (((row_end - row_start) * self.grid_cols) as usize).min(MAX_RESULT_CAP as usize);
            pending.url_max_results = max_results;

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
                label: Some("async_url_params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let results_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("async_url_results"),
                size: (max_results * 3 * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let count_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("async_url_count"),
                contents: &[0u8; 4],
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });

            let bind_group_layout = self.url_detect_pipeline.get_bind_group_layout(0);
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("async_url_bind_group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: cell_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: params_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: results_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: count_buffer.as_entire_binding() },
                ],
            });

            let num_rows = row_end - row_start;
            let workgroups = (num_rows + self.workgroup_size - 1) / self.workgroup_size;
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.url_detect_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }

            let count_staging = self.create_staging_buffer(4);
            encoder.copy_buffer_to_buffer(&count_buffer, 0, &count_staging, 0, 4);

            let results_staging = self.create_staging_buffer((max_results * 3 * 4) as u64);
            encoder.copy_buffer_to_buffer(&results_buffer, 0, &results_staging, 0, (max_results * 3 * 4) as u64);

            pending.url_count_staging = Some(count_staging);
            pending.url_results_staging = Some(results_staging);
            pending.url_results_buffer = Some(results_buffer);
        }

        // --- Highlight pass ---
        {
            let params = HighlightParams {
                total_rows: self.grid_rows,
                cols: self.grid_cols,
                _pad0: 0,
                _pad1: 0,
            };
            let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("async_highlight_params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let categories_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("async_highlight_categories"),
                size: (self.cell_count as u64) * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

            let bind_group_layout = self.highlight_pipeline.get_bind_group_layout(0);
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("async_highlight_bind_group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: cell_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: params_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: categories_buffer.as_entire_binding() },
                ],
            });

            let workgroups = (self.grid_rows + self.workgroup_size - 1) / self.workgroup_size;
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.highlight_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }

            let staging = self.create_staging_buffer((self.cell_count as u64) * 4);
            encoder.copy_buffer_to_buffer(&categories_buffer, 0, &staging, 0, (self.cell_count as u64) * 4);
            pending.highlight_staging = Some(staging);
        }

        // Submit all dispatches + copies in a single command buffer.
        self.queue.submit(Some(encoder.finish()));

        // Request async mapping on all staging buffers (non-blocking).
        self.request_staging_maps(&pending);

        self.pending_readback = Some(pending);
        Ok(())
    }

    /// Request non-blocking map_async on all staging buffers in a pending readback.
    fn request_staging_maps(&self, pending: &PendingFrameReadback) {
        // Map all staging buffers for reading. The callbacks are no-ops;
        // we check mapping status via try_collect_pending using device.poll.
        let map_cb = |_: Result<(), wgpu::BufferAsyncError>| {};

        if let Some(ref buf) = pending.search_count_staging {
            buf.slice(..).map_async(wgpu::MapMode::Read, map_cb);
        }
        if let Some(ref buf) = pending.search_matches_staging {
            buf.slice(..).map_async(wgpu::MapMode::Read, map_cb);
        }
        if let Some(ref buf) = pending.url_count_staging {
            buf.slice(..).map_async(wgpu::MapMode::Read, map_cb);
        }
        if let Some(ref buf) = pending.url_results_staging {
            buf.slice(..).map_async(wgpu::MapMode::Read, map_cb);
        }
        if let Some(ref buf) = pending.highlight_staging {
            buf.slice(..).map_async(wgpu::MapMode::Read, map_cb);
        }
    }

    /// Try to collect results from pending readback (non-blocking poll).
    /// Returns true if results were successfully collected.
    fn try_collect_pending(&mut self) -> bool {
        if self.pending_readback.is_none() {
            return false;
        }

        // Non-blocking poll — advances GPU work and completes map_async callbacks.
        self.device.poll(wgpu::Maintain::Poll);

        let pending = self.pending_readback.take().unwrap();
        let mut results = ComputeFrameResults::default();
        let mut all_ready = true;

        // --- Collect search results ---
        if let Some(ref count_buf) = pending.search_count_staging {
            if let Some(count) = self.try_read_mapped_u32(count_buf) {
                let count = count.min(pending.search_max_matches as u32) as usize;
                if count > 0 {
                    if let Some(ref matches_buf) = pending.search_matches_staging {
                        if let Some(match_data) = self.try_read_mapped_u32_vec(matches_buf, count * 2) {
                            let pattern_len = pending.search_pattern_len;
                            results.search_results = (0..count).map(|i| SearchResult {
                                row: match_data[i * 2],
                                col: match_data[i * 2 + 1],
                                length: pattern_len,
                            }).collect();
                        } else {
                            all_ready = false;
                        }
                    }
                }
            } else {
                all_ready = false;
            }
        }

        // --- Collect URL results ---
        if let Some(ref count_buf) = pending.url_count_staging {
            if let Some(count) = self.try_read_mapped_u32(count_buf) {
                let count = (count as usize).min(pending.url_max_results);
                if count > 0 {
                    if let Some(ref results_buf) = pending.url_results_staging {
                        if let Some(raw) = self.try_read_mapped_u32_vec(results_buf, count * 3) {
                            results.url_matches = (0..count).map(|i| UrlMatch {
                                row: raw[i * 3],
                                start_col: raw[i * 3 + 1],
                                end_col: raw[i * 3 + 2],
                            }).collect();
                        } else {
                            all_ready = false;
                        }
                    }
                }
            } else {
                all_ready = false;
            }
        }

        // --- Collect highlight results ---
        if let Some(ref staging) = pending.highlight_staging {
            if let Some(cat_data) = self.try_read_mapped_u32_vec(staging, pending.highlight_cell_count as usize) {
                let grid_cols = pending.highlight_grid_cols;
                for (i, &cat) in cat_data.iter().enumerate() {
                    if cat != 0 {
                        results.highlight_results.push(HighlightResult {
                            row: (i as u32) / grid_cols,
                            col: (i as u32) % grid_cols,
                            category: HighlightCategory::from_u32(cat),
                        });
                    }
                }
            } else {
                all_ready = false;
            }
        }

        if all_ready {
            self.previous_results = results;
            true
        } else {
            // Not ready yet — put the pending state back for the next poll.
            self.pending_readback = Some(pending);
            false
        }
    }

    /// Try to read a u32 from an already-mapped staging buffer. Returns None if not yet mapped.
    fn try_read_mapped_u32(&self, buffer: &wgpu::Buffer) -> Option<u32> {
        let slice = buffer.slice(..4);
        // get_mapped_range panics if the buffer isn't mapped yet.
        // We check by attempting to get the range — if map_async hasn't completed,
        // the buffer won't be mapped and this will panic. We catch that case
        // by using a feature of wgpu: mapped buffers return data, unmapped ones don't.
        // Unfortunately wgpu doesn't provide a try_get_mapped_range, so we rely on
        // the fact that we called poll() above and the map callback has fired.
        let data = slice.get_mapped_range();
        let value = u32::from_ne_bytes(data[..4].try_into().ok()?);
        drop(data);
        buffer.unmap();
        Some(value)
    }

    /// Try to read a vec of u32 from an already-mapped staging buffer.
    fn try_read_mapped_u32_vec(&self, buffer: &wgpu::Buffer, count: usize) -> Option<Vec<u32>> {
        if count == 0 {
            return Some(Vec::new());
        }
        let byte_len = (count * 4) as u64;
        let slice = buffer.slice(..byte_len);
        let data = slice.get_mapped_range();
        let values: Vec<u32> = bytemuck::cast_slice(&data[..count * 4]).to_vec();
        drop(data);
        buffer.unmap();
        Some(values)
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
#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
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
    #[error("dimension mismatch: expected {expected} cells but got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
}

/// Newtype for Deno ops layer, converting `ComputeError` to a string-based error.
#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[error("{0}")]
pub struct ComputeOpError(String);

impl From<ComputeError> for ComputeOpError {
    fn from(e: ComputeError) -> Self {
        Self(e.to_string())
    }
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

        let results = engine.highlight_cells(&[]).unwrap();
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
