# Marauder Roadmap

## Phase 1: Foundation — Rust Crates + FFI + Tauri Shell

**Goal:** Rust shared libraries, Deno FFI bindings, and a Tauri window showing a PTY session.

### Rust Crates (`pkg/`)

- [ ] Set up Cargo workspace with `rlib + cdylib` targets
- [ ] `pkg/event-bus` — typed pub/sub with C ABI + `#[op2]` exports
- [ ] `pkg/pty` — `portable-pty` wrapper with C ABI + `#[op2]` + `#[tauri::command]`
- [ ] `pkg/parser` — `vte`-based parser with callback C ABI + `#[op2]`
- [ ] `pkg/grid` — cell grid with C ABI + `#[op2]` read access
- [ ] `pkg/renderer` — `wgpu` + `cosmic-text` with C ABI + internal API
- [ ] `pkg/compute` — wgpu compute shaders (search, URL detect, highlights) sharing device with renderer
- [ ] `pkg/runtime` — core runtime lifecycle, bootstrapping
- [ ] `pkg/config-store` — TOML config read/write/watch
- [ ] `pkg/daemon` — background process management
- [ ] `pkg/ipc` — Unix socket transport skeleton

### FFI Bindings (`ffi/`)

- [ ] `ffi/pty/` — `Pty` class wrapping `libmarauder_pty`
- [ ] `ffi/parser/` — `Parser` class wrapping `libmarauder_parser`
- [ ] `ffi/grid/` — `Grid` class wrapping `libmarauder_grid`
- [ ] `ffi/renderer/` — `Renderer` class wrapping `libmarauder_renderer`
- [ ] `ffi/compute/` — `ComputeEngine` class wrapping `libmarauder_compute`
- [ ] `ffi/event-bus/` — `EventBus` class wrapping `libmarauder_event_bus`
- [ ] `ffi/config-store/` — `ConfigStore` class wrapping `libmarauder_config_store`

### Tauri App (`apps/marauder/`)

- [ ] Tauri v2 project (already scaffolded)
- [ ] Frameless window with transparent webview
- [ ] wgpu surface on raw window handle (behind webview)
- [ ] Basic Tauri commands: `create_pane`, `write_pty`, `resize_pane`
- [ ] Tauri `Channel` for streaming PTY events to webview

### Deno Runtime Skeleton (`lib/`)

- [ ] `lib/io/` — I/O types, stream utilities
- [ ] `lib/dev/` — debug utilities
- [ ] `deno.json` workspace config with import map
- [ ] Integration test: FFI opens PTY → writes "echo hello" → reads output

**Milestone:** `cargo tauri dev` opens a window with a working shell session rendered via wgpu.

## Phase 2: Pipeline Wiring — Deno Orchestrates

**Goal:** Deno runtime drives the full pipeline; webview shows basic chrome.

### Deno Runtime (`lib/`)

- [ ] `lib/io/pipeline.ts` — wire PTY → parser → grid → render via FFI/ops
- [ ] `lib/ui/mod.ts` — pane lifecycle, keybinding resolver
- [ ] Frame loop: Deno drives render ticks
- [ ] Input path: key events → keybinding resolution → VT encode → PTY write

### Config System

- [ ] Typed config schema
- [ ] Layered resolution (defaults → system → user → project → CLI)
- [ ] Support both `config.toml` and `config.ts`
- [ ] Live reload via file watcher

### Frontend (`apps/marauder/src/`)

- [ ] Transparent body, opaque tab bar placeholder
- [ ] IPC wrapper module
- [ ] Custom drag region for frameless window

**Milestone:** Full terminal session driven by Deno, basic tab bar in webview.

## Phase 3: Terminal Completeness

**Goal:** Feature parity with standard terminal emulators.

### Rust (Hot Path — `pkg/parser`, `pkg/grid`, `pkg/renderer`)

- [ ] Full VT520 sequence support (CSI, OSC, DCS, PM, APC)
- [ ] 256-color and true color (24-bit)
- [ ] Cell attributes: bold, italic, underline, strikethrough, blink, dim, inverse
- [ ] Alternate screen buffer
- [ ] Mouse tracking (SGR encoding)
- [ ] Wide characters (CJK double-width)
- [ ] Scrollback ring buffer
- [ ] Cursor rendering (block, underline, bar) + blink
- [ ] Selection highlighting overlay
- [ ] URL/hyperlink underline decoration

### Deno (Policy — `lib/ui/`, `lib/shell/`)

- [ ] Selection: mouse drag → grid select → clipboard
- [ ] Scrollback navigation (keybinding → grid scroll)
- [ ] Resize handling (window → recalculate → grid + PTY resize)
- [ ] URL detection → clickable
- [ ] Clipboard integration

**Milestone:** vim, htop, tmux render correctly.

## Phase 4: Extension System

**Goal:** TypeScript extensions with full runtime access.

