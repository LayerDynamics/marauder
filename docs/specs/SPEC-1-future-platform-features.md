# SPEC-1: Marauder Future Platform Features

**Status:** Draft
**Date:** 2026-03-07
**Scope:** 7 feature areas spanning AI integration, WebAssembly, collaborative terminals, custom shell, recording/replay, accessibility, and mobile (deferred)

---

## Background

### Problem Statement

Marauder Phases 1–10 deliver a GPU-accelerated, extensible terminal emulator with a Rust + Deno + Tauri architecture. The core terminal is functionally complete: PTY management, VT parsing, GPU rendering (wgpu), GPU compute (search/URL detection/highlighting), shell integration, extension system with CLI/registry, pane/tab management, multiplexer daemon, and distribution packaging.

However, the terminal landscape is evolving. Users expect:
- **AI assistance** directly in the terminal — command suggestions, error explanations, autonomous agents
- **Browser access** — terminals in web IDEs, cloud development environments, shareable sessions
- **Collaboration** — pair programming with shared terminal sessions
- **Modern shell UX** — structured data pipelines, rich completions, inline help
- **Session capture** — full recording/replay with metadata for debugging and knowledge sharing
- **Accessibility** — screen reader support, high contrast, motion controls

### What Exists Today

- Phases 1–10 are complete and audited
- Extension system supports TypeScript plugins with config, events, commands, keybindings, panels, notifications
- GPU renderer uses wgpu (Vulkan/Metal/DX12) with instanced rendering at 120fps
- GPU compute shaders handle search, URL detection, highlighting
- Multiplexer daemon (`marauder-server`) manages sessions over IPC
- Shell engine tracks zones, history, completions via OSC 133

### Why Now

The extension system (Phase 4/10) provides the foundation for AI and collaboration features. wgpu's WebGPU backend is mature enough for browser deployment. The 3-layer architecture (Rust → Deno → Tauri) cleanly separates concerns, making it possible to swap the Tauri webview for a browser canvas without touching rendering or shell logic.

### Existing Foundations (Built in Phases 1–10)

Several features have partial implementations that reduce scope:

| Area | What Already Exists | Remaining Work |
|------|-------------------|----------------|
| **Shell engine** | Full ShellEngine, CommandHistory, CompletionEngine, PromptTracker, zones, shell inject | Syntax highlighting in prompt, inline help, structured output detection |
| **Accessibility** | `ariaTerminalAttrs()`, `prefersReducedMotion()`, `prefersHighContrast()`, `announceToScreenReaders()` | Keyboard navigation, focus management, high contrast theme, axe-core audit |
| **Recording** | Command history (ring buffer), zone tracking, prompt tracker with exit codes | Raw PTY capture, replay engine, asciinema export, data masking |
| **Multiplexer** | `marauder-server` with PTY sessions, `pkg/ipc` Unix socket transport | WebSocket listener, collaboration protocol |

### Key Assumptions

- WebGPU is available in target browsers (Chrome 113+, Firefox 121+, Safari 18+)
- OpenAI-compatible API is the de facto standard for LLM access (works with OpenAI, Anthropic via proxy, Ollama, vLLM, etc.)
- The existing extension system is stable enough to build AI and collaboration as extensions
- wgpu's WASM target produces viable performance for terminal workloads

---

## Requirements

### Functional Requirements

#### AI Integration — **Must Have**

| ID | Requirement | Priority |
|----|-------------|----------|
| AI-1 | Inline command suggestions from LLM based on terminal context (current dir, recent commands, shell output) | Must |
| AI-2 | Error explanation: detect non-zero exit codes, offer LLM-powered explanation and fix suggestions | Must |
| AI-3 | Chat panel: in-terminal conversational AI accessible via command palette or keybinding | Must |
| AI-4 | Autonomous agent mode: AI reads terminal output, proposes and executes commands with user approval | Must |
| AI-5 | Provider-agnostic LLM interface supporting any OpenAI-compatible API endpoint | Must |
| AI-6 | Context window management: send relevant terminal history, CWD, env vars, git status to LLM | Must |
| AI-7 | Streaming responses: display LLM output token-by-token in chat panel | Should |
| AI-8 | Cost tracking: display token usage and estimated cost per session | Should |
| AI-9 | Local model support via Ollama/llama.cpp endpoints | Should |
| AI-10 | AI-assisted command history search (semantic, not just substring) | Could |

#### WebAssembly / Browser Terminal — **Must Have**

| ID | Requirement | Priority |
|----|-------------|----------|
| WA-1 | Compile Marauder renderer + grid + parser to WASM via wgpu WebGPU backend | Must |
| WA-2 | Browser standalone: run Marauder in a browser tab connecting to remote PTY server | Must |
| WA-3 | Embeddable widget: `<marauder-terminal>` web component for web IDEs | Must |
| WA-4 | WebSocket transport: connect browser terminal to `marauder-server` PTY sessions | Must |
| WA-5 | Feature parity with native: same rendering, same GPU compute, same extension system | Must |
| WA-6 | Font loading: web font fallback when system fonts unavailable | Should |
| WA-7 | Clipboard integration via Clipboard API | Should |
| WA-8 | File upload/download via drag-and-drop | Could |
| WA-9 | PWA support: installable, offline-capable (with local shell via WebContainer or similar) | Could |

