# SPEC-2: GPU Optimization & Compute Acceleration

## Background

Marauder is a GPU-accelerated terminal emulator built on Rust + wgpu + Tauri. The rendering pipeline (instanced rendering at 120fps) and compute engine (4 GPU compute shaders) are production-ready. However, several specified capabilities remain unimplemented, compute results aren't wired into the live UI, and performance hasn't been hardened for extreme workloads (10M+ line scrollback, high-bandwidth PTY output).

**Goal**: Make Marauder the fastest terminal emulator by fully leveraging GPU compute and rendering capabilities across all target platforms.

### What exists today
- Instanced rendering: bg + text + cursor + selection + overlays at 120fps active / 30fps idle
- 4 compute shaders: text search (exact match), URL detection, semantic highlighting, selection extraction
- Zero-copy device sharing between renderer and compute engine
- Adaptive frame rate, dirty tracking, DPI scaling
- Custom overlay pipeline compilation from extension WGSL

### What's missing
- Compute shaders built but not wired into live UI pipeline
- No regex search (GPU does exact substring only)
- No ligature rendering (HarfBuzz not integrated)
- No sixel/iTerm2 image protocol
- No subpixel antialiasing
- No disk-backed scrollback for unlimited history
- No GPU memory profiling or frame profiler overlay
- FFI bindings for compute engine not implemented
- CJK wide character rendering incomplete

### Why now
The rendering pipeline is proven (text and cursor are visible, compositing works). This is the right time to optimize before adding more features on top of an unoptimized foundation.

### Key assumptions
- Apple Silicon (M1+) is the primary development target; Vulkan/DX12 are supported via wgpu abstraction
- Users expect sub-millisecond search across large scrollback
- Terminal power users use code fonts with ligatures (Fira Code, JetBrains Mono)
- Disk-backed scrollback is acceptable for cold data; hot data stays in GPU memory

---

## Requirements

### Functional requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| F1 | GPU regex search across entire scrollback with <2ms latency for hot data | Must |
| F2 | Real-time GPU overlays for search matches, URL highlights, and semantic coloring | Must |
| F3 | Async notification of compute results to Deno/webview for interactive UI (search panel, URL tooltips) | Must |
| F4 | Ligature rendering via HarfBuzz font shaping for code fonts | Must |
| F5 | Sixel and iTerm2 inline image protocol rendering as GPU texture overlays | Must |
| F6 | Subpixel antialiasing (RGB/BGR) for LCD displays | Must |
| F7 | CJK wide character rendering with correct 2-cell width | Must |
| F8 | Disk-backed scrollback with memory-mapped file for unlimited history | Must |
| F9 | GPU-accelerated diff computation between command outputs | Should |
| F10 | User-configurable semantic highlight rules consumed by GPU classifier | Should |
| F11 | Color emoji rendering via separate Rgba8Unorm atlas | Should |
| F12 | FFI bindings for compute engine (Deno standalone mode) | Must |
| F13 | Built-in frame profiler overlay (toggle via hotkey) | Must |

### Non-functional requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| NF1 | 240fps sustained on Apple Silicon with full-screen terminal (250x80) | Must |
| NF2 | <0.5ms CPU time per frame (instance build + command encoding) | Must |
| NF3 | <2ms GPU search across 100K lines of hot scrollback | Must |
| NF4 | <100ms search across 1M lines of cold (disk-backed) scrollback | Must |
| NF5 | <500ms search across 10M lines with progressive streaming results | Must |
| NF6 | <1s search for any scrollback size with progressive results | Must |
| NF7 | Zero dropped frames during `cat large_file` (high-bandwidth PTY output) | Must |
| NF8 | <5MB GPU memory for glyph atlas (standard ASCII + common Unicode) | Should |
| NF9 | <100ms first frame (atlas pre-warm with ASCII + extended ranges) | Must |
| NF10 | Graceful degradation on integrated Intel/AMD GPUs (lower FPS target, same features) | Must |

### Security and compliance requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| S1 | GPU compute shaders must not leak data between PTY sessions | Must |
| S2 | Disk-backed scrollback files must be user-readable only (0600 permissions) | Must |
| S3 | Memory-mapped scrollback must be unmapped and zeroed on session close | Should |

