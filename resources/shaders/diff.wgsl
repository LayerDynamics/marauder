// GPU diff shader: computes FNV-1a hash per row for two cell snapshots.
//
// Each workgroup thread processes one row, hashing all codepoints.
// The CPU then runs Myers-diff DP on the much smaller hash vectors.

struct GpuCell {
    codepoint: u32,
    fg_packed: u32,
    bg_packed: u32,
    flags: u32,
    row: u32,
    col: u32,
};

struct DiffParams {
    rows_a: u32,
    rows_b: u32,
    cols: u32,
    _pad: u32,
};

@group(0) @binding(0) var<storage, read> cells_a: array<GpuCell>;
@group(0) @binding(1) var<storage, read> cells_b: array<GpuCell>;
@group(0) @binding(2) var<uniform> params: DiffParams;
@group(0) @binding(3) var<storage, read_write> hashes_a: array<u32>;
@group(0) @binding(4) var<storage, read_write> hashes_b: array<u32>;

// FNV-1a constants (32-bit)
const FNV_OFFSET: u32 = 2166136261u;
const FNV_PRIME: u32 = 16777619u;

// Hash all 4 bytes of a u32 into an FNV-1a accumulator.
fn fnv_hash_u32(h: u32, val: u32) -> u32 {
    var hash = h;
    hash = hash ^ (val & 0xFFu);
    hash = hash * FNV_PRIME;
    hash = hash ^ ((val >> 8u) & 0xFFu);
    hash = hash * FNV_PRIME;
    hash = hash ^ ((val >> 16u) & 0xFFu);
    hash = hash * FNV_PRIME;
    hash = hash ^ ((val >> 24u) & 0xFFu);
    hash = hash * FNV_PRIME;
    return hash;
}

@compute @workgroup_size(256)
fn hash_rows(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;

    // Hash rows in snapshot A
    if (row < params.rows_a) {
        var hash: u32 = FNV_OFFSET;
        let row_start = row * params.cols;
        for (var col: u32 = 0u; col < params.cols; col++) {
            let idx = row_start + col;
            hash = fnv_hash_u32(hash, cells_a[idx].codepoint);
            hash = fnv_hash_u32(hash, cells_a[idx].fg_packed);
            hash = fnv_hash_u32(hash, cells_a[idx].bg_packed);
            hash = fnv_hash_u32(hash, cells_a[idx].flags);
        }
        hashes_a[row] = hash;
    }

    // Hash rows in snapshot B
    if (row < params.rows_b) {
        var hash: u32 = FNV_OFFSET;
        let row_start = row * params.cols;
        for (var col: u32 = 0u; col < params.cols; col++) {
            let idx = row_start + col;
            hash = fnv_hash_u32(hash, cells_b[idx].codepoint);
            hash = fnv_hash_u32(hash, cells_b[idx].fg_packed);
            hash = fnv_hash_u32(hash, cells_b[idx].bg_packed);
            hash = fnv_hash_u32(hash, cells_b[idx].flags);
        }
        hashes_b[row] = hash;
    }
}
