# Marauder

A GPU-accelerated, fully extensible terminal emulator built with **Rust**, **Deno**, and **Tauri**.

> Rust builds the engine. Deno drives the car. Tauri wraps the cockpit.

## Highlights

- **GPU-rendered terminal grid** via wgpu — instanced rendering at 120fps, <1ms CPU/frame
- **GPU compute shaders** for text search, URL detection, and pattern highlighting (zero-copy from the render buffer)
- **Rich shell integration** — OSC 133/7 command zones, prompt navigation, fuzzy history search, tab completions
- **Extension system** — TypeScript extensions with full runtime access, hot-reload, sandboxed isolation, and UI panels
- **Cross-platform** — macOS (Metal), Linux (Vulkan), Windows (DX12) via wgpu backend auto-detection

## Architecture

```
Layer 3: Tauri Webview   (apps/marauder/src/)      — UI chrome (tabs, status bar, command palette)
Layer 2: Deno Runtime    (lib/ + ffi/)             — orchestration, config, extensions, shell logic
Layer 1: Rust Native     (pkg/ + apps/src-tauri/)  — PTY, parser, grid, GPU renderer, compute, event bus
```

The hot path (**PTY read -> VT parse -> grid update -> GPU render**) stays entirely in Rust and never crosses into Deno or the webview. Deno is notified asynchronously via the event bus for policy decisions. The webview body is transparent — wgpu renders the terminal grid underneath.

## Prerequisites

- **Rust** 1.80+ — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Deno** 2.x+ — `curl -fsSL https://deno.land/install.sh | sh`
- **Tauri CLI** — `cargo install tauri-cli --version "^2"`
- **Platform deps:**
  - **macOS:** Xcode Command Line Tools (`xcode-select --install`)
  - **Linux:** `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev libwayland-dev libxkbcommon-dev pkg-config cmake`
  - **Windows:** Visual Studio Build Tools with C++ workload, WebView2

## Quick Start

```bash
# Clone
git clone https://github.com/LayerDynamics/marauder.git
cd marauder

# Build all Rust crates
cargo build

# Run in Tauri dev mode (Vite HMR + Rust hot-reload)
cargo tauri dev

# Or run the standalone Deno mode (FFI bindings)
deno task dev
```

## Build

```bash
cargo build                          # Debug build (all crates)
cargo build --release                # Release build
cargo tauri build                    # Production app binary
cargo test                           # Run all Rust tests
cargo test -p marauder-grid          # Test a specific crate
deno task test                       # Run Deno/TypeScript tests
deno task fmt                        # Format TypeScript
deno task lint                       # Lint TypeScript
```

## Project Structure

```
apps/marauder/src/             Frontend — Vite + TypeScript webview (tabs, status bar, command palette)
apps/marauder/src-tauri/       Tauri Rust backend — commands, deno_core embed, wgpu init
apps/marauder-server/          Headless multiplexer daemon
pkg/event-bus/                 Typed pub/sub event system
pkg/pty/                       PTY management (portable-pty)
pkg/parser/                    VT/ANSI parser (vte)
pkg/grid/                      Terminal cell grid + scrollback + dirty tracking
pkg/renderer/                  GPU renderer (wgpu + cosmic-text, instanced rendering)
pkg/compute/                   GPU compute pipelines (search, URL detect, highlights)
pkg/runtime/                   Core runtime lifecycle
pkg/config-store/              TOML/JSON config with file watching + layered resolution
pkg/ipc/                       Unix socket transport (multiplexer)
pkg/daemon/                    Background process management
ffi/                           Deno FFI bindings — one TypeScript module per Rust crate
lib/shell/                     Shell engine — OSC zones, history, completions, prompt tracking
lib/ui/                        UI logic — pane/tab management, keybindings, action dispatch
lib/io/                        I/O pipeline, stream management
lib/extensions/                Extension registry, loader, isolation, hot-reload
lib/dev/                       Logging + debug helpers
extensions/                    Bundled extensions (themes, status bar, git, search, notifications)
resources/shaders/             WGSL shader files (background, text, cursor, selection, overlay)
resources/fonts/               Bundled fallback fonts
resources/shell-integrations/  Shell RC snippets (zsh, bash, fish)
```

## Extensions

Extensions are TypeScript packages in `extensions/` with an `extension.json` manifest and a `mod.ts` entry point. They receive an `ExtensionContext` with access to:

- **Events** — subscribe/emit on the typed event bus
- **Config** — read/write scoped configuration
- **Status bar** — set left/center/right segments
- **Commands** — register commands available in the command palette
- **Keybindings** — bind key sequences to commands
- **Notifications** — show desktop notifications

```typescript
import type { ExtensionContext } from "@marauder/extensions";

export function activate(ctx: ExtensionContext) {
  ctx.events.on("shell:cwd-changed", (payload) => {
    ctx.statusBar.set("left", payload.cwd);
  });

  ctx.commands.register("my-extension.hello", () => {
    ctx.notifications.show("Hello", "From my extension!");
  });
}

export function deactivate() {}
```

## Key Design Decisions

- Every `pkg/*` crate builds as both `rlib` (for the Tauri binary) and `cdylib` (for Deno FFI)
- FFI uses opaque handle pointers (`Box::into_raw` / `Box::from_raw`) — Rust types never cross the boundary directly
- Tauri `Channel<T>` (not `emit`) for high-throughput PTY streaming
- `deno_core` runs on a separate tokio `current_thread` runtime (V8 is single-threaded)
- The glyph atlas uses `cosmic-text` for CPU rasterization into GPU textures (`R8Unorm` for text, `Rgba8Unorm` for emoji)
- Compute shaders read the same GPU cell buffer as the renderer (zero-copy, `STORAGE | VERTEX` usage flags)

## Documentation

- [Overview](docs/Overview.md) — vision, design principles, and project goals
- [Architecture](docs/Architecture.md) — three-layer model, data flow, GPU pipeline
- [Development](docs/Development.md) — setup, build commands, testing, contributing
- [Roadmap](docs/Roadmap.md) — implementation phases and progress

## License

MIT
