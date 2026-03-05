// Shared GPU cell struct — included by all compute shaders.
// Must match the Rust-side GpuCell in pkg/compute/src/types.rs.
struct GpuCell {
    codepoint: u32,
    fg_packed: u32,
    bg_packed: u32,
    flags: u32,
    row: u32,
    col: u32,
};