### Data requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| D1 | Scrollback stored as GpuCell structs (24 bytes each) in GPU storage buffer | Must |
| D2 | Cold scrollback stored as memory-mapped file in `$XDG_CACHE_HOME/marauder/scrollback/` | Must |
| D3 | Glyph atlas cached per (font_family, font_size, dpi) tuple | Should |
| D4 | Compute result buffers capped at 64K entries (MAX_RESULT_CAP) | Must |

### Integration requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| I1 | Compute results feed into renderer overlay pipeline (zero CPU round-trip for visual feedback) | Must |
| I2 | Compute results also forwarded to event bus for Deno/webview consumption | Must |
| I3 | HarfBuzz integrated as workspace dependency for font shaping | Must |
| I4 | Sixel decoder integrated (parse sixel escape sequences into pixel buffers) | Must |
| I5 | `ffi/compute/mod.ts` exposes all compute operations to Deno FFI mode | Must |

### Operational requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| O1 | Frame profiler shows: frame time, GPU time, instance counts, atlas usage, buffer sizes | Must |
| O2 | Tracing integration with wgpu spans for external profilers (Tracy, chrome://tracing) | Must |
| O3 | GPU memory tracking: report buffer + texture allocations per frame | Must |
| O4 | Benchmark suite runs in CI: frame time, search latency, scrollback throughput, atlas rebuild | Must |
| O5 | Visual regression tests for rendering correctness (ligatures, emoji, CJK, images) | Must |
| O6 | Stress tests: /dev/urandom throughput, 10M scrollback, rapid resize, concurrent search | Must |

### Delivery constraints

| ID | Constraint |
|----|-----------|
| DC1 | Timeline: 6-8 weeks (3 phases) |
| DC2 | Broad GPU compatibility: Metal + Vulkan + DX12 + integrated Intel/AMD |
| DC3 | Each optimization behind a feature flag for user override |
| DC4 | No breaking changes to existing renderer/compute C ABI |
| DC5 | Phased release: rendering opts -> compute integration -> new capabilities |

---

## Method

### 1. System architecture overview

```
                    ┌─────────────────────────────────────────────┐
                    │              Tauri Webview                   │
                    │  (search panel, URL tooltip, profiler HUD)  │
                    └──────────────┬──────────────────────────────┘
                                   │ async events
                    ┌──────────────▼──────────────────────────────┐
                    │           Event Bus (pub/sub)                │
                    └──────┬───────────────────┬──────────────────┘
                           │                   │
              ┌────────────▼─────┐   ┌────────▼──────────────────┐
              │   Renderer       │   │   Compute Engine           │
              │  (instanced      │◄──┤  (search, URL, highlight,  │
              │   draw calls)    │   │   selection, diff, regex)   │
              └────────┬─────────┘   └────────┬──────────────────┘
                       │                      │
              ┌────────▼──────────────────────▼──────────────────┐
              │          Shared wgpu Device + Queue              │
              │                                                   │
              │  ┌─────────────┐  ┌──────────┐  ┌─────────────┐ │
              │  │ Cell Buffer │  │ Atlas    │  │ Image       │ │
              │  │ STORAGE|    │  │ Textures │  │ Textures    │ │
              │  │ VERTEX      │  │ R8/RGBA  │  │ (sixel/     │ │
              │  │             │  │          │  │  iTerm2)    │ │
              │  └─────────────┘  └──────────┘  └─────────────┘ │
              └──────────────────────┬───────────────────────────┘
                                     │
              ┌──────────────────────▼───────────────────────────┐
              │         Disk-Backed Scrollback (mmap)            │
              │  Hot: GPU buffer (visible + recent)              │
              │  Warm: mmap'd file (searchable, <100ms)          │
              │  Cold: compressed archive (>10M lines, <1s)      │
              └─────────────────────────────────────────────────┘
```

### 2. Architectural style and rationale

**GPU-first architecture**: Every operation that can run in parallel runs on the GPU. The CPU's role is limited to:
- Building instance buffers from dirty grid rows (<0.5ms)
- Encoding wgpu command buffers
- Handling I/O (PTY read, disk, network)
- Policy decisions (Deno runtime)

**Zero-copy pipeline**: The cell buffer is `STORAGE | VERTEX` — the renderer reads it for drawing, the compute engine reads it for search/highlight. No data is copied between them.

**Tiered scrollback**: Hot data in GPU memory, warm data in mmap'd files, cold data in compressed archives. Search progressively streams results from each tier.

### 3. Component responsibilities

| Component | Responsibility |
|-----------|---------------|
| `pkg/renderer` | Frame rendering: bg + text + cursor + selection + overlays + images. Glyph atlas management. Instance buffer lifecycle. Frame profiler overlay. |
| `pkg/compute` | GPU compute dispatch: regex search, URL detection, semantic highlighting, selection extraction, diff computation. Result readback and event publication. |
| `pkg/grid` | Cell storage, dirty tracking, scrollback management. Disk-backed tier management. |
| `pkg/parser` | VT/ANSI parsing including sixel and iTerm2 OSC sequences. |
| `ffi/compute` | Deno FFI bindings for all compute operations. |
| Event Bus | Forward compute results to both renderer overlays and Deno/webview. |

### 4. Data design and schema model

#### GpuCell (24 bytes, repr(C), bytemuck Pod)
```rust
struct GpuCell {
    codepoint: u32,      // Unicode codepoint
    fg_packed: u32,      // RGBA packed
    bg_packed: u32,      // RGBA packed
    flags: u32,          // bold|italic|underline|strikethrough|blink|dim|inverse|wide
    row: u32,            // absolute row index
    col: u32,            // column index
}
```

#### Disk-backed scrollback file format
```
Header (64 bytes):
  magic: [u8; 4] = "MSCR"
  version: u32 = 1
  cell_size: u32 = 24
  total_rows: u64
  cols: u32
  _reserved: [u8; 40]

Body:
  [GpuCell; total_rows * cols]  // flat array, mmap'd
```

#### Glyph atlas entry
```rust
struct GlyphEntry {
    font_id: u32,
    glyph_id: u32,
    size: f32,
    uv: [f32; 4],      // u, v, width, height in atlas
    bearing: [f32; 2],  // x, y offset from baseline
    advance: f32,       // horizontal advance
    is_ligature: bool,  // multi-codepoint glyph
    cell_span: u8,      // number of grid cells this glyph covers
}
```

### 5. API and interface design

#### Compute C ABI additions
```rust
// Regex search (compiles NFA to GPU state machine)
compute_regex_search(handle, pattern_utf8, pattern_len, flags, results_buf, max_results) -> u32

// Diff computation
compute_diff(handle, buf_a, len_a, buf_b, len_b, results_buf, max_results) -> u32

// Progressive search (returns immediately, results stream via callback)
compute_search_async(handle, pattern, len, callback, user_data) -> u64  // returns search_id
compute_search_cancel(handle, search_id)
```

#### Renderer additions
```rust
// Image rendering
renderer_add_image(handle, row, col, width_cells, height_cells, pixel_data, pixel_len, format) -> u32
renderer_remove_image(handle, image_id)

// Profiler overlay
renderer_toggle_profiler(handle)
renderer_get_frame_stats(handle) -> FrameStats

// Font shaping
renderer_set_font_features(handle, features_json, len)  // e.g., {"liga": true, "calt": true}
```

### 6. Workflow and sequence logic

#### GPU regex search flow
```
1. User types search pattern
2. Deno validates pattern, sends to compute engine
3. Compute engine compiles regex to NFA transition table
4. Upload NFA table as GPU uniform buffer
5. Dispatch compute shader: each workgroup processes a row
   - Each thread walks the NFA, consuming cell codepoints
   - Match positions written to atomic output buffer
6. Two-phase readback: count → data
7. Results published to:
   a. Renderer overlay (highlight matches in-place, <1 frame latency)
   b. Event bus → Deno → webview search panel (match count, navigation)
8. For cold scrollback: mmap file → batch upload to GPU → dispatch → readback
9. Progressive results stream to UI as each batch completes
```

#### Sixel image rendering flow
```
1. PTY emits DCS sixel data
2. Parser detects sixel start (DCS P1;P2;P3 q)
3. Parser accumulates sixel data until ST (string terminator)
4. Sixel decoder converts to RGBA pixel buffer
5. Renderer uploads as wgpu::Texture (Rgba8UnormSrgb)
6. Image rendered as textured quad in overlay pass
7. Grid cells under the image marked as "image-occupied" (not drawn by text pass)
8. On scroll: image quad position updated; on scroll-out: texture retained in cache
```

#### Ligature rendering flow
```
1. Grid provides a row of codepoints to renderer
2. Renderer feeds codepoints to HarfBuzz shaper
3. HarfBuzz returns glyph IDs + positions (may merge multiple codepoints into 1 glyph)
4. For each shaped glyph:
   a. Check atlas cache for (font_id, glyph_id, size)
   b. If miss: rasterize via cosmic-text, pack into atlas
   c. Build text instance with correct UV, bearing, and cell_span
5. Ligature glyphs span multiple cells (cell_span > 1)
6. Instance quad width = cell_width * cell_span
```

### 7. Algorithms and business rules

#### GPU regex engine (NFA on GPU)
- Compile regex to Thompson NFA (no backtracking)
- NFA states stored as transition table in uniform buffer
- Each GPU thread maintains NFA state set per position
- Supports: `.`, `*`, `+`, `?`, `|`, `[]`, `^`, `$`, `\d`, `\w`, `\s`
- Does NOT support: backreferences, lookahead/lookbehind (inherently sequential)
- Fallback: patterns with unsupported features run on CPU via `regex` crate

#### Disk-backed scrollback tiers
- **Hot** (GPU buffer): visible rows + 10K recent rows. Always in GPU memory.
- **Warm** (mmap): 10K - 1M rows. Memory-mapped file, pages loaded on demand by OS.
- **Cold** (compressed): >1M rows. LZ4-compressed blocks, decompressed on search.
- Tier transitions are automatic based on scrollback growth.
- Search dispatches to all tiers in parallel; results merged by row index.

#### Subpixel antialiasing
- Atlas format changes from R8Unorm to Rgba8Unorm (4x memory)
- Each pixel stores RGB coverage values (not just alpha)
- Fragment shader: `output.rgb = glyph_rgb * fg_color.rgb; output.a = max(glyph_rgb.r, glyph_rgb.g, glyph_rgb.b)`
- Platform detection: enable on macOS (always) and Windows (if ClearType enabled), disable on Linux (most use grayscale)
- User can force on/off via config

### 8. Consistency and transaction strategy

- Scrollback writes are append-only (no conflicts)
- Disk file grown in 64KB chunks (aligned for mmap)
- GPU buffer resize uses double-buffering: new buffer allocated, data copied, old buffer freed next frame
- Compute dispatches are sequenced: search cancels previous search if new pattern arrives

### 9. Security architecture

- Disk scrollback files: `0600` permissions, stored in user cache directory
- mmap regions unmapped on session close; file optionally zeroed if `secure_scrollback = true` in config
- GPU buffers not explicitly zeroed (GPU memory management is opaque), but buffer contents are overwritten on reuse
- Compute shaders cannot access host memory — wgpu sandboxes all GPU operations

### 10. Reliability and resilience design

- GPU device lost: recreate device + all pipelines + re-upload atlas + re-upload cell buffer. 1-frame stutter.
- Compute shader timeout (>5s): cancel dispatch, fall back to CPU implementation
- Disk scrollback I/O error: degrade to GPU-only scrollback, warn user
- Atlas overflow (>4096x4096): evict least-recently-used glyph pages
- Out of GPU memory: reduce scrollback hot tier, disable subpixel AA, warn user

### 11. Performance and scalability approach

#### Rendering optimizations
| Optimization | Technique | Expected Impact |
|-------------|-----------|-----------------|
| Ring-buffer instances | Persistent mapped buffer with write pointer | Eliminate per-frame buffer allocation |
| Partial dirty upload | Only write dirty row slices, not full buffer | -50% upload bandwidth |
| Async compute overlap | Dispatch compute while previous frame presents | +10-20% throughput |
| Atlas LRU eviction | Evict unused glyph pages instead of rebuilding | Eliminate atlas stalls |
| Pre-warm extended Unicode | Load CJK, emoji, symbols at startup | Eliminate runtime atlas stalls |
| Double-buffered instances | Two instance buffers alternated per frame | Eliminate GPU stall on upload |

#### Compute optimizations
| Optimization | Technique | Expected Impact |
|-------------|-----------|-----------------|
| Batch PTY output | Accumulate bytes for 1ms before parsing | Reduce grid lock contention |
| Coalesced GPU reads | Ensure sequential memory access in shaders | +30% compute throughput |
| Shared memory prefix | Load search pattern into workgroup shared memory | -50% global memory reads |
| Persistent threads | Keep compute threads alive across dispatches | Eliminate dispatch overhead |
| Progressive search | Stream results from GPU as batches complete | <100ms to first result on 10M lines |

### 12. Observability design

#### Frame profiler overlay (toggle via Ctrl+Shift+P or config)
```
┌─────────────────────────────────┐
│ Frame: 16384  FPS: 238/240      │
│ CPU: 0.31ms  GPU: 0.42ms       │
│ Instances: bg=7520 txt=1893     │
│ Atlas: 847/4096 glyphs (21%)   │
│ GPU Mem: 12.4MB buf + 4.1MB tex│
│ Scrollback: 48K hot / 1.2M warm│
│ Search: idle                    │
└─────────────────────────────────┘
```

#### Tracing spans
- `renderer::build_instances` — CPU time building instance data
- `renderer::upload` — GPU buffer write time
- `renderer::encode` — command encoder time
- `renderer::present` — surface present time
- `compute::dispatch` — compute shader execution
- `compute::readback` — GPU→CPU result transfer
- `atlas::rasterize` — glyph rasterization time
- `scrollback::mmap_read` — disk scrollback access

#### GPU memory report (logged every 10s at debug level)
- Cell buffer: N bytes (rows * cols * 24)
- Instance buffers: N bytes (bg + text + selection)
- Atlas textures: N bytes (count * width * height * bpp)
- Image textures: N bytes (sixel/iTerm2 cached images)
- Uniform buffers: N bytes
- Total GPU allocation: N bytes

### 13. Infrastructure and deployment topology

Single binary (Tauri bundle). All GPU resources created at runtime. No external GPU libraries beyond wgpu (which bundles Metal/Vulkan/DX12 backends).

**Build matrix:**
- macOS: Metal backend, universal binary (x86_64 + aarch64)
- Linux: Vulkan backend (requires vulkan-loader)
- Windows: DX12 backend (fallback to Vulkan if DX12 unavailable)

**Feature flags (runtime config):**
```toml
[gpu]
subpixel_aa = true          # RGB subpixel antialiasing
ligatures = true            # HarfBuzz font shaping
sixel = true                # Sixel image protocol
iterm2_images = true        # iTerm2 image protocol
gpu_regex = true            # GPU regex (false = CPU regex fallback)
frame_profiler = false      # Built-in profiler overlay
max_atlas_size = 4096       # Maximum atlas texture dimension
scrollback_hot_rows = 10000 # Rows kept in GPU memory
scrollback_disk = true      # Disk-backed scrollback
scrollback_compress = true  # LZ4 compression for cold scrollback
```

### 14. Tradeoffs and rejected alternatives

| Decision | Alternative | Reason for rejection |
|----------|-------------|---------------------|
| Thompson NFA for GPU regex | DFA | DFA state table too large for GPU uniform buffer; NFA is O(n*m) but parallelizes well |
| HarfBuzz for shaping | rusttype/ab_glyph | HarfBuzz is the only correct implementation for OpenType features and ligatures |
| LZ4 for cold scrollback | zstd | LZ4 decompression is faster (critical for search latency); compression ratio acceptable for text |
| Memory-mapped scrollback | SQLite | mmap is zero-copy for sequential access; SQLite adds overhead for append-only workload |
| R8Unorm + Rgba8Unorm dual atlas | Single Rgba8Unorm | 4x memory overhead for all glyphs is wasteful; most glyphs are grayscale |
| wgpu compute shaders | CUDA/Metal compute | wgpu is cross-platform; CUDA is NVIDIA-only; Metal compute is macOS-only |
| Progressive search results | Wait for complete results | Users expect immediate feedback; streaming first results is better UX |

### 15. Architecture diagrams

TBD — Generate PlantUML diagrams for:
- Render pipeline (per-frame flow)
- Compute dispatch flow
- Scrollback tier management
- Atlas lifecycle

---

## Implementation

### Build phases

#### Phase 1: Rendering Optimizations (Weeks 1-2)
- Ring-buffer instance allocation
- Double-buffered instances
- Partial dirty row upload
- CJK wide character rendering
- HarfBuzz integration + ligature rendering
- Subpixel antialiasing (dual atlas mode)
- Frame profiler overlay
- GPU memory tracking

#### Phase 2: Compute Integration (Weeks 3-4)
- Wire compute results into renderer overlay pipeline
- Wire compute results into event bus for Deno/webview
- GPU regex engine (NFA compilation + compute shader)
- FFI bindings for compute (`ffi/compute/mod.ts`)
- Progressive search with streaming results
- Async compute overlap with rendering

#### Phase 3: New Capabilities (Weeks 5-6)
- Disk-backed scrollback (mmap + tiered storage)
- Cold scrollback search (batch upload + progressive results)
- Sixel image protocol (parser + decoder + texture rendering)
- iTerm2 image protocol
- Color emoji atlas
- GPU diff computation
- User-configurable highlight rules

#### Phase 4: Hardening (Weeks 7-8)
- Benchmark suite (CI automated)
- Visual regression tests
- Stress tests (throughput, scrollback, resize, concurrent ops)
- Performance tuning per platform (Metal, Vulkan, DX12)
- Documentation and profiling guides
- Feature flag testing matrix

### Workstreams

| Stream | Phase 1 | Phase 2 | Phase 3 | Phase 4 |
|--------|---------|---------|---------|---------|
| Renderer | Ring buffer, double buffer, ligatures, subpixel, profiler | Overlay integration | Sixel/iTerm2, emoji | Perf tuning |
| Compute | — | Regex engine, FFI, progressive search | Diff, custom highlights | Stress tests |
| Grid/Scrollback | CJK width | — | Disk-backed, mmap, tiers | Benchmark suite |
| Testing | Unit tests per feature | Integration tests | Visual regression | Full matrix |

### Dependencies

```
HarfBuzz integration ──► Ligature rendering
                     ──► Correct CJK shaping

Disk-backed scrollback ──► Cold search
                       ──► Unlimited history

GPU regex engine ──► Progressive search
                ──► Compute-to-overlay pipeline

FFI bindings ──► Standalone Deno compute access
```

### Testing strategy

**Benchmarks (automated, CI):**
- `bench_frame_time`: Measure frame time at 250x80 grid with varying text content
- `bench_search_hot`: Search 100K rows for pattern, measure latency
- `bench_search_cold`: Search 1M mmap'd rows, measure time-to-first-result
- `bench_scrollback_write`: Measure throughput of scrollback append (MB/s)
- `bench_atlas_rebuild`: Measure time to rasterize 500 new glyphs
- `bench_pty_throughput`: Measure frames dropped during `cat` of 100MB file

**Visual regression:**
- Golden screenshot comparison for: ASCII, ligatures, CJK, emoji, sixel images, subpixel AA
- Test at 1x and 2x DPI
- Test all cursor styles (block, underline, bar)

**Stress tests:**
- `stress_urandom`: `cat /dev/urandom | head -c 100M` — no frame drops
- `stress_scrollback_10M`: Fill 10M lines, search, scroll — responsive
- `stress_rapid_resize`: Resize window 100 times in 1 second — no crash
- `stress_concurrent_search`: 5 concurrent search patterns — results correct

### Rollout strategy

**Phased release with feature flags:**

1. **Alpha (internal)**: All features behind flags, default OFF
2. **Beta Phase 1**: Rendering optimizations ON by default, compute behind flag
3. **Beta Phase 2**: Compute integration ON by default, new capabilities behind flag
4. **RC**: All features ON by default, flags remain for user override
5. **Stable**: Feature flags documented, defaults tuned per platform

**Graceful degradation:**
- Integrated GPU: disable subpixel AA, reduce atlas size, lower hot scrollback tier
- Old GPU (no compute shader support): CPU fallback for search/highlight
- Low memory: reduce scrollback, smaller atlas, disable image caching

### Operational readiness

- [ ] Frame profiler tested on all 3 backends (Metal, Vulkan, DX12)
- [ ] GPU memory tracking reports accurate numbers
- [ ] Tracing spans visible in Tracy/chrome://tracing
- [ ] Benchmark suite passes on CI for all platforms
- [ ] Visual regression suite has golden images for all test cases
- [ ] Stress tests pass without crash or memory leak
- [ ] Feature flags all functional (enable/disable verified)
- [ ] Documentation: GPU config options, profiling guide, troubleshooting

---

## Milestones

| Milestone | Exit Criteria | Target |
|-----------|--------------|--------|
| M1: Rendering Optimized | 240fps on M-series, ring buffer, ligatures, subpixel AA, profiler overlay | Week 2 |
| M2: Compute Integrated | Search results in overlay + webview, GPU regex, FFI bindings, progressive search | Week 4 |
| M3: Full Capabilities | Disk-backed scrollback, sixel/iTerm2, emoji, diff, custom highlights | Week 6 |
| M4: Hardened | All benchmarks pass, visual regression green, stress tests pass, docs complete | Week 8 |

---

## Gathering Results

### Success metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Frame rate (active) | >= 240fps on Apple Silicon, >= 120fps on integrated | `bench_frame_time` |
| CPU per frame | < 0.5ms | `bench_frame_time` |
| Search (100K hot) | < 2ms | `bench_search_hot` |
| Search (1M warm) | < 100ms | `bench_search_cold` |
| Search (10M cold) | < 500ms to first result | `bench_search_cold` |
| PTY throughput | 0 dropped frames at 100MB/s | `bench_pty_throughput` |
| Atlas rebuild | < 5ms for 100 new glyphs | `bench_atlas_rebuild` |
| GPU memory (standard) | < 20MB total | `renderer_get_gpu_memory_report` |

### Validation methods

- **Automated benchmarks**: Run on every PR, block merge if regression > 5%
- **Visual regression**: Screenshot diff on every PR, manual review if delta > 0
- **Stress test suite**: Nightly run on CI, alert on failure
- **User feedback**: Beta testers report perceived performance and rendering quality
- **Profiler audit**: Monthly review of frame profiler data from beta users (opt-in)

### Post-production review cadence

- **Week 1 post-launch**: Check crash reports, GPU compatibility issues, benchmark drift
- **Week 4 post-launch**: Analyze user telemetry (if opted in) for performance patterns
- **Quarterly**: Review GPU vendor updates, wgpu version upgrades, new optimization opportunities

### Remediation triggers

- Frame rate drops below 120fps on Apple Silicon → P0 investigation
- Search latency exceeds 10ms on 100K rows → P0 investigation
- Crash rate > 0.1% on any platform → P0 hotfix
- GPU memory exceeds 100MB → P1 investigation
- Visual regression test fails → P1 before next release

---

## Appendices

### A. Glossary

| Term | Definition |
|------|-----------|
| **Instance buffer** | GPU vertex buffer containing per-cell data (position, color, UV) for instanced draw calls |
| **Glyph atlas** | GPU texture containing pre-rasterized character bitmaps, indexed by UV coordinates |
| **GpuCell** | 24-byte struct representing one terminal cell in GPU memory |
| **Hot scrollback** | Recent rows kept in GPU storage buffer for instant access |
| **Warm scrollback** | Older rows in memory-mapped file, paged by OS on demand |
| **Cold scrollback** | Archived rows compressed with LZ4, decompressed for search |
| **NFA** | Non-deterministic Finite Automaton — regex execution model suitable for GPU parallelism |
| **Sixel** | DEC graphics protocol for inline terminal images (6-pixel-high strips) |
| **Subpixel AA** | Antialiasing that exploits LCD subpixel layout (RGB/BGR) for higher effective resolution |

### B. Risk register

| Risk | Impact | Likelihood | Mitigation |
|------|--------|-----------|------------|
| HarfBuzz adds 2MB+ to binary size | Medium | High | Accept — correctness requires it; strip debug symbols |
| GPU regex NFA too slow for complex patterns | High | Medium | CPU fallback for patterns exceeding complexity threshold |
| Subpixel AA looks wrong on some displays | Medium | Medium | Auto-detect display type; user override in config |
| Disk scrollback I/O blocks render thread | High | Low | Scrollback I/O on dedicated thread; never block renderer |
| wgpu breaking changes in future versions | Medium | Medium | Pin wgpu version; upgrade on schedule with testing |
| Integrated GPUs can't sustain 240fps | Low | High | Graceful degradation; 120fps target for integrated |

### C. Decision log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-03-07 | Use Thompson NFA for GPU regex | DFA state explosion makes it impractical for GPU uniform buffers |
| 2026-03-07 | Three-tier scrollback (hot/warm/cold) | Balances GPU memory usage with search performance |
| 2026-03-07 | HarfBuzz for font shaping | Only correct implementation for OpenType features |
| 2026-03-07 | Feature flags for all optimizations | Users with GPU issues can selectively disable |
| 2026-03-07 | Progressive search results | Better UX than waiting for complete results on large scrollback |