#### Collaborative Terminals — **Must Have**

| ID | Requirement | Priority |
|----|-------------|----------|
| CO-1 | Session sharing: invite others to a live terminal session via link | Must |
| CO-2 | Shared cursor: multiple users can type into the same session simultaneously | Must |
| CO-3 | Presence indicators: show who is connected, cursor positions, who is typing | Must |
| CO-4 | Latency compensation: operational transform or CRDT for concurrent input | Must |
| CO-5 | Connection via `marauder-server` IPC (local) or WebSocket (remote) | Must |
| CO-6 | User identity: display names, avatar colors per participant | Should |
| CO-7 | Chat sidebar for text communication between participants | Should |
| CO-8 | Follow mode: auto-scroll to another user's cursor | Could |

#### Custom Shell — **Should Have**

| ID | Requirement | Priority |
|----|-------------|----------|
| SH-1 | Smart prompt layer wrapping existing shells (zsh/bash/fish) with enhanced UX | Must |
| SH-2 | Structured output: parse command output into tables, JSON, lists when detectable | Must |
| SH-3 | Syntax highlighting in the prompt input line | Must |
| SH-4 | Inline help: show command documentation/flags as you type | Should |
| SH-5 | Optional built-in structured shell mode (Nushell-inspired) with typed data pipelines | Should |
| SH-6 | Pipeline visualization: show data flow through piped commands | Could |
| SH-7 | Custom shell language with Deno runtime for scripting | Could |

#### Recording / Replay — **Should Have**

| ID | Requirement | Priority |
|----|-------------|----------|
| RE-1 | Full session recording: capture all PTY input/output with timestamps | Must |
| RE-2 | Rich metadata: commands, zones, exit codes, AI interactions, extension state | Must |
| RE-3 | Asciinema export: convert recordings to standard .cast format for sharing | Must |
| RE-4 | Replay in Marauder: play back sessions at variable speed with seeking | Must |
| RE-5 | Custom recording format with Marauder-specific metadata | Should |
| RE-6 | Recording controls: start/stop/pause via keybinding or extension API | Should |
| RE-7 | Annotations: add text markers during recording for bookmarks | Could |
| RE-8 | Web replay: play recordings in the browser WASM version | Could |

#### Accessibility — **Must Have**

| ID | Requirement | Priority |
|----|-------------|----------|
| AC-1 | Screen reader support: ARIA live regions announcing terminal output | Must |
| AC-2 | High contrast theme: WCAG AA contrast ratios (4.5:1 minimum) | Must |
| AC-3 | Reduced motion: disable/reduce all animations (cursor blink, smooth scroll) | Must |
| AC-4 | Keyboard-only navigation: all UI elements reachable without mouse | Must |
| AC-5 | Focus management: visible focus indicators, logical tab order | Must |
| AC-6 | Configurable cursor size and style for visibility | Should |
| AC-7 | Font size scaling without layout breakage | Should |
| AC-8 | Color blind safe palette option | Could |

#### Mobile — **Deferred**

| ID | Requirement | Priority |
|----|-------------|----------|
| MO-1 | Touch terminal via wgpu on iOS/Android | Won't (this cycle) |
| MO-2 | On-screen keyboard with terminal shortcuts | Won't (this cycle) |
| MO-3 | Gesture controls (swipe for scroll, pinch for zoom) | Won't (this cycle) |

*Mobile is documented for vision alignment but excluded from the 3–6 month implementation window.*

### Non-Functional Requirements

| ID | Requirement | Target | Priority |
|----|-------------|--------|----------|
| NF-1 | WASM build: 60fps rendering in Chrome/Firefox/Safari | 60fps sustained | Must |
| NF-2 | WASM first frame: <100ms from page load to rendered terminal | <100ms | Must |
| NF-3 | WASM bundle size: <5MB gzipped (renderer + grid + parser) | <5MB gz | Should |
| NF-4 | AI response latency: first token visible within 500ms of request | <500ms TTFT | Should |
| NF-5 | Collaboration latency: <100ms input-to-display for all participants | <100ms p95 | Must |
| NF-6 | Recording overhead: <5% CPU impact during active recording | <5% CPU | Should |
| NF-7 | Accessibility: pass automated WCAG 2.1 AA audit for webview chrome | AA pass | Must |
| NF-8 | Native performance unaffected: AI/collab features must not degrade core 120fps rendering | 0 regression | Must |

### Security and Compliance Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| SE-1 | LLM API keys stored in OS keychain / encrypted config, never in plaintext | Must |
| SE-2 | AI context: user controls what terminal data is sent to LLM (opt-in per session) | Must |
| SE-3 | Collaborative sessions: TLS encryption for all WebSocket connections | Must |
| SE-4 | Session sharing: time-limited invite links with revocation | Must |
| SE-5 | Agent mode: explicit user approval before every command execution | Must |
| SE-6 | WASM: CSP-compatible, no eval(), no unsafe-inline | Must |
| SE-7 | Recording: sensitive data masking (detect and redact passwords, tokens, secrets) | Should |

