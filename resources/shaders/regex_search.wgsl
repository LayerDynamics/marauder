// GPU NFA regex search shader.
//
// Each workgroup thread processes one row, walking a Thompson NFA
// across cell codepoints. Matches are written to the output buffer.
//
// The NFA is represented as a bitmask of active states (max 32 states).
// Transition table: transitions[state * 256 + input_class] = next_state_bitmask.

struct GpuCell {
    codepoint: u32,
    fg_packed: u32,
    bg_packed: u32,
    flags: u32,
    row: u32,
    col: u32,
};

struct RegexSearchParams {
    total_rows: u32,
    cols: u32,
    num_states: u32,
    accept_mask: u32,
    max_results: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<storage, read> cells: array<GpuCell>;
@group(0) @binding(1) var<uniform> params: RegexSearchParams;
@group(0) @binding(2) var<storage, read> transitions: array<u32>;
@group(0) @binding(3) var<storage, read_write> matches: array<u32>;
@group(0) @binding(4) var<storage, read_write> match_count: atomic<u32>;

@compute @workgroup_size(256)
fn regex_search_row(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;
    if (row >= params.total_rows) {
        return;
    }

    let row_start = row * params.cols;

    // Slide the NFA start position across each column
    for (var start_col: u32 = 0u; start_col < params.cols; start_col++) {
        var state_mask: u32 = 1u; // Start state is always state 0

        for (var col: u32 = start_col; col < params.cols; col++) {
            let cell_idx = row_start + col;
            let input = cells[cell_idx].codepoint;

            // Compute next state mask from all active states.
            // Codepoints > 255 have no entry in the 256-wide transition table,
            // so skip the lookup entirely (next_mask stays 0, killing the match).
            // This prevents false matches on CJK, emoji, and accented characters
            // that were previously clamped to index 0 (NUL).
            var next_mask: u32 = 0u;
            if (input <= 255u) {
                for (var s: u32 = 0u; s < params.num_states; s++) {
                    if ((state_mask & (1u << s)) != 0u) {
                        let tidx = s * 256u + input;
                        next_mask = next_mask | transitions[tidx];
                    }
                }
            }

            state_mask = next_mask;

            // No active states — pattern cannot match from this start position
            if (state_mask == 0u) {
                break;
            }

            // Check for accept state
            if ((state_mask & params.accept_mask) != 0u) {
                // Early-out: skip atomic if buffer is already full (non-atomic read is safe
                // here — worst case we do one extra atomicAdd that the bounds check catches).
                let current = atomicLoad(&match_count);
                if (current < params.max_results) {
                    let idx = atomicAdd(&match_count, 1u);
                    if (idx < params.max_results) {
                        let match_len = col - start_col + 1u;
                        matches[idx * 3u] = row;
                        matches[idx * 3u + 1u] = start_col;
                        matches[idx * 3u + 2u] = match_len;
                    }
                }
                break; // Move to next start position after finding a match
            }
        }
    }
}
