# Marauder Development Guide

## Prerequisites

- **Rust** (stable, 1.80+): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Deno** (2.x+): `curl -fsSL https://deno.land/install.sh | sh`
- **Tauri CLI**: `cargo install tauri-cli --version "^2"`
- **System dependencies:**
  - Linux: `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev libwayland-dev libxkbcommon-dev pkg-config cmake`
  - macOS: Xcode Command Line Tools (`xcode-select --install`)
  - Windows: Visual Studio Build Tools with C++ workload, WebView2

## Project Layout

```text
marauder/
├── apps/
│   ├── marauder/                  # Tauri v2 app
│   │   ├── src/                   # Frontend (Vite + TypeScript + HTML/CSS)
│   │   │   ├── main.ts            # Frontend entry point
│   │   │   ├── styles.css
│   │   │   └── assets/
│   │   ├── src-tauri/             # Rust backend
│   │   │   ├── src/
│   │   │   │   ├── main.rs        # Binary entry point
│   │   │   │   └── lib.rs         # Tauri builder, commands, plugins
│   │   │   ├── Cargo.toml
│   │   │   ├── build.rs
│   │   │   ├── tauri.conf.json
│   │   │   ├── capabilities/
│   │   │   └── icons/
│   │   ├── index.html
│   │   ├── package.json
│   │   ├── vite.config.ts
│   │   └── tsconfig.json
│   └── marauder-server/           # Headless multiplexer daemon
│
├── pkg/                           # Rust crates (rlib + cdylib)
│   ├── pty/                       # PTY management (portable-pty)
│   ├── parser/                    # VT/ANSI parser (vte)
│   ├── grid/                      # Terminal cell grid + scrollback
│   ├── renderer/                  # GPU renderer (wgpu + cosmic-text)
│   ├── compute/                   # GPU compute (search, URL detect, highlights)
│   ├── runtime/                   # Core runtime logic
│   ├── event-bus/                 # Native event system
│   ├── config-store/              # Config storage backend
│   ├── ipc/                       # IPC transport (multiplexer)
│   └── daemon/                    # Background process management
│
├── ffi/                           # Deno FFI binding packages
│   ├── pty/
│   ├── parser/
│   ├── grid/
│   ├── renderer/
│   ├── event-bus/
│   └── config-store/
│
├── lib/                           # Deno TypeScript runtime layer
│   ├── shell/                     # Shell engine
│   ├── ui/                        # UI logic (panes, tabs, layout)
│   ├── io/                        # I/O handling, stream management
│   └── dev/                       # Development tools
│
├── extensions/                    # Built-in extensions (TypeScript)
│   ├── theme-default/
│   ├── status-bar/
│   ├── git-integration/
│   ├── search/
│   └── notifications/
│
├── resources/
│   ├── shaders/
│   ├── fonts/
│   └── shell-integrations/
│
├── bin/
│   ├── install.sh
│   ├── marauder.sh
│   └── uninstall.sh
├── docs/
├── Cargo.toml
└── deno.json
```

## Building

### Tauri App (Primary)

```bash
# Development mode (Vite HMR + Rust hot-compile)
cargo tauri dev

# Production build (single native binary)
cargo tauri build
```

The Tauri dev command runs `deno task dev` (configured in `tauri.conf.json` → `build.beforeDevCommand`) which starts the Vite dev server on port 1420.

### Rust Crates Only

```bash
# Build all crates (rlib + cdylib shared libraries)
cargo build

# Release
cargo build --release

# Specific crate
cargo build -p marauder-pty

# Shared library output:
#   target/debug/libmarauder_pty.dylib   (macOS)
#   target/debug/libmarauder_pty.so      (Linux)
#   target/debug/marauder_pty.dll        (Windows)
```

### Deno Standalone Mode

```bash
# Build Rust shared libs first
cargo build

# Run terminal in Deno-driven mode (no Tauri, no webview)
deno task dev
```

### Frontend Only

```bash
cd apps/marauder
deno task dev      # Vite dev server at localhost:1420
deno task build    # Production build → dist/
```

## Workspace Configurations

### Cargo.toml (Rust)

```toml
[workspace]
resolver = "2"
members = [
  "apps/marauder/src-tauri",
  "apps/marauder-server",
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

[workspace.dependencies]
# Terminal core
portable-pty = "0.9"
vte = "0.15"

# GPU rendering
wgpu = "24.0"
cosmic-text = "0.12"

# Tauri
tauri = { version = "2", features = [] }
tauri-build = "2"
tauri-plugin = "2"
tauri-plugin-opener = "2"

# Deno runtime
deno_core = "0.311"
tokio = { version = "1", features = ["full"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# Utilities
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = "1"
thiserror = "2"
notify = "7"
raw-window-handle = "0.6"
```