### Data Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| DA-1 | AI conversation history: stored locally per session, optionally persisted | Should |
| DA-2 | Recordings: stored in `~/.config/marauder/recordings/` with index | Must |
| DA-3 | Collaboration state: ephemeral (in-memory on server), no persistent storage | Must |
| DA-4 | User preferences for all new features: stored in existing config-store | Must |

### Integration Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| IN-1 | OpenAI-compatible API: any endpoint accepting `/v1/chat/completions` | Must |
| IN-2 | WebSocket protocol: standard RFC 6455 for browser ↔ server communication | Must |
| IN-3 | Asciinema: `.cast` v2 format compatibility for recording export | Must |
| IN-4 | Web component: standard custom elements API (`<marauder-terminal>`) | Must |
| IN-5 | VS Code extension host: embeddable via webview panel | Should |

### Operational Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| OP-1 | WASM builds in CI: automated wasm-pack / wasm-bindgen compilation | Must |
| OP-2 | CDN deployment for WASM assets | Should |
| OP-3 | Collaboration server monitoring: connection count, latency metrics | Should |

### Delivery Constraints

- **Timeline:** 3–6 months for priority features (AI, WebAssembly, Collaboration)
- **Team size:** Solo developer + AI pair programming
- **Hosting:** Self-hosted collaboration server; WASM assets on CDN
- **Budget:** Minimal — prefer open-source tooling, pay-as-you-go LLM APIs

---

## Method

### 1. System Architecture Overview

All 7 features integrate into the existing 3-layer architecture without modifying the core rendering hot path. AI and recording are implemented as extensions. WebAssembly replaces the Tauri shell with a browser canvas. Collaboration extends `marauder-server` with WebSocket support and presence tracking.

