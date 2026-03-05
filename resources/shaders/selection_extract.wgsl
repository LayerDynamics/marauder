// GpuCell is prepended at load time from gpu_cell.wgsl

struct SelectionParams {
    start_row: u32,
    start_col: u32,
    end_row: u32,
    end_col: u32,
    cols: u32,
    max_output: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<storage, read> cells: array<GpuCell>;
@group(0) @binding(1) var<uniform> params: SelectionParams;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;
@group(0) @binding(3) var<storage, read_write> output_len: atomic<u32>;

// Each invocation handles one row. Output offsets are computed deterministically
// so rows write in parallel without ordering conflicts.

@compute @workgroup_size(256)
fn extract_selection(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row_idx = gid.x; // 0-based index into the selection range
    let row = params.start_row + row_idx;
    if (row > params.end_row) {
        return;
    }

    let total_rows = params.end_row - params.start_row + 1u;

    // Determine this row's column range
    var col_start: u32 = 0u;
    var col_end: u32 = params.cols;
    if (row == params.start_row) {
        col_start = params.start_col;
    }
    if (row == params.end_row) {
        col_end = params.end_col + 1u;
    }
    let chars_this_row = col_end - col_start;

    // Compute deterministic output offset for this row.
    // Full rows (not first, not last) contribute cols chars + 1 newline.
    // First row contributes (cols - start_col) chars + 1 newline (if not also last).
    // Last row contributes (end_col + 1) chars, no trailing newline.
    var offset: u32 = 0u;

    if (row_idx == 0u) {
        // First row — offset is 0
        offset = 0u;
    } else {
        // Start with first row's contribution: (cols - start_col) + 1 newline
        offset = (params.cols - params.start_col) + 1u;

        // Add full middle rows: each contributes cols + 1 (newline)
        if (row_idx > 1u) {
            offset += (row_idx - 1u) * (params.cols + 1u);
        }
    }

    // Bounds check
    if (offset + chars_this_row > params.max_output) {
        return;
    }

    // Copy codepoints for this row
    for (var col = col_start; col < col_end; col++) {
        let cell_idx = row * params.cols + col;
        let out_idx = offset + (col - col_start);
        output[out_idx] = cells[cell_idx].codepoint;
    }

    // Append newline after this row (but not the last row)
    if (row < params.end_row) {
        let nl_idx = offset + chars_this_row;
        if (nl_idx < params.max_output) {
            output[nl_idx] = 10u; // '\n'
        }
    }

    // The last row writes the total output length (only one thread does this)
    if (row == params.end_row) {
        let total_len = offset + chars_this_row;
        atomicMax(&output_len, total_len);
    }
}
