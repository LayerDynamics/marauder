// GpuCell is prepended at load time from gpu_cell.wgsl

struct UrlDetectParams {
    total_rows: u32,
    cols: u32,
    row_start: u32,
    row_end: u32,
    max_results: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<storage, read> cells: array<GpuCell>;
@group(0) @binding(1) var<uniform> params: UrlDetectParams;
@group(0) @binding(2) var<storage, read_write> results: array<u32>;
@group(0) @binding(3) var<storage, read_write> result_count: atomic<u32>;

fn is_url_terminator(cp: u32) -> bool {
    return cp <= 32u       // space and control chars
        || cp == 34u       // "
        || cp == 39u       // '
        || cp == 60u       // <
        || cp == 62u       // >
        || cp == 96u       // `
        || cp == 123u      // {
        || cp == 125u;     // }
}

// Trailing chars that are typically sentence punctuation, not part of the URL,
// UNLESS they have a matching opener inside the URL body.
fn is_trailing_punct(cp: u32) -> bool {
    return cp == 46u       // .
        || cp == 44u       // ,
        || cp == 59u       // ;
        || cp == 58u       // :
        || cp == 33u       // !
        || cp == 63u;      // ?
}

// Strip trailing punctuation and unbalanced closers from a URL span.
// Handles ), ], and sentence-ending punctuation (.,:;!?).
// Preserves balanced parens/brackets so Wikipedia-style URLs work.
fn strip_trailing(base: u32, start: u32, raw_end: u32) -> u32 {
    var end = raw_end;
    while (end > start) {
        let last = cp_at(base, end - 1u);

        // Simple trailing punctuation — always strip
        if (is_trailing_punct(last)) {
            end -= 1u;
            continue;
        }

        // Closing paren — strip only if unbalanced within the URL
        if (last == 41u) { // )
            var depth: i32 = 0;
            for (var i = start; i < end; i++) {
                let c = cp_at(base, i);
                if (c == 40u) { depth += 1; } // (
                if (c == 41u) { depth -= 1; } // )
            }
            if (depth < 0) {
                end -= 1u;
                continue;
            }
            break;
        }

        // Closing bracket — strip only if unbalanced
        if (last == 93u) { // ]
            var depth: i32 = 0;
            for (var i = start; i < end; i++) {
                let c = cp_at(base, i);
                if (c == 91u) { depth += 1; } // [
                if (c == 93u) { depth -= 1; } // ]
            }
            if (depth < 0) {
                end -= 1u;
                continue;
            }
            break;
        }

        break;
    }
    return end;
}

fn is_alnum(cp: u32) -> bool {
    return (cp >= 48u && cp <= 57u)    // 0-9
        || (cp >= 65u && cp <= 90u)    // A-Z
        || (cp >= 97u && cp <= 122u);  // a-z
}

fn is_alpha(cp: u32) -> bool {
    return (cp >= 65u && cp <= 90u)    // A-Z
        || (cp >= 97u && cp <= 122u);  // a-z
}

fn cp_at(base: u32, offset: u32) -> u32 {
    return cells[base + offset].codepoint;
}

// Match "://" at position
fn match_scheme_sep(base: u32, col: u32, cols: u32) -> bool {
    if (col + 3u > cols) { return false; }
    return cp_at(base, col) == 58u       // :
        && cp_at(base, col + 1u) == 47u  // /
        && cp_at(base, col + 2u) == 47u; // /
}

// Match "http" at position
fn match_http(base: u32, col: u32, cols: u32) -> bool {
    if (col + 4u > cols) { return false; }
    return cp_at(base, col) == 104u      // h
        && cp_at(base, col + 1u) == 116u // t
        && cp_at(base, col + 2u) == 116u // t
        && cp_at(base, col + 3u) == 112u;// p
}

// Match "ftp" at position
fn match_ftp(base: u32, col: u32, cols: u32) -> bool {
    if (col + 3u > cols) { return false; }
    return cp_at(base, col) == 102u      // f
        && cp_at(base, col + 1u) == 116u // t
        && cp_at(base, col + 2u) == 112u;// p
}

// Match "file" at position
fn match_file(base: u32, col: u32, cols: u32) -> bool {
    if (col + 4u > cols) { return false; }
    return cp_at(base, col) == 102u      // f
        && cp_at(base, col + 1u) == 105u // i
        && cp_at(base, col + 2u) == 108u // l
        && cp_at(base, col + 3u) == 101u;// e
}

// Match "ssh" at position
fn match_ssh(base: u32, col: u32, cols: u32) -> bool {
    if (col + 3u > cols) { return false; }
    return cp_at(base, col) == 115u      // s
        && cp_at(base, col + 1u) == 115u // s
        && cp_at(base, col + 2u) == 104u;// h
}

// Match "mailto:" at position (7 chars)
fn match_mailto(base: u32, col: u32, cols: u32) -> bool {
    if (col + 7u > cols) { return false; }
    return cp_at(base, col) == 109u      // m
        && cp_at(base, col + 1u) == 97u  // a
        && cp_at(base, col + 2u) == 105u // i
        && cp_at(base, col + 3u) == 108u // l
        && cp_at(base, col + 4u) == 116u // t
        && cp_at(base, col + 5u) == 111u // o
        && cp_at(base, col + 6u) == 58u; // :
}

fn emit_result(row: u32, start_col: u32, end_col: u32) {
    let idx = atomicAdd(&result_count, 1u);
    if (idx < params.max_results) {
        results[idx * 3u] = row;
        results[idx * 3u + 1u] = start_col;
        results[idx * 3u + 2u] = end_col;
    }
}

// Try to match a scheme-based URL (http, https, ftp, file, ssh) at col.
// Returns end column if matched, or 0 if no match.
fn try_scheme_url(base: u32, col: u32, cols: u32) -> u32 {
    var scheme_end: u32 = 0u;

    // http:// or https://
    if (match_http(base, col, cols)) {
        scheme_end = col + 4u;
        if (scheme_end < cols && cp_at(base, scheme_end) == 115u) { // 's'
            scheme_end += 1u;
        }
    }
    // ftp://
    else if (match_ftp(base, col, cols)) {
        scheme_end = col + 3u;
    }
    // file://
    else if (match_file(base, col, cols)) {
        scheme_end = col + 4u;
    }
    // ssh://
    else if (match_ssh(base, col, cols)) {
        scheme_end = col + 3u;
    }
    else {
        return 0u;
    }

    // Require "://" after scheme
    if (!match_scheme_sep(base, scheme_end, cols)) {
        return 0u;
    }

    var url_end = scheme_end + 3u; // past "://"

    // Scan forward until terminator
    while (url_end < cols && !is_url_terminator(cp_at(base, url_end))) {
        url_end += 1u;
    }

    // Strip trailing punctuation and unbalanced closers
    let min_end = scheme_end + 3u;
    url_end = strip_trailing(base, col, url_end);

    // Need at least 1 char after "://"
    if (url_end <= min_end) {
        return 0u;
    }

    return url_end;
}

// Check for bare email: local@domain.tld
// Scans backward for local part, forward for domain.
// Emits the result directly via emit_result and returns true, or returns false.
fn try_email(row: u32, base: u32, at_col: u32, cols: u32) -> bool {
    // Need chars before and after @
    if (at_col == 0u || at_col + 1u >= cols) { return false; }

    // Scan backward for local part (alnum, '.', '_', '-', '+')
    var local_start = at_col;
    while (local_start > 0u) {
        let c = cp_at(base, local_start - 1u);
        if (is_alnum(c) || c == 46u || c == 95u || c == 45u || c == 43u) {
            local_start -= 1u;
        } else {
            break;
        }
    }
    if (local_start == at_col) { return false; } // no local part

    // Scan forward for domain (alnum, '.', '-')
    var domain_end = at_col + 1u;
    var has_dot = false;
    while (domain_end < cols) {
        let c = cp_at(base, domain_end);
        if (is_alnum(c) || c == 45u) {
            domain_end += 1u;
        } else if (c == 46u) {
            has_dot = true;
            domain_end += 1u;
        } else {
            break;
        }
    }

    // Must have at least one dot in domain
    if (!has_dot) { return false; }
    // Domain must not end with dot or hyphen
    let last_domain = cp_at(base, domain_end - 1u);
    if (last_domain == 46u || last_domain == 45u) { return false; }

    emit_result(row, local_start, domain_end);
    return true;
}

@compute @workgroup_size(256)
fn detect_urls(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = params.row_start + gid.x;
    if (row >= params.row_end || row >= params.total_rows) {
        return;
    }

    let base = row * params.cols;

    var col: u32 = 0u;
    while (col < params.cols) {
        let cp = cp_at(base, col);

        // Check for mailto: (special — scheme without //)
        if (cp == 109u && match_mailto(base, col, params.cols)) {
            var end = col + 7u;
            while (end < params.cols && !is_url_terminator(cp_at(base, end))) {
                end += 1u;
            }
            end = strip_trailing(base, col, end);
            if (end > col + 7u) {
                emit_result(row, col, end);
                col = end;
                continue;
            }
        }

        // Check for scheme-based URLs (http, https, ftp, file, ssh)
        let scheme_end = try_scheme_url(base, col, params.cols);
        if (scheme_end > 0u) {
            emit_result(row, col, scheme_end);
            col = scheme_end;
            continue;
        }

        // Check for bare email at '@' sign
        if (cp == 64u) { // @
            if (try_email(row, base, col, params.cols)) {
                // try_email emitted the result; skip past the domain.
                // We need to re-scan forward to find domain_end for skip.
                var skip = col + 1u;
                while (skip < params.cols && !is_url_terminator(cp_at(base, skip))) {
                    skip += 1u;
                }
                col = skip;
                continue;
            }
        }

        col += 1u;
    }
}
