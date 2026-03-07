# Marauder Architecture

## The Three-Layer Model

```text
┌─────────────────────────────────────────────────┐
│  Layer 3: Tauri Webview (apps/marauder/src/)     │
│  Vite + TypeScript + HTML/CSS                    │
│  Tabs, status bar, palette, settings, ext UI     │
├────────────── Tauri IPC ────────────────────────┤
│  Layer 2: Deno Runtime (lib/ + ffi/)             │
│  TypeScript                                      │
│  Orchestration, config, shell, extensions        │
├────────────── #[op2] / FFI ─────────────────────┤
│  Layer 1: Rust Native (pkg/)                     │
│  PTY, VT parser, grid, wgpu renderer, runtime    │
│  Hot path: PTY → parse → grid → GPU render       │
│  Tauri app shell (apps/marauder/src-tauri/)       │
└─────────────────────────────────────────────────┘
```

| Layer | Location | Language | Owns |
|-------|----------|----------|------|
| **Native** | `pkg/`, `apps/*/src-tauri/` | Rust | Performance primitives, Tauri app shell, wgpu renderer |
| **Runtime** | `lib/`, `ffi/` | Deno/TypeScript | Orchestration, lifecycle, config, shell, extensions, policy |
| **Frontend** | `apps/marauder/src/` | TypeScript/HTML/CSS | UI chrome: tabs, status bar, command palette, settings |

**Key principle:** The hot rendering path (PTY bytes → VT parse → grid update → wgpu draw) stays **entirely in Rust** — never crosses into Deno or the webview. The GPU handles both rendering (instanced draw calls) and computation (search, URL detection, highlighting via compute shaders) on the same device with zero-copy cell buffer access. Deno handles orchestration. The webview handles chrome only.

## Data Flow

### Input Path (user → shell)

```text
Tauri webview (key event)
  → Tauri emit("input:key", { key, mods })
    → Rust Tauri command handler
      → deno_core: runtime.execute("onKeyEvent", event)
        → Deno keybinding resolver (lib/ui/)
          ├─ Keybind matched → Deno executes action
          └─ No match → Deno encodes VT bytes
              → #[op2] op_pty_write(pane_id, bytes)
                → Rust PTY write → Shell stdin
```

### Output Path (shell → screen) — THE HOT PATH

```text
Shell stdout
  → Rust PTY reader thread (pkg/pty, async)
    → Rust VT parser (pkg/parser, vte)
      → Rust grid.apply_action() (pkg/grid)
        → Dirty rows flagged
  On frame tick (vsync):
    → Rust renderer.update_cells(grid) (pkg/renderer)
      → Rust renderer.render_frame() (wgpu)
        → GPU → Screen

  In parallel (non-blocking, via event bus):
    → Deno runtime notified of grid changes
      → Shell engine processes OSC sequences (lib/shell/)
      → Extension hooks fire
      → Webview notified (Tauri Channel) for tab title, status bar
```

The output hot path is **100% Rust**. Deno and the webview are notified asynchronously but never block rendering.

### UI Chrome Path (webview ↔ backend)

```text
Webview (apps/marauder/src/): user clicks tab / types in palette
  → Tauri invoke("create_tab") or invoke("run_command", { cmd })
    → Rust Tauri command (apps/marauder/src-tauri/src/lib.rs)
      → deno_core: runtime.execute("onCommand", args)
        → Deno runtime handles logic
          → #[op2] ops to Rust crates as needed
            → Result flows back via Tauri Channel to webview
```

---

## Layer 1: Rust Native (`pkg/` + `apps/`)

### `apps/marauder/src-tauri/` — Tauri App Shell

The entry point. Creates the native window, initializes wgpu, embeds `deno_core`, registers Tauri commands, and manages the render loop.