```text
┌─────────────────────────────────────────────────────────────────┐
│  Presentation Layer                                              │
│  ┌──────────────┐  ┌────────────────┐  ┌─────────────────────┐  │
│  │ Tauri Webview │  │ Browser Canvas │  │ Embedded Component  │  │
│  │  (native)    │  │  (WASM)        │  │  (web IDE widget)   │  │
│  └──────┬───────┘  └───────┬────────┘  └──────────┬──────────┘  │
├─────────┴──────────────────┴───────────────────────┴────────────┤
│  Runtime Layer (Deno / WASM)                                     │
│  ┌──────────┐ ┌──────────┐ ┌───────────┐ ┌────────────────────┐ │
│  │ Shell    │ │ AI Ext   │ │ Collab    │ │ Recording Ext      │ │
│  │ Engine   │ │ Engine   │ │ Client    │ │                    │ │
│  └──────────┘ └──────────┘ └───────────┘ └────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│  Native Layer (Rust / WASM)                                      │
│  ┌─────┐ ┌────────┐ ┌──────┐ ┌──────────┐ ┌─────────┐          │
│  │ PTY │ │ Parser │ │ Grid │ │ Renderer │ │ Compute │          │
│  └─────┘ └────────┘ └──────┘ └──────────┘ └─────────┘          │
├─────────────────────────────────────────────────────────────────┤
│  Server Layer (Rust)                                             │
│  ┌──────────────────┐ ┌────────────────────────────────────────┐ │
│  │ marauder-server  │ │ Collaboration Server (WebSocket)       │ │
│  │ (PTY sessions)   │ │ (presence, OT/CRDT, relay)             │ │
│  └──────────────────┘ └────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### 2. Architectural Style and Rationale

**Extension-first:** AI integration, recording, and custom shell features are built as extensions using the existing `ExtensionContext` API. This keeps the core lightweight, allows independent updates, and lets users opt in/out.

**Compilation-target polymorphism:** The Rust `pkg/*` crates compile to both native (`cdylib`) and WASM (`wasm32-unknown-unknown`). The renderer switches between native wgpu and WebGPU at compile time via `#[cfg(target_arch = "wasm32")]`. No runtime feature detection.

**Server-mediated collaboration:** All collaborative features route through `marauder-server`. The server holds the authoritative PTY session; clients send input and receive output via WebSocket. Conflict resolution uses operational transform on the input stream.

### 3. Component Responsibilities

| Component | New Responsibility |
|-----------|-------------------|
| `extensions/ai/` | LLM client, context assembly, chat panel, suggestion engine, agent mode |
| `extensions/recording/` | Session capture, replay engine, asciinema export |
| `extensions/collab/` | Collaboration client (presence, cursor sharing, input relay) |
| `extensions/shell-enhanced/` | Smart prompt layer, structured output, inline help |
| `extensions/a11y/` | Screen reader bridge, high contrast, reduced motion |
| `pkg/renderer` | WASM compilation target, WebGPU backend |
| `pkg/grid` | WASM compilation target |
| `pkg/parser` | WASM compilation target |
| `marauder-server` | WebSocket listener, collaboration session management, OT engine |
| `apps/marauder-web/` | Browser entry point, canvas setup, WebSocket client |

### 4. Data Design and Schema Model

#### AI Conversation

```typescript
interface AIConversation {
  id: string;
  sessionId: string;           // terminal session this is attached to
  messages: AIMessage[];
  provider: string;            // "openai", "ollama", etc.
  model: string;               // "gpt-4", "claude-3", etc.
  tokenUsage: { prompt: number; completion: number; total: number };
  createdAt: number;
}

interface AIMessage {
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: number;
  context?: TerminalContext;   // snapshot of terminal state when message was sent
}

interface TerminalContext {
  cwd: string;
  lastCommand: string;
  lastExitCode: number;
  recentOutput: string;        // last N lines of terminal output
  env: Record<string, string>; // filtered subset
  gitBranch?: string;
}
```

#### Recording Format

```typescript
interface MarauderRecording {
  version: 1;
  metadata: {
    title: string;
    shell: string;
    term: string;
    cols: number;
    rows: number;
    startedAt: number;
    duration: number;
    env: Record<string, string>;
  };
  events: RecordingEvent[];
}

type RecordingEvent =
  | { t: number; type: "output"; data: string }
  | { t: number; type: "input"; data: string }
  | { t: number; type: "resize"; cols: number; rows: number }
  | { t: number; type: "command"; cmd: string; exitCode?: number }
  | { t: number; type: "zone"; zone: "prompt" | "input" | "output"; id: number }
  | { t: number; type: "ai"; role: string; content: string }
  | { t: number; type: "annotation"; text: string };
```

#### Collaboration Session

```typescript
interface CollabSession {
  id: string;
  ptySessionId: string;        // underlying marauder-server session
  host: string;                // user who created the session
  participants: CollabParticipant[];
  createdAt: number;
  inviteToken: string;         // short-lived, revocable
  expiresAt: number;
}

interface CollabParticipant {
  id: string;
  name: string;
  color: string;               // cursor/indicator color
  role: "driver" | "viewer";   // shared cursor = both can be "driver"
  connectedAt: number;
  cursorPosition?: { row: number; col: number };
}
```

### 5. API and Interface Design

#### AI Extension API (exposed via `ExtensionContext`)

```typescript
interface AIExtensionAPI {
  // Configuration
  configure(opts: { endpoint: string; apiKey: string; model: string }): void;

  // Suggestions
  suggest(context: TerminalContext): Promise<string[]>;

  // Error explanation
  explainError(command: string, exitCode: number, output: string): Promise<string>;

  // Chat
  chat(message: string, context?: TerminalContext): AsyncIterable<string>;

  // Agent mode
  proposeAction(goal: string, context: TerminalContext): Promise<AgentAction>;
  executeAction(action: AgentAction): Promise<ActionResult>;
}

interface AgentAction {
  command: string;
  explanation: string;
  risk: "safe" | "moderate" | "destructive";
  requiresApproval: boolean;
}
```

#### WebSocket Protocol (Browser ↔ Server)

```typescript
// Client → Server
type ClientMessage =
  | { type: "auth"; token: string }
  | { type: "input"; data: Uint8Array }
  | { type: "resize"; cols: number; rows: number }
  | { type: "cursor"; row: number; col: number }
  | { type: "ping" };

// Server → Client
type ServerMessage =
  | { type: "output"; data: Uint8Array }
  | { type: "resize"; cols: number; rows: number }
  | { type: "participant_joined"; participant: CollabParticipant }
  | { type: "participant_left"; id: string }
  | { type: "cursor_update"; id: string; row: number; col: number }
  | { type: "pong" };
```

#### Web Component API

```html
<marauder-terminal
  server="wss://example.com/ws"
  token="abc123"
  font-family="JetBrains Mono"
  font-size="14"
  theme="catppuccin-mocha"
></marauder-terminal>
```

```typescript
class MarauderTerminal extends HTMLElement {
  connect(url: string, token: string): Promise<void>;
  disconnect(): void;
  write(data: string | Uint8Array): void;
  resize(cols: number, rows: number): void;
  setTheme(theme: string | ThemeConfig): void;
  addEventListener(event: "connected" | "disconnected" | "output", handler: EventHandler): void;
}
```

### 6. Workflow and Sequence Logic

#### AI Command Suggestion Flow

```text
1. User types partial command in terminal
2. Shell engine detects prompt zone + input
3. AI extension receives "shell:input_changed" event
4. Extension assembles TerminalContext (CWD, history, env, git)
5. Extension calls LLM API: POST /v1/chat/completions
6. Extension renders suggestions as ghost text or dropdown
7. User accepts (Tab) or dismisses (Esc)
8. Accepted suggestion written to PTY
```

#### AI Agent Mode Flow

```text
1. User activates agent via Ctrl+Shift+A or command palette
2. User describes goal in chat panel
3. AI extension proposes first action with risk assessment
4. User approves/edits/rejects
5. If approved: extension writes command to PTY via ctx.terminal.write()
6. Extension monitors output via "TerminalOutput" events
7. Extension waits for prompt (zone change), reads exit code
8. Extension proposes next action or reports completion
9. Loop until goal achieved or user cancels
```

#### Collaboration Session Flow

```text
1. Host: marauder-server creates PTY session
2. Host: collaboration extension generates invite link (time-limited token)
3. Guest: opens link in browser (WASM) or native Marauder
4. Guest: authenticates via WebSocket with invite token
5. Server: relays PTY output to all connected clients
6. Any participant: types → input sent via WebSocket → server writes to PTY
7. Server: applies operational transform for concurrent inputs
8. Server: broadcasts cursor positions to all participants
9. Disconnect: participant removed, others notified
```

### 7. Algorithms and Business Rules

#### Operational Transform for Collaborative Input

Concurrent terminal input from multiple users must be serialized. Since terminal input is a sequential byte stream (not a document), the OT model is simpler than text editing:

1. Each input message carries a sequence number and timestamp
2. Server maintains authoritative sequence
3. Concurrent inputs are ordered by server receipt time (FIFO)
4. Server applies input to PTY in order, broadcasts sequence numbers
5. Clients rebase pending local input against server sequence

This is effectively a distributed queue, not full OT — terminal input is inherently sequential.

#### AI Context Window Management

```text
Context budget: 4096 tokens (configurable)
Allocation:
  - System prompt: ~200 tokens (role, capabilities)
  - Terminal context: ~1000 tokens (CWD, env, git, recent commands)
  - Recent output: ~1500 tokens (last N lines, truncated)
  - Conversation history: ~1000 tokens (sliding window)
  - User message: remaining tokens

Truncation strategy: oldest messages first, preserve system + current context
```

#### Recording Sensitive Data Masking

```text
Patterns to detect and mask:
  - Environment variables: *_KEY, *_SECRET, *_TOKEN, *_PASSWORD
  - Command arguments: --password=*, --token=*, -p *
  - Inline patterns: Bearer *, Basic *, ghp_*, sk-*, AKIA*
  - Interactive prompts: Password:, Enter passphrase:

Masking: replace detected values with [REDACTED]
Mode: opt-in (default off) — user enables via config
```

### 8. Consistency and Transaction Strategy

- **AI state:** Per-session, in-memory. Conversation history optionally persisted to disk on session close. No cross-session consistency needed.
- **Collaboration:** Server is single source of truth for PTY state. Clients are display-only replicas. Input ordering is server-determined (no conflicts).
- **Recording:** Append-only event log. Writes are sequential (single writer). No concurrent access concerns.

### 9. Security Architecture

```text
┌─────────────────────────────────────────────────┐
│  User                                            │
│  ├── API keys → OS keychain (keytar / Secret Service / Keychain Access)
│  ├── Session tokens → in-memory only, never persisted
│  └── Config → ~/.config/marauder/ai.toml (no secrets)
│
│  AI Extension                                    │
│  ├── Reads API key from keychain at activation
│  ├── Sends to LLM endpoint via HTTPS only
│  ├── User controls what context is sent (opt-in)
│  └── Agent mode: explicit approval per command
│
│  Collaboration                                   │
│  ├── TLS for all WebSocket connections
│  ├── Invite tokens: CSPRNG, 32 bytes, time-limited (1 hour default)
│  ├── Token revocation: host can revoke at any time
│  └── Server validates token before granting PTY access
│
│  WASM                                            │
│  ├── CSP: no eval, no unsafe-inline
│  ├── CORS: server whitelist for WebSocket origins
│  └── Sandboxed: WASM has no filesystem/network access beyond WebSocket
└─────────────────────────────────────────────────┘
```

### 10. Reliability and Resilience Design

| Failure Mode | Mitigation |
|-------------|-----------|
| LLM API timeout | 10s timeout, graceful fallback ("AI unavailable"), retry with backoff |
| LLM API error | Display error in chat, continue terminal operation unaffected |
| WebSocket disconnect (collab) | Auto-reconnect with exponential backoff (1s, 2s, 4s, max 30s) |
| WASM crash | Catch panics via `std::panic::catch_unwind`, display error overlay, offer reload |
| Recording file corruption | Append-only with periodic checkpoints, recover from last valid checkpoint |
| Server OOM (many collab sessions) | Per-session memory limit, evict oldest idle sessions |

### 11. Performance and Scalability Approach

| Dimension | Strategy |
|-----------|----------|
| WASM rendering | 60fps target; same instanced rendering as native, WebGPU backend |
| WASM bundle size | Tree-shake unused code; lazy-load glyph atlas; compress with brotli |
| AI latency | Stream responses; show spinner immediately; cancel on user input |
| Collaboration | Server relays raw bytes (no parsing); <100ms p95 for LAN, <200ms WAN |
| Recording | Buffered writes (flush every 100ms); compress idle periods in replay |
| Accessibility | Screen reader bridge runs async; never blocks render path |

### 12. Observability Design

| Signal | What | Where |
|--------|------|-------|
| Metrics | WASM frame time, FPS, bundle load time | Browser Performance API |
| Metrics | AI request latency, token usage, error rate | AI extension internal counters |
| Metrics | Collab session count, participant count, input latency | marauder-server |
| Logs | AI conversation (opt-in), command suggestions | `~/.config/marauder/logs/ai.log` |
| Logs | Collaboration events (join, leave, errors) | marauder-server stdout |
| Traces | Recording event stream (is the trace) | Recording files |

### 13. Infrastructure and Deployment Topology

#### Native (existing)

```text
User machine: Tauri app (single binary)
  ├── pkg/* Rust crates (linked)
  ├── Deno runtime (embedded deno_core)
  └── Extensions loaded from ~/.config/marauder/extensions/
```

#### Browser

```text
CDN: static assets
  ├── marauder.wasm (~3MB gzip)
  ├── marauder.js (wasm-bindgen glue)
  └── index.html + web component bundle

User's server (or cloud): marauder-server
  ├── PTY session management
  ├── WebSocket listener (port 8080)
  └── Collaboration session manager
```

#### Embeddable

```text
npm package: @marauder/terminal
  ├── marauder.wasm
  ├── MarauderTerminal web component
  └── TypeScript types
```

### 14. Tradeoffs and Rejected Alternatives

| Decision | Chosen | Rejected | Why |
|----------|--------|----------|-----|
| AI as extension vs core | Extension | Core feature | Keeps core minimal; users who don't want AI don't pay for it |
| Collaboration transport | WebSocket | WebRTC | WebSocket is simpler, sufficient for terminal byte streams, works through firewalls |
| WASM renderer | wgpu WebGPU | xterm.js / Canvas 2D | Maintains rendering parity with native; same shaders, same quality |
| Collab conflict resolution | Server-ordered FIFO | Full OT/CRDT | Terminal input is sequential; full OT is unnecessary complexity |
| Recording format | Custom + asciinema export | Pure asciinema | Need richer metadata (commands, AI, zones); asciinema as export target |
| Custom shell | Hybrid (smart layer + optional) | Full replacement | Most users want their existing shell; full replacement is multi-year effort |
| Mobile | Deferred | Include now | Touch terminal UX requires significant R&D; better to ship other features first |

### 15. Architecture Diagrams

#### AI Extension Architecture

```text
┌─────────────────────────────────────────────────┐
│  AI Extension (extensions/ai/)                   │
│                                                  │
│  ┌──────────────┐  ┌──────────────────────────┐ │
│  │ Context      │  │ LLM Client               │ │
│  │ Assembler    │──│ (OpenAI-compatible)       │ │
│  │              │  │ POST /v1/chat/completions │ │
│  └──────┬───────┘  └────────────┬─────────────┘ │
│         │                       │                │
│  ┌──────┴───────┐  ┌───────────┴──────────────┐ │
│  │ Suggestion   │  │ Chat Panel               │ │
│  │ Engine       │  │ (webview panel)           │ │
│  └──────────────┘  └──────────────────────────┘ │
│                                                  │
│  ┌──────────────────────────────────────────────┐│
│  │ Agent Mode                                    ││
│  │ propose → approve → execute → observe → loop  ││
│  └──────────────────────────────────────────────┘│
└─────────────────────────────────────────────────┘
     │ ctx.events    │ ctx.commands    │ ctx.panels
     ▼               ▼                ▼
  Event Bus      Command Registry   Panel Registry
```

#### WASM Compilation Architecture

```text
  Native build:                   WASM build:
  ┌────────────┐                  ┌────────────┐
  │ pkg/renderer│                 │ pkg/renderer│
  │ wgpu native │                 │ wgpu WASM  │
  │ (Metal/Vk)  │                 │ (WebGPU)   │
  └──────┬──────┘                 └──────┬──────┘
         │                               │
  ┌──────┴──────┐                 ┌──────┴──────┐
  │ Tauri window│                 │ <canvas>    │
  │ raw handle  │                 │ element     │
  └─────────────┘                 └─────────────┘

  Shared: pkg/grid, pkg/parser, pkg/compute
  (compiled to both targets via #[cfg])
```

---

## Implementation

### Build Phases

#### Phase 11: AI Integration (Weeks 1–4)

| Task | Description | Depends On |
|------|-------------|-----------|
| 11.1 | LLM client library: OpenAI-compatible API client with streaming, retry, key management | — |
| 11.2 | Context assembler: gather CWD, history, env, git, recent output into context window | Shell engine |
| 11.3 | Suggestion engine: ghost text / dropdown for inline command suggestions | 11.1, 11.2 |
| 11.4 | Error explanation: hook `shell:command_finished`, detect failures, offer explanation | 11.1, 11.2 |
| 11.5 | Chat panel: webview panel for conversational AI | 11.1, Panel API |
| 11.6 | Agent mode: propose/approve/execute loop with risk assessment | 11.1, 11.2, 11.5 |
| 11.7 | Configuration: provider selection, model, endpoint, key storage | Config store |
| 11.8 | Token/cost tracking display | 11.1 |

#### Phase 12: WebAssembly Browser Terminal (Weeks 3–7)

| Task | Description | Depends On |
|------|-------------|-----------|
| 12.1 | WASM compilation: `pkg/grid`, `pkg/parser` compile to `wasm32-unknown-unknown` | — |
| 12.2 | WASM renderer: `pkg/renderer` with WebGPU backend (`wgpu` WASM target) | 12.1 |
| 12.3 | WASM compute: `pkg/compute` with WebGPU compute shaders | 12.1 |
| 12.4 | WebSocket PTY client: connect to `marauder-server` from browser | marauder-server |
| 12.5 | Browser entry point: `apps/marauder-web/` with canvas, input handling, clipboard | 12.2, 12.4 |
| 12.6 | `<marauder-terminal>` web component | 12.5 |
| 12.7 | Web font loading + glyph atlas for WASM | 12.2 |
| 12.8 | `marauder-server` WebSocket listener | IPC crate |
| 12.9 | npm package: `@marauder/terminal` | 12.6 |

#### Phase 13: Collaborative Terminals (Weeks 6–9)

| Task | Description | Depends On |
|------|-------------|-----------|
| 13.1 | Collaboration server: WebSocket session management in `marauder-server` | 12.8 |
| 13.2 | Input serialization: server-ordered FIFO for concurrent input | 13.1 |
| 13.3 | Presence protocol: join/leave notifications, cursor positions | 13.1 |
| 13.4 | Collaboration client extension: connect, display participants, relay input | 13.1 |
| 13.5 | Invite system: token generation, time-limited links, revocation | 13.1 |
| 13.6 | Browser collaboration: WASM client with collaboration support | 12.6, 13.4 |
| 13.7 | User identity: names, colors, cursor indicators | 13.3 |

#### Phase 14: Recording + Accessibility + Shell Polish (Weeks 8–12)

**Existing foundations:**
- **Shell engine (DONE):** `lib/shell/` — full ShellEngine, CommandHistory (ring buffer + fuzzy search), CompletionEngine (pluggable providers for history + paths), PromptTracker (zone delimitation, exit codes), SemanticZoneTracker (OSC 133/OSC 7), shell inject scripts (zsh/bash/fish)
- **Accessibility stubs (DONE):** `lib/ui/styling/accessibility.ts` — `ariaTerminalAttrs()`, `prefersReducedMotion()`, `prefersHighContrast()`, `announceToScreenReaders()`
- **Command history (DONE):** `lib/shell/history.ts` — in-memory ring buffer with CWD + exit code tracking

| Task | Description | Depends On |
|------|-------------|-----------|
| 14.1 | ~~Smart prompt layer~~ **DONE** — enhance with syntax highlighting + inline help | Existing shell engine |
| 14.2 | Recording extension: raw PTY byte capture with event metadata (builds on existing PromptTracker/zones) | Event bus |
| 14.3 | Replay engine: variable speed playback with seeking | 14.2 |
| 14.4 | Asciinema export: convert `.mrec` to `.cast` format | 14.2 |
| 14.5 | Sensitive data masking: pattern detection and redaction | 14.2 |
| 14.6 | Accessibility: keyboard-only navigation + focus management (extends existing ARIA stubs) | Webview |
| 14.7 | High contrast theme (extends existing `prefersHighContrast()` detection) | Theme system |
| 14.8 | Reduced motion integration (extends existing `prefersReducedMotion()` detection) | Renderer config |
| 14.9 | Optional structured shell mode (Nushell-inspired) | Existing shell engine |

### Workstreams

| Workstream | Phases | Parallelizable With |
|------------|--------|-------------------|
| AI | 11 | 12 (weeks 3–4 overlap) |
| WebAssembly | 12 | 11 (weeks 3–4), 14 (weeks 8+) |
| Collaboration | 13 | 14 |
| Shell + Recording + A11y | 14 | 13 |

### Dependencies

```text
Phase 11 (AI) ──────────────────────────────────────→ Done
Phase 12 (WASM) ────────────────────────────────────→ Done
Phase 13 (Collab) ──→ depends on 12.8 (WebSocket) ─→ Done
Phase 14 (Shell/Rec/A11y) ─────────────────────────→ Done
```

### Testing Strategy

| Layer | Strategy | Tools |
|-------|----------|-------|
| AI client | Unit tests with mock HTTP server | Deno.test + mock fetch |
| AI context | Unit tests with fixture terminal states | Deno.test |
| WASM compilation | CI build: `cargo build --target wasm32-unknown-unknown` | GitHub Actions |
| WASM rendering | Manual: headless Chrome with WebGPU, screenshot comparison | Playwright |
| WebSocket protocol | Integration: spawn server, connect client, verify message flow | Deno.test |
| Collaboration | Integration: multi-client scenario, verify input ordering | Deno.test |
| Recording | Unit: capture events, verify format; Integration: record + replay | Deno.test |
| Accessibility | Automated: axe-core audit on webview; Manual: VoiceOver/NVDA testing | axe-core |
| Regression | Existing `cargo test` + `deno test` suites must continue passing | CI |

### Rollout Strategy

1. **Alpha (internal):** AI extension behind feature flag, WASM demo on staging URL
2. **Beta (public):** AI + WASM available in release builds, collaboration behind flag
3. **GA:** All features enabled, npm package published, docs complete

Feature flags via config:
```toml
[features]
ai = true
wasm = true           # only relevant for server-side WASM builds
collaboration = false  # beta
recording = true
```

### Operational Readiness

- [ ] CI pipeline: WASM build + test in GitHub Actions
- [ ] CDN setup for WASM assets
- [ ] `marauder-server` systemd service file for collaboration hosting
- [ ] Documentation for self-hosting collaboration server
- [ ] API key setup guide for AI extension
- [ ] Accessibility testing checklist

---

## Milestones

| Milestone | Target | Exit Criteria | Owner |
|-----------|--------|---------------|-------|
| M1: AI MVP | Week 4 | Command suggestions + error explanation working in terminal | — |
| M2: WASM Renders | Week 5 | Terminal renders in Chrome via WebGPU, connects to server PTY | — |
| M3: AI Agent | Week 6 | Agent mode: propose → approve → execute → observe loop functional | — |
| M4: Browser Terminal | Week 7 | Full browser terminal with clipboard, fonts, resize | — |
| M5: Collab MVP | Week 9 | Two users share a terminal session with shared cursor | — |
| M6: Recording | Week 10 | Record session, replay with seeking, export to asciinema | — |
| M7: Accessibility | Week 11 | Keyboard nav complete, high contrast theme, reduced motion (builds on existing ARIA stubs) | — |
| M8: Shell Polish | Week 12 | Prompt syntax highlighting, inline help (builds on existing ShellEngine) | — |
| M9: npm Package | Week 12 | `@marauder/terminal` published, `<marauder-terminal>` documented | — |
| M10: GA | Week 14 | All features stable, docs complete, CI green | — |

---

## Gathering Results

### Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| AI suggestion acceptance rate | >30% of shown suggestions accepted | Extension telemetry (opt-in) |
| AI error explanation usefulness | >70% thumbs-up rating | In-app feedback |
| WASM first frame time | <100ms on Chrome | Browser Performance API |
| WASM FPS | 60fps sustained at 80x24 | `requestAnimationFrame` timing |
| WASM bundle size | <5MB gzipped | CI build output |
| Collaboration latency | <100ms p95 (LAN) | Server metrics |
| Recording file size | <1MB per hour of typical usage | File size measurement |
| Accessibility audit | 0 critical / 0 serious axe-core violations | Automated audit |

### Validation Methods

- **WASM:** Automated Playwright tests in headless Chrome with WebGPU
- **AI:** Manual testing with multiple providers (OpenAI, Ollama, Anthropic proxy)
- **Collaboration:** Multi-client integration test (3+ simultaneous users)
- **Recording:** Round-trip test: record → replay → verify output matches
- **Accessibility:** Automated axe-core + manual VoiceOver (macOS) testing
- **Performance:** Benchmark suite: `cat large_file`, rapid typing, scrollback search

### Post-Production Review Cadence

- **Week 1 post-GA:** Bug triage, hotfix any critical issues
- **Week 4 post-GA:** Usage metrics review, prioritize follow-ups
- **Monthly:** Feature usage analysis, extension ecosystem health

### Remediation Triggers

| Trigger | Action |
|---------|--------|
| WASM FPS drops below 30fps | Investigate WebGPU pipeline, profile GPU workload |
| AI suggestion acceptance <10% | Review context assembly, tune prompts, consider disabling |
| Collaboration input lag >500ms | Profile WebSocket relay, consider input batching |
| Accessibility audit fails | Block release until critical/serious violations resolved |
| Recording corrupts session data | Disable recording, investigate append logic, fix before re-enable |

---

## Appendices

### A. Glossary

| Term | Definition |
|------|-----------|
| **WebGPU** | W3C standard for GPU access in browsers; successor to WebGL |
| **wgpu** | Rust implementation of WebGPU, used by Marauder for rendering |
| **OT** | Operational Transform — algorithm for resolving concurrent edits |
| **CRDT** | Conflict-free Replicated Data Type — alternative to OT |
| **PTY** | Pseudoterminal — kernel interface for terminal I/O |
| **OSC 133** | Terminal escape sequence for semantic zones (prompt/input/output) |
| **Ghost text** | Semi-transparent suggestion text shown ahead of cursor |
| **TTFT** | Time To First Token — latency before LLM starts streaming response |

### B. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| WebGPU not available in target browser | Low | High | Detect at startup, show "WebGPU required" message with browser list |
| wgpu WASM performance insufficient | Medium | High | Profile early (Phase 12.2); fallback plan: Canvas 2D renderer |
| LLM API costs too high for users | Medium | Medium | Default to local models (Ollama); display cost estimates |
| Collaboration server DDoS | Medium | Medium | Rate limiting, connection limits, invite token required |
| Browser clipboard API denied | Low | Low | Show copy/paste instructions; fall back to Ctrl+C/V detection |

### C. Decision Log

| Date | Decision | Context |
|------|----------|---------|
| 2026-03-07 | AI as extension, not core | Keeps core minimal; opt-in; independent release cycle |
| 2026-03-07 | WebSocket over WebRTC for collab | Simpler; sufficient for sequential byte streams; firewall-friendly |
| 2026-03-07 | Server-ordered FIFO over full OT | Terminal input is sequential; full OT is overkill |
| 2026-03-07 | Custom recording format + asciinema export | Need richer metadata than .cast supports |
| 2026-03-07 | Mobile deferred | Touch UX too different; focus on desktop + browser first |
| 2026-03-07 | Hybrid shell over full replacement | Users keep their shell; smart layer adds value without disruption |
| 2026-03-07 | OpenAI-compatible API as standard | De facto standard; works with OpenAI, Anthropic proxy, Ollama, vLLM |
