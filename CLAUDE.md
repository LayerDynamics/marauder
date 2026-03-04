# Marauder — Implementation Guide

## Project Overview

Marauder is a GPU-accelerated, fully extensible terminal emulator built with Rust + Deno + Tauri.

- **Rust** (`pkg/`): Native performance crates — PTY, VT parser, grid, wgpu renderer, event bus, runtime, daemon, IPC, config store. Each crate builds as `rlib` + `cdylib`.
- **Deno** (`lib/` + `ffi/`): Runtime orchestration layer — shell engine, UI logic, I/O pipeline, dev tools, and FFI bindings wrapping every Rust cdylib.
- **Tauri** (`apps/marauder/`): App shell with Vite-powered webview for UI chrome (tabs, status bar, command palette). wgpu renders the terminal grid behind the transparent webview.
- **Extensions** (`extensions/`): TypeScript packages with first-class access to the runtime.

Read `docs/Overview.md`, `docs/Architecture.md`, `docs/Development.md`, `docs/Roadmap.md` for full details.

## Critical Rules

- If something is called but missing, it should be **implemented**, not removed
- Unused variables, methods, or imports are ALWAYS intentional — use them appropriately as they are critical to operations
- You are not on the production server — all commands need to be provided
- Never delete code that appears unused without understanding its purpose; it likely has a planned consumer

## Architecture — The Three-Layer Model

```
Layer 3: Tauri Webview  (apps/marauder/src/)     — UI chrome only
Layer 2: Deno Runtime   (lib/ + ffi/)            — orchestration, policy, extensions
Layer 1: Rust Native    (pkg/ + apps/src-tauri/)  — performance primitives, hot path
```

**THE HOT PATH STAYS IN RUST**: PTY read → VT parse → grid update → wgpu render. This path NEVER crosses into Deno or the webview. Deno is notified asynchronously via the event bus but never blocks rendering.

**GPU ACCELERATION IS A REQUIREMENT, NOT AN OPTIMIZATION**: The GPU handles both rendering AND computation. `pkg/renderer` draws every frame via wgpu instanced rendering (120fps, <1ms CPU/frame). `pkg/compute` runs wgpu compute shaders for text search, URL detection, pattern highlighting, and selection extraction — all on the same GPU device, reading the same cell buffer with zero-copy. No CPU-side glyph blitting, no software rasterization, no CPU text search when the GPU can do it in parallel.

**Deno makes all policy decisions**: keybindings, config resolution, shell integration, extension loading, pane/tab management. Rust provides primitives; Deno composes them.

**The webview renders chrome only**: tabs, status bar, command palette, settings, extension UI. The terminal grid area is transparent — wgpu renders underneath.

## Directory Structure

```
apps/marauder/src/            → Frontend (Vite + TS + HTML/CSS for webview)
apps/marauder/src-tauri/      → Tauri Rust backend (commands, deno_core embed, wgpu)
apps/marauder-server/         → Headless multiplexer daemon (Rust)
pkg/pty/                      → PTY management (portable-pty wrapper)
pkg/parser/                   → VT/ANSI parser (vte-based)
pkg/grid/                     → Terminal cell grid + scrollback + dirty tracking
pkg/renderer/                 → GPU renderer (wgpu + cosmic-text)
pkg/compute/                  → GPU compute pipelines (search, URL detect, highlights)
pkg/runtime/                  → Core runtime lifecycle
pkg/event-bus/                → Typed pub/sub event system
pkg/config-store/             → TOML/JSON config read/write/watch
pkg/ipc/                      → Unix socket transport (multiplexer)
pkg/daemon/                   → Background process management
ffi/pty/                      → Deno FFI binding for pkg/pty
ffi/parser/                   → Deno FFI binding for pkg/parser
ffi/grid/                     → Deno FFI binding for pkg/grid
ffi/renderer/                 → Deno FFI binding for pkg/renderer
ffi/compute/                  → Deno FFI binding for pkg/compute
ffi/event-bus/                → Deno FFI binding for pkg/event-bus
ffi/config-store/             → Deno FFI binding for pkg/config-store
lib/shell/                    → Shell engine (zones, history, completions)
lib/ui/                       → UI logic (panes, tabs, layout engine)
lib/io/                       → I/O pipeline, stream management
lib/dev/                      → Development tools, debugging
extensions/theme-default/     → Bundled color schemes
extensions/status-bar/        → Status bar extension
extensions/git-integration/   → Git status/branch display
extensions/search/            → In-terminal search
extensions/notifications/     → Desktop notifications
resources/shaders/            → WGSL shader files
resources/fonts/              → Bundled fallback fonts
resources/shell-integrations/ → Shell RC snippets (zsh, bash, fish)
bin/install.sh                → Install script
bin/marauder.sh               → Launch script
bin/uninstall.sh              → Uninstall script
```