### tauri.conf.json

```jsonc
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "marauder",
  "version": "0.1.0",
  "identifier": "com.ryanoboyle.marauder",
  "build": {
    "beforeDevCommand": "deno task dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "deno task build",
    "frontendDist": "../dist"
  },
  "app": {
    "withGlobalTauri": true,
    "windows": [{ "title": "marauder", "width": 800, "height": 600 }],
    "security": { "csp": null }
  }
}
```

### deno.json

```jsonc
{
  "tasks": {
    "dev": "cd apps/marauder && vite",
    "build": "cd apps/marauder && tsc && vite build",
    "check": "deno check lib/**/*.ts ffi/**/*.ts",
    "test": "deno test --unstable-ffi --allow-ffi lib/ ffi/",
    "test:extensions": "deno test extensions/",
    "fmt": "deno fmt lib/ ffi/ extensions/",
    "lint": "deno lint lib/ ffi/ extensions/"
  },
  "imports": {
    "@marauder/ffi-pty": "./ffi/pty/mod.ts",
    "@marauder/ffi-parser": "./ffi/parser/mod.ts",
    "@marauder/ffi-grid": "./ffi/grid/mod.ts",
    "@marauder/ffi-renderer": "./ffi/renderer/mod.ts",
    "@marauder/ffi-event-bus": "./ffi/event-bus/mod.ts",
    "@marauder/ffi-config-store": "./ffi/config-store/mod.ts",
    "@marauder/shell": "./lib/shell/mod.ts",
    "@marauder/ui": "./lib/ui/mod.ts",
    "@marauder/io": "./lib/io/mod.ts",
    "@marauder/dev": "./lib/dev/mod.ts"
  }
}
```

### package.json (Tauri Frontend)

```json
{
  "name": "marauder",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-opener": "^2"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "vite": "^6.0.3",
    "typescript": "~5.6.2"
  }
}
```

## Dependency Graph

```text
Rust crates (rlib + cdylib):
  event-bus          (standalone)
  pty                → portable-pty
  parser             → vte
  grid               → parser (types)
  renderer           → grid, wgpu, cosmic-text, raw-window-handle
  compute            → grid, wgpu (shares device with renderer)
  runtime            → event-bus, pty, parser, grid, compute
  config-store       → toml, serde, notify
  ipc                → serde, tokio
  daemon             → tokio

Tauri app (apps/marauder/src-tauri):
  → tauri, deno_core, all pkg/* crates

Deno FFI packages (ffi/):
  ffi/pty            → Deno.dlopen() → libmarauder_pty
  ffi/parser         → Deno.dlopen() → libmarauder_parser
  ffi/grid           → Deno.dlopen() → libmarauder_grid
  ffi/renderer       → Deno.dlopen() → libmarauder_renderer
  ffi/compute        → Deno.dlopen() → libmarauder_compute
  ffi/event-bus      → Deno.dlopen() → libmarauder_event_bus
  ffi/config-store   → Deno.dlopen() → libmarauder_config_store

Deno runtime (lib/):
  lib/shell          → ffi/event-bus, ffi/pty
  lib/ui             → ffi/pty, ffi/grid, ffi/renderer, ffi/event-bus
  lib/io             → ffi/pty, ffi/parser, ffi/grid
  lib/dev            → (standalone utilities)

Frontend (apps/marauder/src/):
  → @tauri-apps/api (invoke, Channel, listen)
```

## Development Workflows

### Adding a New Rust Crate

1. `cargo init pkg/my-crate --lib`
2. Set `crate-type = ["rlib", "cdylib"]`
3. Add `#[no_mangle] pub extern "C" fn ...` exports (for FFI)
4. Add `#[op2]` functions (for embedded Deno)
5. Add `#[tauri::command]` functions (for webview access)
6. Add to workspace `Cargo.toml` members
7. Create `ffi/my-crate/mod.ts` with `Deno.dlopen()` wrapper
8. Add to `deno.json` imports

### Adding an FFI Function

**Rust** (`pkg/grid/src/lib.rs`):

