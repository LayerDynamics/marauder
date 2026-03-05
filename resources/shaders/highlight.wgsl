// GpuCell is prepended at load time from gpu_cell.wgsl

struct HighlightParams {
    total_rows: u32,
    cols: u32,
    _pad0: u32,
    _pad1: u32,
};

// Categories: 0=None, 1=Number, 2=FilePath, 3=Flag, 4=Operator

@group(0) @binding(0) var<storage, read> cells: array<GpuCell>;
@group(0) @binding(1) var<uniform> params: HighlightParams;
@group(0) @binding(2) var<storage, read_write> categories: array<u32>;

fn is_digit(cp: u32) -> bool {
    return cp >= 48u && cp <= 57u; // '0'-'9'
}

fn is_hex_digit(cp: u32) -> bool {
    return is_digit(cp)
        || (cp >= 65u && cp <= 70u)   // A-F
        || (cp >= 97u && cp <= 102u); // a-f
}

fn is_operator(cp: u32) -> bool {
    return cp == 124u  // |
        || cp == 38u   // &
        || cp == 59u   // ;
        || cp == 62u   // >
        || cp == 60u   // <
        || cp == 61u;  // =
}

fn is_whitespace(cp: u32) -> bool {
    return cp == 32u || cp == 9u || cp == 0u;
}

fn is_path_break(cp: u32) -> bool {
    return is_whitespace(cp) || is_operator(cp);
}

fn is_flag_char(cp: u32) -> bool {
    // alphanumeric or '-'
    return (cp >= 65u && cp <= 90u)    // A-Z
        || (cp >= 97u && cp <= 122u)   // a-z
        || (cp >= 48u && cp <= 57u)    // 0-9
        || cp == 45u;                  // -
}

@compute @workgroup_size(256)
fn classify_cells(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;
    if (row >= params.total_rows) {
        return;
    }

    let row_start = row * params.cols;

    // Sequential left-to-right scan for this row.
    // Track what "run" we're currently inside.
    // 0 = no run, 1 = number run, 2 = file path run, 3 = flag run, 5 = hex number run
    var in_run: u32 = 0u;

    for (var col: u32 = 0u; col < params.cols; col++) {
        let idx = row_start + col;
        let cp = cells[idx].codepoint;
        var category: u32 = 0u;

        let prev_ws = (col == 0u) || is_whitespace(cells[idx - 1u].codepoint);

        // Operators — standalone, always classified
        if (is_operator(cp)) {
            category = 4u;
            in_run = 0u;
        }
        // File path start: '/' or '~/' preceded by whitespace or at start
        else if ((cp == 47u) && prev_ws) {
            category = 2u;
            in_run = 2u;
        }
        else if ((cp == 126u) && prev_ws) {
            // '~' at word boundary — check if next char is '/'
            if (col + 1u < params.cols && cells[idx + 1u].codepoint == 47u) {
                category = 2u;
                in_run = 2u;
            }
        }
        // Flag start: '-' preceded by whitespace
        else if (cp == 45u && prev_ws) {
            category = 3u;
            in_run = 3u;
        }
        // Continue hex number run
        else if (in_run == 5u) {
            if (is_hex_digit(cp)) {
                category = 1u;
            } else {
                in_run = 0u;
            }
        }
        // Continue decimal number run — detect 0x prefix to enter hex mode
        else if (in_run == 1u) {
            if (is_digit(cp)) {
                category = 1u;
            } else if ((cp == 120u || cp == 88u) && col > 0u && cells[idx - 1u].codepoint == 48u) {
                // 'x' or 'X' immediately after '0' → hex prefix
                category = 1u;
                in_run = 5u;
            } else {
                in_run = 0u;
            }
        }
        // Continue file path run
        else if (in_run == 2u) {
            if (is_path_break(cp)) {
                in_run = 0u;
                if (is_digit(cp)) {
                    category = 1u;
                    in_run = 1u;
                }
            } else {
                category = 2u;
            }
        }
        // Continue flag run
        else if (in_run == 3u) {
            if (is_flag_char(cp)) {
                category = 3u;
            } else {
                in_run = 0u;
                if (is_digit(cp)) {
                    category = 1u;
                    in_run = 1u;
                }
            }
        }
        // Standalone number start
        else if (is_digit(cp)) {
            category = 1u;
            in_run = 1u;
        }
        // Whitespace or other — reset run
        else {
            in_run = 0u;
        }

        categories[idx] = category;
    }
}