## Implementation Order (Phase 1 — Foundation)

Follow this exact order. Each step depends on the previous.

### Step 1: Cargo Workspace Setup

Set up `Cargo.toml` at root as a workspace. Every `pkg/*` crate must have `crate-type = ["rlib", "cdylib"]`.

```toml
[workspace]
resolver = "2"
members = [
  "apps/marauder/src-tauri",
  "pkg/event-bus",
  "pkg/pty",
  "pkg/parser",
  "pkg/grid",
  "pkg/renderer",
  "pkg/compute",
  "pkg/runtime",
  "pkg/config-store",
  "pkg/ipc",
  "pkg/daemon",
]
```

Key workspace deps: `portable-pty = "0.9"`, `vte = "0.15"`, `wgpu = "24.0"`, `cosmic-text = "0.12"`, `tauri = "2"`, `deno_core = "0.311"`, `tokio = { version = "1", features = ["full"] }`, `serde`, `serde_json`, `toml`, `tracing`, `anyhow`, `thiserror`, `notify = "7"`, `raw-window-handle = "0.6"`.

### Step 2: `pkg/event-bus`

Build first — everything depends on it.

- Typed event enum covering all layers (input, pty, parser, grid, shell, render, config)
- `HashMap<EventType, Vec<Subscriber>>` pub/sub
- Interceptor support (priority-ordered, can modify/suppress events)
- C ABI exports: `event_bus_create`, `event_bus_subscribe`, `event_bus_publish`, `event_bus_intercept`, `event_bus_destroy`
- Bridge structs for forwarding to Deno and Tauri webview (async, non-blocking)
- Events serialized as bytes (serde_json) across FFI boundary

### Step 3: `pkg/pty`

- Wrap `portable-pty` with lifecycle management
- `PtyManager` struct holding `HashMap<PaneId, PtyPair>`
- Async reader thread per PTY (reads into channel)
- C ABI: `pty_create`, `pty_read`, `pty_write`, `pty_resize`, `pty_close`, `pty_get_pid`, `pty_wait`
- `#[op2]` ops for embedded deno_core mode
- `#[tauri::command]` for webview access
- Signal forwarding (SIGWINCH on resize)
- Config: shell path, env vars, working directory, rows/cols

### Step 4: `pkg/parser`

- Implement `vte::Perform` trait on a `MarauderPerformer` struct
- Convert VTE callbacks into a typed `TerminalAction` enum (~40+ variants)
- Feed function: `parser.feed(bytes, callback)` — callback receives each action
- C ABI: `parser_create`, `parser_feed` (with callback), `parser_reset`, `parser_destroy`
- `parser_feed` callback signature: `fn(action_type: u32, data: *const u8, len: usize, user_data: *mut c_void)`
- Action types: Print, Execute, CursorMove, SetColor, SetAttribute, Erase, Scroll, SetMode, OscDispatch, etc.

### Step 5: `pkg/grid`

