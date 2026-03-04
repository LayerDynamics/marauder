# Marauder Terminal

## What is Marauder?

Marauder is a GPU-accelerated, fully extensible terminal built from scratch using three co-equal technologies:

- **Rust** — high-performance native crates (PTY, VT parsing, grid state, GPU rendering, runtime, daemon) + Tauri app shell
- **Deno** — runtime layer for orchestration, configuration, shell logic, and the extension system via FFI bindings into every Rust crate
- **Tauri** — native window management, Vite-powered webview for UI chrome (tabs, status bar, command palette, settings), and IPC between frontend and backend

Every Rust crate in `pkg/` ships as both `rlib` (for the Tauri binary) and `cdylib` (for Deno FFI), making every layer usable from TypeScript in both embedded and standalone modes.

## Vision

> Rust builds the engine. Deno drives the car. Tauri wraps the cockpit.

The terminal grid renders via **wgpu** for native GPU performance. The UI chrome (tabs, status bar, command palette, settings) renders in Tauri's **webview** via Vite + TypeScript — trivially themeable and hackable. **Deno** orchestrates everything: pipeline, extensions, config, and shell logic.

## Design Principles

1. **Three-Layer Architecture**: Rust native crates → Deno runtime (FFI + embedded `deno_core`) → Tauri webview frontend. Each layer has clear ownership and a clean boundary.

2. **Hybrid Rendering**: wgpu renders the terminal grid (hot path, 120fps). Tauri webview renders UI chrome. The hot path (PTY → parse → grid → GPU) stays entirely in Rust.

3. **Deno as Runtime**: Deno owns orchestration, config resolution, extension loading, shell integration, keybinding resolution, and all policy decisions. It calls into Rust for the heavy lifting.

4. **Everything is Extensible**: Extensions are TypeScript packages with the same power as the core. They hook into the Deno runtime, register commands, update webview UI, and access native crates through FFI/ops.

5. **Everything is Hackable**: The webview frontend is TypeScript/HTML/CSS via Vite. The Deno layer in `lib/` and `ffi/` is plain TypeScript. Users can override any module without recompiling Rust.

6. **Minimal Dependencies**: Rust uses `portable-pty`, `vte`, `wgpu`, `cosmic-text`, `tauri`, `deno_core`. No framework bloat.

## Architecture at a Glance

```text
┌─────────────────────────────────────────────────────────────┐
│              Tauri Webview (apps/marauder/src/)              │
│  Vite + TypeScript + HTML/CSS                               │
│  Tab bar, status bar, command palette, settings, ext UI     │
│                  Tauri invoke / Channel / emit               │
├─────────────────────────────┬───────────────────────────────┤
│   Deno Runtime (lib/)       │   Deno FFI Bindings (ffi/)    │
│   lib/shell/  lib/ui/       │   ffi/pty/  ffi/parser/       │
│   lib/io/     lib/dev/      │   ffi/grid/ ffi/renderer/     │
│                             │   ffi/event-bus/              │
│                             │   ffi/config-store/           │
├─────────────────────────────┴───────────────────────────────┤
│                  Rust Native Layer (pkg/)                    │
│  pkg/pty  pkg/parser  pkg/grid  pkg/renderer  pkg/runtime   │
│  pkg/event-bus  pkg/config-store  pkg/ipc  pkg/daemon       │
├─────────────────────────────────────────────────────────────┤
│  Tauri App Shell (apps/marauder/src-tauri/)                 │
│  Window management, wgpu surface, system tray, IPC          │
└─────────────────────────────────────────────────────────────┘
```

## Technology Stack

| Layer | Component | Technology | Role |
|-------|-----------|-----------|------|
| **Frontend** | UI chrome | Vite + TypeScript + HTML/CSS (Tauri webview) | Tabs, status bar, palette, settings, extension UI |
| **Frontend** | IPC | Tauri `invoke` / `Channel` / `emit` | Frontend ↔ backend communication |
| **Deno** | Shell engine | `lib/shell/` (TypeScript) | Prompt zones, completions, history, CWD tracking |
| **Deno** | UI logic | `lib/ui/` (TypeScript) | Pane management, layout engine, tab/pane composition |
| **Deno** | I/O layer | `lib/io/` (TypeScript) | Stream handling, data pipeline wiring |
| **Deno** | Dev tools | `lib/dev/` (TypeScript) | Development utilities, debugging helpers |
| **Deno** | FFI bindings | `ffi/*` (TypeScript) | Type-safe wrappers around every Rust shared library |
| **Rust** | App shell | Tauri v2 (`apps/marauder/src-tauri/`) | Window, webview, system tray, IPC |
| **Rust** | PTY | `pkg/pty` (`portable-pty`) | Cross-platform pseudoterminal I/O |
| **Rust** | VT parser | `pkg/parser` (`vte`) | Zero-alloc ANSI/VT escape sequence state machine |
| **Rust** | Grid | `pkg/grid` (custom) | Cell buffer, scrollback, dirty tracking, selection |
| **Rust** | Renderer | `pkg/renderer` (`wgpu` + `cosmic-text`) | GPU-accelerated terminal grid rendering |
| **Rust** | Compute | `pkg/compute` (`wgpu` compute shaders) | GPU-accelerated search, URL detection, highlighting, selection |
| **Rust** | Runtime | `pkg/runtime` | Core runtime logic, lifecycle management |
| **Rust** | Event bus | `pkg/event-bus` | High-perf typed pub/sub across native layers |
| **Rust** | Config store | `pkg/config-store` | Config storage backend (TOML/JSON) |
| **Rust** | IPC | `pkg/ipc` | Unix socket protocol for multiplexer mode |
| **Rust** | Daemon | `pkg/daemon` | Background process management |

