// GpuCell is prepended at load time from gpu_cell.wgsl

struct SearchParams {
    pattern_len: u32,
    total_rows: u32,
    cols: u32,
    max_results: u32,
};

@group(0) @binding(0) var<storage, read> cells: array<GpuCell>;
@group(0) @binding(1) var<uniform> params: SearchParams;
@group(0) @binding(2) var<storage, read> pattern: array<u32>;
@group(0) @binding(3) var<storage, read_write> matches: array<u32>;
@group(0) @binding(4) var<storage, read_write> match_count: atomic<u32>;

@compute @workgroup_size(256)
fn search_row(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;
    if (row >= params.total_rows) {
        return;
    }
    if (params.pattern_len == 0u || params.pattern_len > params.cols) {
        return;
    }

    let row_start = row * params.cols;
    let max_col = params.cols - params.pattern_len;

    for (var col: u32 = 0u; col <= max_col; col++) {
        var matched = true;
        for (var i: u32 = 0u; i < params.pattern_len; i++) {
            if (cells[row_start + col + i].codepoint != pattern[i]) {
                matched = false;
                break;
            }
        }
        if (matched) {
            let idx = atomicAdd(&match_count, 1u);
            if (idx < params.max_results) {
                // Two u32 slots per match: [row, col] — supports rows/cols beyond 65535
                matches[idx * 2u] = row;
                matches[idx * 2u + 1u] = col;
            }
        }
    }
}