- `Grid` struct: primary `Screen`, alternate `Screen`, `Cursor`, `TerminalModes`, `DirtyTracker`
- `Screen`: `Vec<Row>` where `Row` is `Vec<Cell>`
- `Cell`: char + fg Color + bg Color + CellAttributes (bitflags: bold, italic, underline, strikethrough, blink, dim, inverse) + optional HyperlinkId + CellWidth
- Scrollback: ring buffer of `Row`, configurable capacity
- `grid.apply_action(action)` — the core state mutation function
- Dirty tracking: per-row dirty bit, cleared after render reads
- Selection: start/end coordinates, get_selection_text()
- C ABI: `grid_create`, `grid_apply_action`, `grid_get_cell`, `grid_get_dirty_rows`, `grid_get_cursor`, `grid_resize`, `grid_scroll_viewport`, `grid_select`, `grid_get_selection_text`, `grid_clear_dirty`, `grid_destroy`

### Step 6: `pkg/renderer` — GPU-Accelerated Rendering

GPU acceleration is a **core requirement**, not an optimization. Every frame is rendered on the GPU. The CPU never draws glyphs or fills backgrounds — it only prepares vertex/instance buffers that the GPU consumes.

**Architecture:**

```text
CPU side (Rust):
  Grid dirty rows → build instance buffer (one instance per cell)
  Instance data: { row, col, glyph_index, fg_color, bg_color, attrs }
  Upload instance buffer to GPU (wgpu::Buffer, mapped write)

GPU side (WGSL shaders):
  Background pass: instanced quads, one per cell, colored by bg_color
  Text pass: instanced textured quads, sample glyph from atlas texture
  Overlay pass: cursor, selection, decorations (blended on top)
  → Single present() call
```

**Glyph atlas (GPU texture):**
- `cosmic-text` rasterizes glyphs on the CPU → grayscale bitmaps
- Pack into one or more 512×512 or 1024×1024 GPU textures (bin packing)
- Atlas stores UV coordinates per glyph — lookup by (font_id, glyph_id, size)
- Cache hit rate is critical: most terminal output uses <200 unique glyphs
- Atlas rebuilt only when font changes or new glyphs encountered
- GPU texture format: `R8Unorm` (single channel) for standard, `Rgba8Unorm` for color emoji

**Instanced rendering (the key to performance):**
- Do NOT draw one quad at a time. Use instanced draw calls.
- One draw call for ALL background cells. One draw call for ALL text glyphs.
- Instance buffer layout per cell:

```rust
#[repr(C)]
struct CellInstance {
    pos: [f32; 2],           // pixel position (x, y)
    size: [f32; 2],          // cell size (width, height)
    bg_color: [f32; 4],      // RGBA
    fg_color: [f32; 4],      // RGBA
    glyph_uv: [f32; 4],     // atlas UV (u, v, width, height)
    glyph_offset: [f32; 2],  // glyph bearing offset
    flags: u32,              // bold, italic, underline, strikethrough, cursor, selected
}
```

- Total cells = rows × cols (e.g., 80×24 = 1920 instances, 250×80 = 20000)
- At 120fps with 20K instances, this is trivial for any modern GPU

**Damage tracking (avoid full rebuilds):**
- Only rebuild instance data for dirty rows (from `grid.get_dirty_rows()`)
- Instance buffer is a persistent GPU buffer; dirty rows overwrite their slice
- Full rebuild only on resize or font change

**Render pipeline:**
1. `renderer.update_cells(grid)`: read dirty rows, update instance buffer regions
2. `renderer.render_frame()`:
   - Begin render pass (clear with background color)
   - Draw background instances (instanced quad, bg_color per instance)
   - Draw text instances (instanced textured quad, sample glyph atlas)
   - Draw overlays (cursor blink, selection highlight, extension layers)
   - End render pass, `surface.present()`

**WGSL shaders** (`resources/shaders/`):
- `background.wgsl`: vertex shader positions quad from instance pos/size, fragment shader outputs bg_color
- `text.wgsl`: vertex shader positions quad + glyph_offset, fragment shader samples atlas texture × fg_color
- `cursor.wgsl`: animated cursor (block/underline/bar), blink via uniform time
- `overlay.wgsl`: selection highlight, search match highlight, extension overlays