## Project Structure

```text
marauder/
├── apps/
│   ├── marauder/                  # Tauri v2 app
│   │   ├── src/                   # Frontend: Vite + TypeScript + HTML/CSS
│   │   │   ├── main.ts            # Frontend entry point
│   │   │   ├── styles.css         # Global styles
│   │   │   └── assets/            # Static assets
│   │   ├── src-tauri/             # Rust backend
│   │   │   ├── src/
│   │   │   │   ├── main.rs        # Binary entry point
│   │   │   │   └── lib.rs         # Tauri builder, commands, plugins
│   │   │   ├── Cargo.toml         # Tauri crate dependencies
│   │   │   ├── tauri.conf.json    # Tauri config (window, build, bundle)
│   │   │   ├── capabilities/      # Permission capabilities
│   │   │   └── icons/             # App icons
│   │   ├── index.html             # Root HTML
│   │   ├── package.json           # Frontend deps (Vite, @tauri-apps/api)
│   │   ├── vite.config.ts         # Vite config (Tauri dev integration)
│   │   └── tsconfig.json          # TypeScript config
│   └── marauder-server/           # Headless multiplexer daemon (Rust)
│
├── pkg/                           # Rust crates (each → rlib + cdylib)
│   ├── pty/                       # PTY management
│   ├── parser/                    # VT/ANSI parser
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
│   ├── pty/                       # @marauder/ffi-pty
│   ├── parser/                    # @marauder/ffi-parser
│   ├── grid/                      # @marauder/ffi-grid
│   ├── renderer/                  # @marauder/ffi-renderer
│   ├── event-bus/                 # @marauder/ffi-event-bus
│   └── config-store/              # @marauder/ffi-config-store
│
├── lib/                           # Deno TypeScript runtime layer
│   ├── shell/                     # Shell engine (completions, history, zones)
│   ├── ui/                        # UI logic (pane management, layout engine)
│   ├── io/                        # I/O handling, stream management
│   └── dev/                       # Development tools, debugging utilities
│
├── extensions/                    # Built-in extensions (TypeScript)
│   ├── theme-default/
│   ├── status-bar/
│   ├── git-integration/
│   ├── search/
│   └── notifications/
│
├── resources/
│   ├── shaders/                   # WGSL shader files
│   ├── fonts/                     # Bundled fallback fonts
│   └── shell-integrations/        # Shell RC snippets (zsh, bash, fish)
│
├── bin/                           # Scripts
│   ├── install.sh
│   ├── marauder.sh
│   └── uninstall.sh
├── docs/
├── Cargo.toml                     # Rust workspace manifest
└── deno.json                      # Deno workspace config + import map
```

## Execution Modes

### 1. Tauri App (Primary — Distribution)

```bash
cargo tauri dev     # Development with Vite HMR
cargo tauri build   # Production binary
```

Single native binary. Tauri manages the window, webview renders UI chrome via Vite, wgpu renders the terminal grid, `deno_core` runs the TypeScript runtime.

### 2. Deno-Driven (Development / Hackable)

```bash
deno task dev
```

Deno is the process. It loads Rust shared libraries via `Deno.dlopen()` from `ffi/`. No Tauri, no webview. For hacking on the runtime, testing extensions, and building alternative UIs.

### 3. Headless Server (Multiplexer)

```bash
marauder-server
```

Rust binary, no GUI. Manages PTY sessions over IPC. Clients attach/detach.

## Target Platforms

- **Linux**: X11 and Wayland (primary development target)
- **macOS**: Cocoa/AppKit via Tauri
- **Windows**: Win32 + ConPTY via Tauri
- **WebAssembly**: Future target via wgpu WebGPU backend

## Inspirations

| Project | What we learn |
|---------|--------------|
| **Alacritty** | Minimal, fast Rust core with clean VTE/grid separation |
| **WezTerm** | Scripting integration, multiplexer, shell integration |
| **Warp** | Hybrid native GPU + web UI, block-based terminal |
| **Zellij** | WASM plugin sandboxing, client-server threading |
| **Ghostty** | Shell integration protocol, semantic zones, C API library |
| **VS Code** | TypeScript extension model, marketplace |
| **Tabby** | Webview terminal (xterm.js), plugin system |
| **Deno** | FFI patterns, permission model, TypeScript-native runtime |
| **Tauri** | Rust + webview hybrid apps, IPC, permission system |