```rust
// apps/marauder/src-tauri/src/lib.rs
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

This will grow to include:
- Tauri plugin wrappers for each `pkg/*` crate
- `deno_core` JsRuntime on a background thread
- wgpu surface initialization on the raw window handle
- Pipeline wiring (PTY → parser → grid → renderer)
- Tauri commands bridging webview → Deno → Rust crates

### `pkg/runtime` — Core Runtime

Runtime lifecycle management, bootstrapping, and coordination between Rust crates.

### `pkg/pty` — Pseudoterminal Manager

Wraps `portable-pty`. Exposes C ABI for FFI + `#[op2]` for embedded Deno + `#[tauri::command]` for webview.

```rust
// C ABI (for ffi/pty/)
#[no_mangle] pub extern "C" fn pty_create(config: *const PtyConfig) -> *mut PtyHandle;
#[no_mangle] pub extern "C" fn pty_read(handle: *mut PtyHandle, buf: *mut u8, len: usize) -> isize;
#[no_mangle] pub extern "C" fn pty_write(handle: *mut PtyHandle, data: *const u8, len: usize) -> isize;
#[no_mangle] pub extern "C" fn pty_resize(handle: *mut PtyHandle, cols: u16, rows: u16);
#[no_mangle] pub extern "C" fn pty_close(handle: *mut PtyHandle);

// deno_core ops (for embedded mode)
#[op2(async)]
async fn op_pty_create(state: &OpState, #[string] shell: String, cols: u16, rows: u16) -> Result<String, AnyError>;
#[op2(async)]
async fn op_pty_write(state: &OpState, #[string] pane_id: String, #[buffer] data: &[u8]) -> Result<(), AnyError>;

// Tauri command (for webview access)
#[tauri::command]
async fn cmd_create_pane(state: State<'_, PtyManager>, shell: String, cols: u16, rows: u16) -> Result<String, String>;
```

### `pkg/parser` — VT/ANSI Parser

Wraps `vte`. Consumed directly by the pipeline in Rust (hot path). Also exposes FFI/ops for Deno access.

### `pkg/grid` — Terminal Grid State

Cell buffer, scrollback, cursor, selection, dirty tracking. Read-only ops for Deno/webview queries.

### `pkg/renderer` — GPU-Accelerated Renderer

GPU acceleration is a core requirement. Every frame is rendered entirely on the GPU via wgpu — no CPU-side rasterization, no software fallback.

**Rendering strategy: instanced draw calls**

The renderer uses instanced rendering — one draw call for all background cells, one draw call for all text glyphs. The CPU builds an instance buffer (one entry per cell), uploads it to the GPU, and the GPU draws everything in parallel.

```rust
// Per-cell instance data uploaded to GPU
#[repr(C)]
struct CellInstance {
    pos: [f32; 2],           // pixel position
    size: [f32; 2],          // cell dimensions
    bg_color: [f32; 4],      // background RGBA
    fg_color: [f32; 4],      // foreground RGBA
    glyph_uv: [f32; 4],     // glyph atlas UV coordinates
    glyph_offset: [f32; 2],  // glyph bearing offset
    flags: u32,              // bold, italic, underline, strikethrough, cursor, selected
}
```

**Glyph atlas:**
- `cosmic-text` rasterizes glyphs on CPU → grayscale bitmaps
- Packed into 1024×1024 GPU textures (bin-packed atlas)
- Stored as `R8Unorm` (single channel), `Rgba8Unorm` for color emoji
- Atlas rebuilt only on font change or new glyph encounter
- Pre-warmed with ASCII + common glyphs at startup

**Render pipeline per frame:**
1. `update_cells(grid)` — read dirty rows from grid, update instance buffer slices (NOT full rebuild)
2. Begin render pass (clear with terminal background)
3. Background pass — instanced quads, bg_color per instance
4. Text pass — instanced textured quads, sample glyph atlas × fg_color
5. Overlay pass — cursor (animated), selection highlight, extension overlays
6. End render pass, `surface.present()`

**WGSL shaders** (`resources/shaders/`):
- `background.wgsl` — vertex positions quad from instance pos/size, fragment outputs bg_color
- `text.wgsl` — vertex positions quad + glyph_offset, fragment samples atlas × fg_color
- `cursor.wgsl` — animated cursor block/underline/bar with blink
- `overlay.wgsl` — selection, search highlights, extension decorations

**Performance targets:**
- 120fps sustained at full-screen (250×80 = 20K cells)
- < 1ms CPU time per frame (buffer update + command encoding)
- Damage tracking: only dirty rows rebuild instance data
- `cat large_file` must not drop frames (PTY read batching + dirty coalescing)

**wgpu configuration:**
- Backend: auto-detect (Vulkan/Linux, Metal/macOS, DX12/Windows)
- Surface format: `Bgra8UnormSrgb`
- Present mode: `Fifo` (vsync) or `Mailbox` (low-latency)
- DPI: `window.scale_factor()` → scale cell size and resize surface

**C ABI:**
```rust
#[no_mangle] pub extern "C" fn renderer_create(window_handle: *const c_void, config: *const u8, len: usize) -> *mut RendererHandle;
#[no_mangle] pub extern "C" fn renderer_set_font(handle: *mut RendererHandle, family: *const c_char, size: f32, line_height: f32);
#[no_mangle] pub extern "C" fn renderer_set_theme(handle: *mut RendererHandle, theme_json: *const u8, len: usize);
#[no_mangle] pub extern "C" fn renderer_update_cells(handle: *mut RendererHandle, grid: *mut GridHandle);
#[no_mangle] pub extern "C" fn renderer_render_frame(handle: *mut RendererHandle);
#[no_mangle] pub extern "C" fn renderer_resize_surface(handle: *mut RendererHandle, width: u32, height: u32, scale: f32);
#[no_mangle] pub extern "C" fn renderer_add_overlay(handle: *mut RendererHandle, layer_id: u32, config: *const u8, len: usize);
#[no_mangle] pub extern "C" fn renderer_remove_overlay(handle: *mut RendererHandle, layer_id: u32);
#[no_mangle] pub extern "C" fn renderer_set_cursor_style(handle: *mut RendererHandle, style: u32, blink: bool);
#[no_mangle] pub extern "C" fn renderer_get_cell_size(handle: *mut RendererHandle, out_w: *mut f32, out_h: *mut f32);
#[no_mangle] pub extern "C" fn renderer_destroy(handle: *mut RendererHandle);
```

### `pkg/compute` — GPU Compute Engine

The GPU is not just for rendering — `pkg/compute` uses wgpu compute shaders for runtime operations that benefit from massive parallelism. It shares the `wgpu::Device` and `wgpu::Queue` with `pkg/renderer`, and reads the same cell storage buffer with zero-copy.

**GPU-accelerated operations:**

| Operation | What it does | GPU advantage |
|-----------|-------------|---------------|
| **Text search** | Substring/regex search across entire scrollback | Each workgroup processes one row in parallel — 100K rows in <2ms |
| **URL detection** | Scan cells for URL/email/path patterns | State machine per row, runs on every new content arrival |
| **Semantic highlighting** | Classify cell runs (numbers, paths, flags, operators) | Parallel classification, results feed render overlay pass |
| **Selection extract** | Gather codepoints from large selection ranges | Parallel gather into contiguous buffer, <1ms for 10K rows |
| **Cell transforms** | Batch dim/highlight cells during search | Modify overlay buffer directly on GPU, no CPU round-trip |
| **Diff computation** | Compare command outputs for extensions | GPU-parallel LCS on cell buffers |

**Zero-copy architecture:**

The renderer already uploads cell data to a GPU buffer (`wgpu::BufferUsages::STORAGE | VERTEX`). Compute shaders bind that same buffer as read-only storage — no redundant CPU→GPU copy. Compute results (match positions, highlight categories) are written to separate GPU buffers that the renderer's overlay pass consumes directly.

```text
Grid (CPU) → cell_buffer (GPU, STORAGE|VERTEX)
                ├── Renderer reads as vertex instances → draw calls
                └── Compute reads as storage → search/detect/highlight
                        └── results_buffer (GPU) → overlay render pass
```

**Shared GPU cell representation:**

```rust
#[repr(C)]
struct GpuCell {
    codepoint: u32,       // Unicode codepoint
    fg_color: [u8; 4],    // RGBA packed
    bg_color: [u8; 4],    // RGBA packed
    flags: u32,           // bold, italic, underline, etc.
    row: u32,             // row index
    col: u32,             // col index
}
```

**Compute shaders** (`resources/shaders/`):

- `search.wgsl` — Per-row substring/regex match, writes positions to atomic-counted output buffer
- `url_detect.wgsl` — Per-row state machine for URL pattern recognition
- `highlight.wgsl` — Cell run classifier, outputs highlight categories for overlay pass
- `selection_extract.wgsl` — Parallel codepoint gather from row range

**C ABI:**

```rust
#[no_mangle] pub extern "C" fn compute_create(device_shared: *const c_void) -> *mut ComputeHandle;
#[no_mangle] pub extern "C" fn compute_search(handle: *mut ComputeHandle, pattern: *const u32, pattern_len: u32, results: *mut u32, max: u32) -> u32;
#[no_mangle] pub extern "C" fn compute_detect_urls(handle: *mut ComputeHandle, row_start: u32, row_end: u32, results: *mut u32, max: u32) -> u32;
#[no_mangle] pub extern "C" fn compute_highlight_cells(handle: *mut ComputeHandle, rules: *const u8, rules_len: u32, output: *mut u32) -> u32;
#[no_mangle] pub extern "C" fn compute_extract_selection(handle: *mut ComputeHandle, sr: u32, sc: u32, er: u32, ec: u32, out: *mut u32, max: u32) -> u32;
#[no_mangle] pub extern "C" fn compute_destroy(handle: *mut ComputeHandle);
```

**Scheduling:** Compute dispatches run between frames or asynchronously. The search pipeline: Deno calls `compute_search` → GPU dispatch → results read back via `map_async` → highlight overlay updated → next frame renders matches with zero additional CPU work.

### `pkg/event-bus` — Native Event System

Typed pub/sub across Rust layers. Bridges events to Deno and webview:

```rust
pub struct EventBus {
    subscribers: HashMap<EventType, Vec<Box<dyn Fn(&Event) + Send + Sync>>>,
    deno_bridge: Option<DenoEventBridge>,     // async forward to deno_core
    tauri_bridge: Option<TauriEventBridge>,   // async forward to webview Channel
}
```

### `pkg/config-store` — Config Storage Backend

Fast TOML/JSON read/write with file watching.

### `pkg/ipc` — IPC Transport

Unix socket / named pipe protocol for multiplexer mode.

### `pkg/daemon` — Background Process Management

Daemon lifecycle, process supervision, background task scheduling.

### `apps/marauder-server/` — Headless Multiplexer

Rust binary, no GUI. Manages PTY sessions over IPC.

---

## Layer 2: Deno Runtime (`lib/` + `ffi/`)

### FFI Binding Packages (`ffi/`)

Each Rust crate in `pkg/` has a corresponding TypeScript FFI wrapper in `ffi/`:

```text
ffi/
├── pty/           # Deno.dlopen() → libmarauder_pty
├── parser/        # Deno.dlopen() → libmarauder_parser
├── grid/          # Deno.dlopen() → libmarauder_grid
├── renderer/      # Deno.dlopen() → libmarauder_renderer
├── compute/       # Deno.dlopen() → libmarauder_compute
├── event-bus/     # Deno.dlopen() → libmarauder_event_bus
└── config-store/  # Deno.dlopen() → libmarauder_config_store
```

Example (`ffi/pty/mod.ts`):
```typescript
const lib = Deno.dlopen("libmarauder_pty.dylib", {
  pty_create: { parameters: ["pointer"], result: "pointer" },
  pty_read: { parameters: ["pointer", "pointer", "usize"], result: "isize" },
  pty_write: { parameters: ["pointer", "pointer", "usize"], result: "isize" },
  pty_resize: { parameters: ["pointer", "u16", "u16"], result: "void" },
  pty_close: { parameters: ["pointer"], result: "void" },
});

export class Pty {
  #handle: Deno.PointerValue;

  constructor(config: PtyConfig) {
    this.#handle = lib.symbols.pty_create(encodePtyConfig(config));
  }

  write(data: Uint8Array): number {
    return Number(lib.symbols.pty_write(this.#handle, data, data.length));
  }

  read(buffer: Uint8Array): number {
    return Number(lib.symbols.pty_read(this.#handle, buffer, buffer.length));
  }

  resize(cols: number, rows: number): void {
    lib.symbols.pty_resize(this.#handle, cols, rows);
  }

  close(): void { lib.symbols.pty_close(this.#handle); }
  [Symbol.dispose](): void { this.close(); }
}
```

### Runtime Libraries (`lib/`)

TypeScript modules that compose FFI bindings and ops into higher-level systems:

```text
lib/
├── shell/         # Shell engine
│   ├── mod.ts     # ShellEngine: orchestrates zones, history, completions
│   ├── zones.ts   # OSC 133 semantic zone tracker
│   ├── history.ts # Command history with search
│   └── completions.ts  # Extensible tab completion
├── ui/            # UI logic
│   ├── mod.ts     # PaneManager, TabManager
│   ├── panes.ts   # Pane lifecycle, split, close, focus
│   ├── tabs.ts    # Tab management
│   └── layout.ts  # Tree-based layout engine
├── io/            # I/O handling
│   ├── mod.ts     # Stream management, data pipeline
│   └── pipeline.ts # PTY → parser → grid wiring
└── dev/           # Development tools
    ├── mod.ts     # Debug utilities
    └── inspector.ts # Runtime inspection helpers
```

### Shell Engine (`lib/shell/`)

```typescript
export class ShellEngine {
  private zones = new SemanticZoneTracker();
  private history = new CommandHistory();
  private completions = new CompletionEngine();

  constructor(bus: EventBus, config: ConfigResolver) {
    bus.on("parser:osc", (e) => this.handleOsc(e));
  }

  private handleOsc(event: OscEvent): void {
    switch (event.code) {
      case 133: this.zones.update(event.params); break;
      case 7:   this.bus.publish("shell:cwd_changed", { cwd: event.uri }); break;
    }
  }

  getCwd(): string { return this.zones.cwd; }
  getHistory(): CommandEntry[] { return this.history.entries(); }
  getCompletions(prefix: string): string[] { return this.completions.complete(prefix); }
}
```

### UI Logic (`lib/ui/`)

```typescript
export class PaneManager {
  async createPane(config: PaneConfig): Promise<Pane> {
    const paneId = await Marauder.ops.pty.create(config.shell, config.cols, config.rows);
    const pane = new Pane({ id: paneId, ...config });
    this.panes.set(paneId, pane);
    this.bus.publish("pane:created", { paneId });
    return pane;
  }

  split(paneId: string, direction: "horizontal" | "vertical"): Pane { ... }
  close(paneId: string): void { ... }
  focus(paneId: string): void { ... }
}
```

### Config System

Supports both TOML and TypeScript config files:

```typescript
// ~/.config/marauder/config.ts
import { defineConfig } from "@marauder/config";

export default defineConfig({
  terminal: { shell: "/bin/zsh", scrollback: 10_000 },
  font: { family: "JetBrains Mono", size: 14 },
  keybindings: {
    "ctrl+shift+t": "tab.new",
    "ctrl+shift+w": "tab.close",
  },
  extensions: ["theme-catppuccin", "status-bar", "git-integration"],
});
```

Resolution order (last wins):
1. Built-in defaults
2. System config (`/etc/marauder/config.toml`)
3. User config (`~/.config/marauder/config.ts` or `config.toml`)
4. Project config (`.marauder/config.ts` in CWD)
5. Extension overrides
6. CLI flags

### Extension System

Extensions are TypeScript packages in `extensions/`. They have the same access to FFI bindings and ops as the core.

**Extension manifest (`extension.json`):**
```json
{
  "name": "git-integration",
  "version": "1.0.0",
  "entry": "mod.ts",
  "permissions": {
    "terminal.read": true,
    "filesystem.read": ["~/.git", ".git"],
    "shell.execute": ["git"]
  },
  "hooks": ["shell:cwd_changed", "shell:command_finished"]
}
```

**Extension code (`mod.ts`):**
```typescript
import type { ExtensionContext } from "@marauder/extensions";

export function activate(ctx: ExtensionContext) {
  ctx.on("shell:cwd_changed", async ({ cwd }) => {
    const branch = await ctx.shell.exec("git", ["branch", "--show-current"], { cwd });
    ctx.statusBar.set("git-branch", { text: `${branch.trim()}`, position: "left" });
  });

  ctx.registerKeybind("ctrl+shift+g", "git.palette");
}

export function deactivate() {}
```

**Extension API surface:**
```typescript
export interface ExtensionContext {
  on(event: string, handler: (data: any) => void): void;
  registerCommand(name: string, handler: () => void): void;
  registerKeybind(combo: string, command: string): void;
  statusBar: { set(id: string, content: StatusBarItem): void };
  notifications: { show(msg: string, opts?: NotifyOpts): void };
  commandPalette: { register(items: PaletteItem[]): void };
  terminal: { write, getCell, getCursor, getSelectionText };
  shell: { getCwd, getHistory, exec };
  config: { get, set, onChange };
}
```

---

## Layer 3: Tauri Webview (`apps/marauder/src/`)

### What the Webview Renders

The webview handles **UI chrome only** — not the terminal grid:

- **Tab bar** — tab list, new/close, switching, drag-to-reorder
- **Status bar** — segments populated by extensions
- **Command palette** — Ctrl+Shift+P fuzzy finder
- **Settings panel** — GUI config editor
- **Extension UI** — custom panels/overlays
- **Notifications** — toast messages

The terminal grid area is transparent in the webview, with wgpu rendering underneath.
`macOSPrivateApi: true` is required in `tauri.conf.json` because Tauri's transparent window and `windowEffects` (vibrancy) support on macOS depends on private NSWindow APIs for proper compositing of the wgpu surface behind the webview.

### Frontend Stack

- **Vite** — build tool with HMR (configured for Tauri dev)
- **TypeScript** — type-safe frontend code
- **@tauri-apps/api** — IPC with Rust backend
- **Vanilla or lightweight framework** — no heavy framework required

```text
apps/marauder/
├── src/                   # Frontend source
│   ├── main.ts            # Entry point
│   ├── styles.css         # Global styles
│   └── assets/            # Static assets
├── index.html             # Root HTML
├── package.json           # Vite, @tauri-apps/api, TypeScript
├── vite.config.ts         # Vite config (Tauri integration)
└── tsconfig.json
```

### Frontend ↔ Backend IPC

```typescript
// apps/marauder/src/ipc.ts
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Commands (request/response)
export const createTab = () => invoke<string>("create_tab");
export const runCommand = (cmd: string) => invoke("run_command", { cmd });

// Channels (streaming from backend)
const statusChannel = new Channel<StatusBarUpdate>();
statusChannel.onmessage = (update) => renderStatusSegment(update);

// Events (pub/sub)
listen("tab:created", (e) => addTab(e.payload));
listen("tab:title_changed", (e) => updateTabTitle(e.payload));
listen("notifications:show", (e) => showToast(e.payload));
```

### Webview + wgpu Compositing

```text
┌──────────────────────────────────────┐
│ Tab Bar (webview, opaque)            │  ← HTML/CSS
├──────────────────────────────────────┤
│                                      │
│  Terminal Grid (wgpu, below webview) │  ← GPU rendered
│  [transparent webview body above]    │
│                                      │
├──────────────────────────────────────┤
│ Status Bar (webview, opaque)         │  ← HTML/CSS
└──────────────────────────────────────┘
```

---

## Threading Model

```text
┌─────────────────────────┐
│  Main Thread             │ ← Tauri event loop + wgpu render loop
├─────────────────────────┤
│  Deno Thread             │ ← tokio current_thread + V8 isolate
├─────────────────────────┤
│  PTY Reader Thread(s)    │ ← One per PTY (async read → parse → grid)
├─────────────────────────┤
│  Webview Thread          │ ← Managed by Tauri/wry (OS webview process)
├─────────────────────────┤
│  File Watcher Thread     │ ← Config + extension hot-reload
└─────────────────────────┘
```

Inter-thread communication via `tokio::sync::mpsc` channels + Tauri `Channel` API.

## Security Model

### Tauri Permissions (App-Level)

```json
// apps/marauder/src-tauri/capabilities/default.json
{
  "identifier": "default",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default"
  ]
}
```

Will expand to include per-plugin permissions as Tauri plugins are added.

### Extension Sandboxing

| Permission | Default |
|-----------|---------|
| `terminal.read` | Granted |
| `terminal.write` | Denied |
| `filesystem.read` | Denied (scoped) |
| `filesystem.write` | Denied (scoped) |
| `network` | Denied |
| `shell.execute` | Denied (scoped) |
| `config.write` | Denied |
| `ui.statusbar` | Granted |
| `ui.notifications` | Granted |