**wgpu setup:**
- Backend: auto-detect (Vulkan on Linux, Metal on macOS, DX12 on Windows)
- Surface format: `Bgra8UnormSrgb` (standard sRGB)
- Present mode: `Fifo` (vsync) or `Mailbox` (low-latency, may tear)
- Features: none required beyond defaults — keep compatibility broad
- DPI scaling: query `window.scale_factor()`, multiply cell size, resize surface

**Performance targets:**
- 120fps sustained with full-screen terminal (250×80 grid)
- < 1ms CPU time per frame (instance buffer update + command encoding)
- `cat large_file` should not drop frames — PTY read batching + dirty tracking
- First frame in < 100ms (atlas pre-warm with ASCII + common glyphs)

**C ABI:**
- `renderer_create(window_handle, config) → *mut RendererHandle`
- `renderer_set_font(handle, family, size, line_height)`
- `renderer_set_theme(handle, theme_json, len)` — theme is a JSON color map
- `renderer_update_cells(handle, grid_handle)` — reads dirty rows directly
- `renderer_render_frame(handle)` — full frame: bg + text + overlays + present
- `renderer_resize_surface(handle, width, height, scale_factor)`
- `renderer_add_overlay(handle, layer_id, config_json, len)`
- `renderer_remove_overlay(handle, layer_id)`
- `renderer_set_cursor_style(handle, style, blink)`
- `renderer_get_cell_size(handle) → (f32, f32)` — needed for grid resize calculations
- `renderer_destroy(handle)`

### Step 6b: `pkg/compute` — GPU Compute Acceleration

The GPU is not just for rendering — it's a massively parallel processor. `pkg/compute` provides wgpu compute shaders for runtime operations that benefit from parallelism. It shares the wgpu `Device` and `Queue` with `pkg/renderer`.

**What runs on the GPU (compute shaders):**

- **Text search** — Parallel regex/substring search across entire scrollback buffer. Each workgroup processes a row. Results written to a GPU buffer, read back to CPU. Searching 100K lines of scrollback completes in <1ms on GPU vs ~50ms on CPU.
- **URL/pattern detection** — Scan all visible + scrollback cells for URL patterns, email addresses, file paths, IP addresses. Runs every time new content arrives. Results feed into clickable link overlays.
- **Semantic highlighting** — Batch classify cell runs for syntax-like highlighting (numbers, paths, flags, operators) directly on GPU.
- **Selection operations** — Extract text from large selection regions across scrollback. GPU gathers cell chars into a contiguous buffer.
- **Diff computation** — When extensions need to compare command outputs or track changes, GPU-accelerated LCS/diff on cell buffers.
- **Cell transforms** — Batch attribute changes (dim all non-matching cells during search, highlight all instances of a pattern).

**Architecture:**

```text
pkg/compute shares wgpu Device + Queue with pkg/renderer
  → Compute pipelines run between frames or on demand
  → Input: grid cell buffer (already in GPU memory from renderer)
  → Output: result buffers (read back via map_async or consumed by render overlays)
```

**Key design: zero-copy grid access.** The renderer already uploads cell data to GPU buffers. Compute shaders read that same buffer — no redundant CPU→GPU transfer. The grid's cell storage buffer is a shared `wgpu::Buffer` with `STORAGE | VERTEX` usage flags.

```rust
// Shared GPU cell buffer (created by renderer, used by compute)
let cell_buffer = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("cell_storage"),
    size: max_cells * std::mem::size_of::<GpuCell>() as u64,
    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_SRC,
    mapped_at_creation: false,
});

// GpuCell — the cell representation in GPU memory (used by both renderer and compute)
#[repr(C)]
struct GpuCell {
    codepoint: u32,       // Unicode codepoint
    fg_color: [u8; 4],    // RGBA packed
    bg_color: [u8; 4],    // RGBA packed
    flags: u32,           // bold, italic, underline, etc.
    row: u32,             // row index (for compute addressing)
    col: u32,             // col index
}
```