```rust
#[no_mangle]
pub extern "C" fn grid_search_text(
    handle: *mut GridHandle,
    pattern: *const c_char,
    results: *mut SearchResult,
    max_results: usize,
) -> usize {
    let grid = unsafe { &*handle };
    let pattern = unsafe { CStr::from_ptr(pattern) }.to_str().unwrap();
    grid.search(pattern, results, max_results)
}
```

**Deno FFI** (`ffi/grid/mod.ts`):

```typescript
// Add to Deno.dlopen symbols
grid_search_text: {
  parameters: ["pointer", "pointer", "pointer", "usize"],
  result: "usize",
},

// Add typed method to Grid class
searchText(pattern: string): SearchResult[] {
  const patternBuf = new TextEncoder().encode(pattern + "\0");
  const resultsBuf = new Uint8Array(MAX_RESULTS * SEARCH_RESULT_SIZE);
  const count = lib.symbols.grid_search_text(
    this.#handle, patternBuf, resultsBuf, MAX_RESULTS
  );
  return decodeSearchResults(resultsBuf, count);
}
```

### Adding a Tauri Command

**Rust** (`apps/marauder/src-tauri/src/lib.rs`):

```rust
#[tauri::command]
async fn create_tab(state: State<'_, DenoRuntime>) -> Result<TabInfo, String> {
    state.execute("runtime.createTab()").await.map_err(|e| e.to_string())
}
```

**Frontend** (`apps/marauder/src/main.ts`):

```typescript
import { invoke } from "@tauri-apps/api/core";
const tab = await invoke<TabInfo>("create_tab");
```

### Writing an Extension

**Manifest** (`extensions/my-ext/extension.json`):

```json
{
  "name": "my-extension",
  "version": "0.1.0",
  "entry": "mod.ts",
  "permissions": { "terminal.read": true, "ui.statusbar": true }
}
```

**Code** (`extensions/my-ext/mod.ts`):

```typescript
import type { ExtensionContext } from "@marauder/extensions";

export function activate(ctx: ExtensionContext) {
  ctx.on("shell:command_finished", ({ exitCode, command }) => {
    ctx.statusBar.set("last-cmd", {
      text: exitCode === 0 ? `✓ ${command}` : `✗ ${command}`,
      position: "right",
    });
  });
}

export function deactivate() {}
```

## Testing

```bash
# Rust — all crates
cargo test

# Rust — specific crate
cargo test -p marauder-grid

# Deno — lib/ and ffi/ (requires cargo build first)
cargo build && deno task test

# Extensions
deno task test:extensions

# Tauri app (dev mode)
cargo tauri dev
```

## Debugging

### Rust

```bash
RUST_LOG=marauder_parser=trace cargo tauri dev    # Trace VT parsing
WGPU_BACKEND=vulkan WGPU_VALIDATION=1 cargo tauri dev  # GPU validation
WGPU_BACKEND=gl cargo tauri dev                   # Software fallback
```

### Deno

```bash
# V8 inspector (standalone mode)
deno run --inspect --unstable-ffi --allow-ffi lib/main.ts

# FFI tracing
MARAUDER_FFI_TRACE=1 deno task dev
```

### Tauri Webview

```bash
cargo tauri dev  # Right-click → Inspect in the webview
```

### FFI Symbols

```bash
nm -gU target/debug/libmarauder_pty.dylib | grep "pty_"
```

## Code Conventions

### Rust

- **Crate naming**: `marauder-{name}` (e.g., `marauder-grid`)
- **Lib targets**: `crate-type = ["rlib", "cdylib"]`
- **FFI**: `#[no_mangle] pub extern "C" fn crate_function(...)` — prefix with crate name
- **Ops**: `#[op2]` with `#[serde]` returns for embedded Deno
- **Tauri**: `#[tauri::command]` for webview endpoints
- **Errors**: `thiserror` in libs, `anyhow` in bins
- **Unsafe**: Only at FFI boundary with `// SAFETY:` comments
- **Logging**: `tracing` macros

### Deno / TypeScript (`lib/`, `ffi/`)

- **FFI wrappers**: Class-based, `[Symbol.dispose]()` for cleanup
- **No npm deps**: Deno standard library only
- **Strict TypeScript**: No `any`
- **Testing**: `Deno.test()` with `--unstable-ffi`

### Frontend (`apps/marauder/src/`)

- **Vite** build tooling
- **@tauri-apps/api** for IPC
- **CSS custom properties** for theming (`--marauder-*`)
- **Transparent body** — never occlude the wgpu surface