### Extension Manager

- [ ] `ExtensionManifest`, `ExtensionContext` types
- [ ] Discovery, loading, manifest validation
- [ ] Extension API: events, keybinds, commands, status bar, notifications, palette
- [ ] Hot-reload: file watcher → unload → reload
- [ ] Error isolation: extension crash doesn't affect core

### Extension ↔ Webview Bridge

- [ ] Extensions update webview UI via Tauri Channel
- [ ] Extensions register custom webview panels

### Distribution

- [ ] `extension.json` manifest spec
- [ ] Local loading from `~/.config/marauder/extensions/`
- [ ] Git-based install (`marauder ext install github:user/repo`)

**Milestone:** TS extension hooks shell events, adds keybinding, updates status bar.

## Phase 5: Shell Engine (`lib/shell/`)

**Goal:** Rich shell integration in TypeScript.

- [ ] `lib/shell/zones.ts` — OSC 133 semantic zone tracker
- [ ] `lib/shell/history.ts` — command history with search
- [ ] `lib/shell/completions.ts` — extensible tab completion
- [ ] `lib/shell/prompt.ts` — prompt detection + metadata
- [ ] Shell integration scripts: `resources/shell-integrations/{zsh,bash,fish}`
- [ ] Auto-injection: detect shell → source integration
- [ ] Jump between prompts, exit code display, CWD tracking
- [ ] Command palette (Ctrl+Shift+P) in webview

**Milestone:** Navigate between commands, exit codes, tab completion.

## Phase 6: Window Management (`lib/ui/` + webview)

**Goal:** Tabs and panes.

### Deno (`lib/ui/`)

- [ ] `lib/ui/panes.ts` — PaneManager: create, split, close, focus, resize
- [ ] `lib/ui/tabs.ts` — TabManager: create, close, rename, reorder
- [ ] `lib/ui/layout.ts` — tree-based layout engine
- [ ] Each pane owns PTY + parser + grid (via FFI/ops)
- [ ] Session save/restore (layout → JSON)

### Webview (`apps/marauder/src/`)

- [ ] Tab bar component
- [ ] Status bar with extension segments
- [ ] Pane borders via renderer
- [ ] Click-to-focus, keyboard navigation

**Milestone:** Multiple panes and tabs, keyboard-driven navigation.

## Phase 7: Built-in Extensions (`extensions/`)

**Goal:** Prove the extension system with useful defaults.

- [ ] `extensions/theme-default/` — Catppuccin, Dracula, Solarized, Nord
- [ ] `extensions/status-bar/` — CWD, git branch, time, battery
- [ ] `extensions/git-integration/` — branch display, status indicators
- [ ] `extensions/search/` — Ctrl+F in-terminal search
- [ ] `extensions/notifications/` — desktop notifications on long commands

**Milestone:** Useful out-of-the-box.

## Phase 8: Distribution + Polish

**Goal:** Single-binary distribution, visual polish.

### Distribution

- [ ] `cargo tauri build` native installers
- [ ] macOS: `.dmg`, Homebrew
- [ ] Linux: AppImage, `.deb`, `.rpm`, AUR
- [ ] Windows: `.msi`, Scoop/Winget
- [ ] Bundled TypeScript (deno_core snapshot)
- [ ] `bin/install.sh` and `bin/uninstall.sh`

### Rendering Polish

- [ ] Font ligature support
- [ ] Subpixel antialiasing
- [ ] Window transparency / blur
- [ ] Smooth scrolling
- [ ] Sixel / iTerm2 image protocol
- [ ] Custom WGSL shaders via extensions
- [ ] Adaptive frame rate

**Milestone:** Polished, distributable terminal.

## Phase 9: Multiplexer (`apps/marauder-server/`)

**Goal:** tmux-like session persistence.

- [ ] `apps/marauder-server/` — headless daemon
- [ ] `pkg/ipc/` — Unix socket protocol
- [ ] `pkg/daemon/` — process supervision
- [ ] Attach/detach, session list
- [ ] Session persistence, multi-client

**Milestone:** Detach, reattach, sessions survive close.

## Phase 10: Ecosystem

**Goal:** Community extension marketplace.

- [ ] Extension registry + CLI (`marauder ext install/search/publish`)
- [ ] API versioning + stability guarantees
- [ ] Template generator (`marauder ext create`)
- [ ] Documentation site
- [ ] Example gallery

**Milestone:** Thriving extension ecosystem.

## Future Exploration

- **WebAssembly**: Browser terminal via wgpu WebGPU + Deno WASM
- **Custom shell**: Nushell-like shell in Deno
- **AI integration**: LLM completions, error explanation (extensions)
- **Collaborative terminals**: Shared sessions via IPC
- **Recording/replay**: asciinema-compatible
- **Accessibility**: Screen reader, high contrast
- **Mobile**: Touch terminal via wgpu