**Compute shader examples (`resources/shaders/`):**

- `search.wgsl` — Each workgroup takes a row of cells, runs substring/regex match against a pattern uniform, writes match positions to an output buffer
- `url_detect.wgsl` — State machine per row scanning for URL patterns (http://, https://, file://, etc.)
- `highlight.wgsl` — Pattern classifier: reads cell runs, writes highlight category to an output buffer consumed by the overlay render pass
- `selection_extract.wgsl` — Parallel gather of codepoints from a row range into a contiguous output buffer

**Search shader sketch (`resources/shaders/search.wgsl`):**

```wgsl
struct GpuCell {
    codepoint: u32,
    fg_color: u32,
    bg_color: u32,
    flags: u32,
    row: u32,
    col: u32,
};

struct SearchParams {
    pattern_len: u32,
    total_rows: u32,
    cols: u32,
    _pad: u32,
};

@group(0) @binding(0) var<storage, read> cells: array<GpuCell>;
@group(0) @binding(1) var<uniform> params: SearchParams;
@group(0) @binding(2) var<storage, read> pattern: array<u32>;  // codepoints
@group(0) @binding(3) var<storage, read_write> matches: array<u32>;  // match positions
@group(0) @binding(4) var<storage, read_write> match_count: atomic<u32>;

@compute @workgroup_size(256)
fn search_row(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;
    if (row >= params.total_rows) { return; }
    let row_start = row * params.cols;
    // Slide pattern across row
    for (var col: u32 = 0u; col <= params.cols - params.pattern_len; col++) {
        var matched = true;
        for (var i: u32 = 0u; i < params.pattern_len; i++) {
            if (cells[row_start + col + i].codepoint != pattern[i]) {
                matched = false;
                break;
            }
        }
        if (matched) {
            let idx = atomicAdd(&match_count, 1u);
            matches[idx] = row_start + col;
        }
    }
}
```

**C ABI:**
- `compute_create(device_shared: *const c_void) → *mut ComputeHandle` — takes shared wgpu device
- `compute_search(handle, pattern_codepoints, pattern_len, results_buf, max_results) → u32` — returns match count
- `compute_detect_urls(handle, row_start, row_end, results_buf, max_results) → u32`
- `compute_highlight_cells(handle, rules_json, rules_len, output_buf) → u32`
- `compute_extract_selection(handle, start_row, start_col, end_row, end_col, output_buf, max_len) → u32`
- `compute_destroy(handle)`

**Integration with renderer:**
- `ComputeEngine` and `Renderer` share `wgpu::Device` + `wgpu::Queue`
- The grid's cell buffer is `STORAGE | VERTEX` — readable by compute, drawable by render
- Compute dispatches happen between frames or async (non-blocking)
- Search/highlight results become overlay data consumed by `overlay.wgsl` in the render pass
- This means search highlights appear at GPU speed — no CPU round-trip for visual feedback

**FFI binding (`ffi/compute/mod.ts`):**
```typescript
export class ComputeEngine {
  #handle: Deno.PointerValue;
  search(pattern: string): SearchMatch[] { ... }
  detectUrls(startRow: number, endRow: number): UrlMatch[] { ... }
  highlightCells(rules: HighlightRule[]): void { ... }
  extractSelection(start: CellPos, end: CellPos): string { ... }
}
```

**Performance expectations:**
- Text search across 100K rows: <2ms (GPU) vs ~50ms (CPU)
- URL detection on visible rows: <0.5ms per frame
- Highlight classification: <0.5ms per frame
- Selection extract (10K rows): <1ms

### Step 7: `pkg/config-store`

- TOML read/write using `toml` + `serde`
- File watching via `notify` crate
- Layered: defaults → system → user → project → plugin → CLI
- C ABI: `config_store_create`, `config_store_get`, `config_store_set`, `config_store_watch`, `config_store_save`

### Step 8: `pkg/runtime`

- Bootstrapping logic: init event bus → init config → init PTY → init parser → init grid → init renderer
- Lifecycle: boot, run, shutdown
- Coordinates the pipeline in Rust (for hot path)
- Exposes lifecycle hooks for Deno integration

### Step 9: `pkg/daemon` + `pkg/ipc`

- Skeleton only in Phase 1
- `pkg/daemon`: tokio-based background process supervision
- `pkg/ipc`: Unix socket server/client, message framing, serde serialization

### Step 10: FFI Bindings (`ffi/`)

One TypeScript module per Rust crate. Each does `Deno.dlopen()` and wraps raw FFI in ergonomic classes.

Pattern for every FFI module:
```typescript
const lib = Deno.dlopen("target/debug/libmarauder_CRATE.dylib", { /* symbols */ });

export class CrateName {
  #handle: Deno.PointerValue;
  constructor(...) { this.#handle = lib.symbols.crate_create(...); }
  // ... typed methods wrapping each symbol
  close(): void { lib.symbols.crate_destroy(this.#handle); }
  [Symbol.dispose](): void { this.close(); }
}
```

Library path resolution: check `MARAUDER_LIB_DIR` env var, then `target/release/`, then `target/debug/`. File extension: `.dylib` (macOS), `.so` (Linux), `.dll` (Windows).

### Step 11: Tauri App Wiring (`apps/marauder/src-tauri/`)

- Extend `lib.rs` to init `deno_core` JsRuntime on a tokio current_thread background thread
- Register `#[op2]` ops from all `pkg/*` crates into the JsRuntime extensions
- Init wgpu renderer on the main window's raw handle
- Wire PTY reader → parser → grid → renderer pipeline
- Register Tauri commands that bridge webview → Deno → Rust
- Tauri `Channel` for streaming events to webview

### Step 12: Deno Runtime Skeleton (`lib/`)

- `lib/io/mod.ts` — stream types, buffer utilities
- `lib/io/pipeline.ts` — the pipeline wiring in TypeScript (for standalone FFI mode)
- `lib/dev/mod.ts` — logging, debug helpers
- `lib/ui/mod.ts` — PaneManager skeleton
- `lib/shell/mod.ts` — ShellEngine skeleton

### Step 13: Frontend (`apps/marauder/src/`)

- Replace Tauri scaffold content with terminal chrome
- Transparent body CSS: `body { background: transparent; margin: 0; }`
- Tab bar component at top (opaque)
- Status bar component at bottom (opaque)
- IPC wrapper: `import { invoke, Channel } from "@tauri-apps/api/core"`
- Custom drag region: `data-tauri-drag-region` on tab bar

## Rust Crate Conventions

- **Every `pkg/*` crate** must have `crate-type = ["rlib", "cdylib"]` in its `Cargo.toml`
- **FFI exports** use `#[no_mangle] pub extern "C" fn crate_function_name(...)` — always prefix with crate name to avoid symbol collisions
- **Handle pattern** for FFI: return opaque `*mut Handle` pointers. Never expose Rust types directly across FFI. The handle is a `Box::into_raw(Box::new(instance))` and freed with `Box::from_raw`.
- **`#[op2]` ops** use `#[serde]` for structured return types, `#[string]` for String args, `#[buffer]` for byte slices
- **`#[tauri::command]`** functions return `Result<T, String>` (Tauri requires String errors)
- **Error handling**: `thiserror` in `pkg/*` libs, `anyhow` in `apps/*` bins. At FFI boundary: return null/error codes. At Tauri boundary: `.map_err(|e| e.to_string())`.
- **Unsafe**: Only at FFI boundary. Always document with `// SAFETY: <invariant>`.
- **Logging**: `tracing` macros everywhere. `RUST_LOG=marauder_crate=level` for filtering.
- **Naming**: Crate names `marauder-{name}`, lib names `marauder_{name}`.

## Deno / TypeScript Conventions

- **`ffi/` modules**: Class-based wrappers with `[Symbol.dispose]()` for cleanup
- **`lib/` modules**: Each subdir has `mod.ts` as entry point
- **No npm deps** in `lib/` or `ffi/` — Deno standard library only
- **Strict TypeScript**: No `any`. All FFI data decoded into typed interfaces.
- **Import map**: Defined in `deno.json` — use `@marauder/ffi-pty`, `@marauder/shell`, etc.
- **Buffer handling**: Use `Uint8Array` for byte data. Use `TextEncoder`/`TextDecoder` for string ↔ bytes at FFI boundary. Null-terminate C strings manually.
- **Pointer types**: `Deno.PointerValue` for opaque handles. Never dereference directly — always call back into Rust.

## Tauri Frontend Conventions

- **Vite** build tool, TypeScript, strict mode
- **`@tauri-apps/api`** for all IPC: `invoke` (request/response), `Channel` (streaming), `listen`/`emit` (pub/sub)
- **Transparent webview body** — CSS `background: transparent`. Never render anything over the terminal grid area.
- **`data-tauri-drag-region`** on the tab bar for frameless window dragging
- **No heavy framework** — vanilla TS, or lightweight (Solid, Lit, Preact)

## Key Technical Constraints

1. **`deno_core` requires tokio `current_thread`** — V8 is single-threaded. The Deno runtime and wgpu rendering MUST be on separate threads, communicating via channels.

2. **wgpu + Tauri webview compositing** — The webview sits ABOVE the wgpu surface. The webview body must be transparent. Tab bar and status bar are opaque strips. There are known flickering issues on some platforms (see tauri-apps/tauri#9220). Test on macOS first (most stable).

3. **FFI callback threading** — Rust `extern "C"` callbacks from Deno FFI run on the Deno thread. Do NOT call back into the Deno runtime from a Rust thread. Use channels to forward events.

4. **Tauri `Channel` for high-throughput** — Use Tauri's `Channel<T>` (not `emit`) for streaming PTY data and frequent updates. `emit` evaluates JS directly and is slow for high-frequency data.

5. **Triple export pattern** — Every `pkg/*` crate exports three interfaces:
   - `extern "C"` functions (for `ffi/` Deno FFI bindings)
   - `#[op2]` functions (for embedded `deno_core` mode)
   - `#[tauri::command]` functions (for webview access)
   These can share the same internal implementation.

6. **Serialization at boundaries** — Use `serde_json` bytes for FFI event data. Use `serde_v8` for `#[op2]` ops. Use Tauri's built-in JSON serialization for commands.

## Current State

- **Tauri app scaffolded** at `apps/marauder/` — default Vite + TS template with a greet command
- **All `pkg/` dirs exist** but are empty
- **All `ffi/` dirs exist** but are empty
- **All `lib/` dirs exist** but are empty
- **All `extensions/` dirs exist** but are empty
- **`Cargo.toml` and `deno.json`** at root are empty — need to be populated
- **Next step**: Populate root `Cargo.toml` workspace, then implement `pkg/event-bus`

## Build Commands

```bash
cargo build                          # Build all Rust crates
cargo build --release                # Release build
cargo tauri dev                      # Tauri dev mode (Vite HMR + Rust)
cargo tauri build                    # Production binary
cargo test                           # Rust tests
cargo test -p marauder-grid          # Specific crate tests
deno task dev                        # Standalone Deno mode (FFI)
deno task test                       # Deno tests
deno task fmt                        # Format TypeScript
deno task lint                       # Lint TypeScript
```

## Dependency Versions (Pin These)

```
portable-pty   = "0.9"
vte            = "0.15"
wgpu           = "24.0"
cosmic-text    = "0.12"
tauri          = "2"
tauri-build    = "2"
tauri-plugin-opener = "2"
deno_core      = "0.311"
tokio          = "1" (features = ["full"])
serde          = "1" (features = ["derive"])
serde_json     = "1"
toml           = "0.8"
tracing        = "0.1"
tracing-subscriber = "0.3"
anyhow         = "1"
thiserror      = "2"
notify         = "7"
raw-window-handle = "0.6"
```
