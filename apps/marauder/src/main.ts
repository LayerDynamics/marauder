/**
 * Marauder frontend bootstrap — wires tab bar, status bar, and IPC.
 */

import { invoke } from "@tauri-apps/api/core";
import { EventBusClient, PtyClient } from "./ipc";
import { TabBar } from "./components/tab-bar";
import { StatusBar } from "./components/status-bar";
import {
  EventType,
  type BusEvent,
  type PtyInfo,
  type ShellCwdPayload,
  type ShellCommandStartedPayload,
  type GridResizedPayload,
  type PanePayload,
} from "./types";

const eventBus = new EventBusClient();
const ptyClient = new PtyClient();

let tabBar: TabBar;
let statusBar: StatusBar;
let activePaneId: number | null = null;
let tabCounter = 0;

/** Cached cell size from the renderer. Updated on init and font changes. */
let cellWidth = 0;
let cellHeight = 0;

/** Decode a BusEvent payload (byte array) to a parsed object. */
function decodePayload<T>(event: BusEvent): T | null {
  try {
    if (event.payload.length === 0) return null;
    const text = new TextDecoder().decode(new Uint8Array(event.payload));
    return JSON.parse(text) as T;
  } catch {
    return null;
  }
}

/** Fetch cell size from the renderer via Tauri command. */
async function fetchCellSize(): Promise<void> {
  try {
    const size: [number, number] = await invoke("renderer_get_cell_size");
    cellWidth = size[0];
    cellHeight = size[1];

    // Update data attributes on grid element for external consumers
    const grid = document.getElementById("terminal-grid");
    if (grid) {
      grid.setAttribute("data-cell-width", cellWidth.toString());
      grid.setAttribute("data-cell-height", cellHeight.toString());
    }
  } catch {
    // Renderer may not be ready yet — use config-derived defaults
    // These match RendererConfig::default() font_size=14, line_height=1.2
    if (cellWidth === 0) {
      cellWidth = 8.4;  // 14 * 0.6
      cellHeight = 16.8; // 14 * 1.2
    }
  }
}

/** Create a new terminal tab with its own PTY session. */
async function createTab(): Promise<void> {
  try {
    const info: PtyInfo = await ptyClient.create({ rows: 24, cols: 80 });
    tabCounter++;
    tabBar.addTab(info.pane_id, `shell ${tabCounter}`);
    activePaneId = info.pane_id;
    statusBar.setDimensions(info.rows, info.cols);
    statusBar.setCwd("~");
  } catch (e) {
    console.error("Failed to create PTY session:", e);
  }
}

/** Close a terminal tab and its PTY session. */
async function closeTab(paneId: number): Promise<void> {
  try {
    await ptyClient.close(paneId);
  } catch {
    // PTY may already be closed
  }
  tabBar.removeTab(paneId);
  if (activePaneId === paneId) {
    // Fall back to the tab that TabBar selected, or null if none remain
    activePaneId = tabBar.getActiveTabId();
  }
}

/** Handle incoming bus events and update UI. */
function handleEvent(event: BusEvent): void {
  switch (event.event_type) {
    case EventType.PaneCreated: {
      const p = decodePayload<PanePayload>(event);
      if (p) {
        tabBar.addTab(p.pane_id, `shell ${++tabCounter}`);
      }
      break;
    }
    case EventType.PaneClosed: {
      const p = decodePayload<PanePayload>(event);
      if (p) {
        tabBar.removeTab(p.pane_id);
      }
      break;
    }
    case EventType.ShellCwdChanged: {
      const p = decodePayload<ShellCwdPayload>(event);
      if (p) {
        statusBar.setCwd(p.cwd);
      }
      break;
    }
    case EventType.ShellCommandStarted: {
      const p = decodePayload<ShellCommandStartedPayload>(event);
      if (p) {
        statusBar.setCommand(p.command);
      }
      break;
    }
    case EventType.ShellCommandFinished: {
      statusBar.clearCommand();
      break;
    }
    case EventType.GridResized: {
      const p = decodePayload<GridResizedPayload>(event);
      if (p) {
        statusBar.setDimensions(p.rows, p.cols);
      }
      break;
    }
  }
}

/**
 * Build a canonical key sequence string from a KeyboardEvent.
 * Mirrors the logic in lib/ui/user/parser.ts for the webview context.
 */
function buildKeySequence(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Meta");

  let key = e.key;
  if (key.length === 1 && key >= "a" && key <= "z") key = key.toUpperCase();
  else if (key === "ArrowUp") key = "Up";
  else if (key === "ArrowDown") key = "Down";
  else if (key === "ArrowLeft") key = "Left";
  else if (key === "ArrowRight") key = "Right";
  else if (key === " ") key = "Space";

  // Skip modifier-only keys
  if (key === "Control" || key === "Shift" || key === "Alt" || key === "Meta") {
    return parts.join("+");
  }

  parts.push(key);
  return parts.join("+");
}

/**
 * Encode a key press into VT/ANSI bytes for PTY consumption.
 * Inline version of lib/io/vt.ts for the webview context.
 */
/**
 * Encode a KeyboardEvent into VT/ANSI bytes for PTY consumption.
 * Returns null for unrecognized keys.
 *
 * NOTE: This must stay in sync with lib/io/vt.ts (the Deno-side encoder).
 * The webview cannot import Deno modules directly, so this is a mirror copy.
 * If you add key encodings here, add them in vt.ts too (and vice versa).
 */
function encodeKeyForPty(e: KeyboardEvent): Uint8Array | null {
  const enc = new TextEncoder();

  // Ctrl+letter → control character (0x01-0x1A)
  if (e.ctrlKey && e.key.length === 1) {
    const code = e.key.toLowerCase().charCodeAt(0);
    if (code >= 0x61 && code <= 0x7a) return new Uint8Array([code - 0x60]);
    // Ctrl+special
    if (e.key === "[") return new Uint8Array([0x1b]); // Escape
    if (e.key === "\\") return new Uint8Array([0x1c]); // SIGQUIT
    if (e.key === "]") return new Uint8Array([0x1d]);
    if (e.key === "^") return new Uint8Array([0x1e]);
    if (e.key === "_") return new Uint8Array([0x1f]);
    if (e.key === " ") return new Uint8Array([0x00]); // Ctrl+Space = NUL
    return null;
  }

  // Alt+char → ESC + char
  if (e.altKey && e.key.length === 1) {
    const charBytes = enc.encode(e.key);
    const result = new Uint8Array(1 + charBytes.length);
    result[0] = 0x1b;
    result.set(charBytes, 1);
    return result;
  }

  // Printable character
  if (e.key.length === 1 && !e.ctrlKey && !e.metaKey) {
    return enc.encode(e.key);
  }

  // xterm-style modifier parameter:
  // 1=none, 2=Shift, 3=Alt, 4=Shift+Alt, 5=Ctrl, 6=Shift+Ctrl, 7=Alt+Ctrl, 8=Shift+Alt+Ctrl
  let mod = 1;
  if (e.shiftKey) mod += 1;
  if (e.altKey) mod += 2;
  if (e.ctrlKey) mod += 4;

  /** Encode CSI tilde-sequence with optional modifier. */
  const csi = (seq: string): Uint8Array => {
    if (mod > 1 && seq.endsWith("~")) {
      const base = seq.slice(0, -1);
      return enc.encode(`\x1b[${base};${mod}~`);
    }
    return enc.encode(`\x1b[${seq}`);
  };

  /** Encode arrow/Home/End (SS3 unmodified, CSI modified). */
  const ss3 = (letter: string): Uint8Array => {
    if (mod > 1) return enc.encode(`\x1b[1;${mod}${letter}`);
    return enc.encode(`\x1b[${letter}`);
  };

  switch (e.key) {
    case "Enter": return enc.encode("\r");
    case "Backspace": return e.altKey ? new Uint8Array([0x1b, 0x7f]) : new Uint8Array([0x7f]);
    case "Tab": return e.shiftKey ? enc.encode("\x1b[Z") : enc.encode("\t");
    case "Escape": return new Uint8Array([0x1b]);
    case "Delete": return csi("3~");
    case "Insert": return csi("2~");
    case "PageUp": return csi("5~");
    case "PageDown": return csi("6~");

    // Arrow keys
    case "ArrowUp": return ss3("A");
    case "ArrowDown": return ss3("B");
    case "ArrowRight": return ss3("C");
    case "ArrowLeft": return ss3("D");

    // Home/End
    case "Home": return ss3("H");
    case "End": return ss3("F");

    // Function keys F1-F4 (SS3 prefix unmodified)
    case "F1": return enc.encode(mod > 1 ? `\x1b[1;${mod}P` : "\x1bOP");
    case "F2": return enc.encode(mod > 1 ? `\x1b[1;${mod}Q` : "\x1bOQ");
    case "F3": return enc.encode(mod > 1 ? `\x1b[1;${mod}R` : "\x1bOR");
    case "F4": return enc.encode(mod > 1 ? `\x1b[1;${mod}S` : "\x1bOS");

    // Function keys F5-F12
    case "F5": return csi("15~");
    case "F6": return csi("17~");
    case "F7": return csi("18~");
    case "F8": return csi("19~");
    case "F9": return csi("20~");
    case "F10": return csi("21~");
    case "F11": return csi("23~");
    case "F12": return csi("24~");

    // Function keys F13-F24
    case "F13": return csi("25~");
    case "F14": return csi("26~");
    case "F15": return csi("28~");
    case "F16": return csi("29~");
    case "F17": return csi("31~");
    case "F18": return csi("32~");
    case "F19": return csi("33~");
    case "F20": return csi("34~");
    case "F21": return csi("42~");
    case "F22": return csi("43~");
    case "F23": return csi("44~");
    case "F24": return csi("45~");

    default: return null;
  }
}

/**
 * Forward keyboard input to the active PTY with keybinding resolution.
 *
 * Two-phase approach:
 * 1. Build canonical key sequence and check for UI action bindings
 * 2. If no binding matches, encode as VT/ANSI and write to PTY
 */
async function handleKeyInput(e: KeyboardEvent): Promise<void> {
  if (activePaneId === null) return;

  // Skip modifier-only key presses
  if (e.key === "Control" || e.key === "Shift" || e.key === "Alt" || e.key === "Meta") return;

  // Let Meta (Cmd on macOS) shortcuts pass through to the system
  if (e.metaKey) return;

  // Phase 1: Check for keybinding action
  const keySeq = buildKeySequence(e);

  // Prevent default early so the browser doesn't act on the key while we await
  // (we'll let it through if the key isn't bound and can't be encoded)
  e.preventDefault();

  // Try to resolve via backend keybinding handler
  try {
    const result = await invoke("resolve_keybinding", { keySeq });
    const res = result as { action: string | null } | null;
    if (res?.action) {
      // UI action handled by backend — don't write to PTY
      return;
    }
  } catch {
    // resolve_keybinding command not available yet — fall through to direct PTY write
  }

  // Phase 2: Encode and write to PTY (only reached when no keybinding matched)
  const encoded = encodeKeyForPty(e);
  if (encoded === null) return;

  const bytes = Array.from(encoded);
  ptyClient.write(activePaneId, bytes).catch((err) => {
    console.error("Failed to write to PTY:", err);
  });

  // Publish KeyInput event for extensions via event bus bridge
  invoke("event_bus_emit", {
    event_type: EventType.KeyInput,
    payload: JSON.stringify({ paneId: activePaneId, keySeq }),
  }).catch((err) => {
    console.warn("Failed to emit KeyInput event:", err);
  });
}

/** Handle window resize — resize the active PTY using renderer cell metrics. */
function handleResize(): void {
  if (activePaneId === null) return;

  const grid = document.getElementById("terminal-grid");
  if (!grid) return;

  if (cellWidth <= 0 || cellHeight <= 0) return;

  const cols = Math.floor(grid.clientWidth / cellWidth);
  const rows = Math.floor(grid.clientHeight / cellHeight);

  if (cols > 0 && rows > 0) {
    ptyClient.resize(activePaneId, rows, cols).catch((e) => {
      console.error("Failed to resize PTY:", e);
    });
    statusBar.setDimensions(rows, cols);

    // Notify renderer of new surface size
    invoke("renderer_resize", {
      width: Math.round(grid.clientWidth * window.devicePixelRatio),
      height: Math.round(grid.clientHeight * window.devicePixelRatio),
      scaleFactor: window.devicePixelRatio,
    }).catch(() => {
      // Renderer may not be initialized yet
    });
  }
}

/** Clean up subscriptions and listeners on window unload. */
function teardown(): void {
  document.removeEventListener("keydown", handleKeyInput);
  window.removeEventListener("resize", handleResize);
  eventBus.destroy().catch(() => {});
}

/** Bootstrap the application. */
window.addEventListener("DOMContentLoaded", async () => {
  const tabBarEl = document.getElementById("tab-bar")!;
  const statusBarEl = document.getElementById("status-bar")!;

  tabBar = new TabBar(tabBarEl);
  statusBar = new StatusBar(statusBarEl);

  // Wire tab bar custom events
  tabBarEl.addEventListener("tab-new", () => createTab());
  tabBarEl.addEventListener("tab-select", ((e: CustomEvent) => {
    const id = e.detail.id as number;
    tabBar.setActiveTab(id);
    activePaneId = id;
  }) as EventListener);
  tabBarEl.addEventListener("tab-close", ((e: CustomEvent) => {
    closeTab(e.detail.id as number);
  }) as EventListener);

  // Subscribe to bus events for UI updates
  await eventBus.subscribe(
    [
      EventType.PaneCreated,
      EventType.PaneClosed,
      EventType.ShellCwdChanged,
      EventType.ShellCommandStarted,
      EventType.ShellCommandFinished,
      EventType.GridResized,
    ],
    handleEvent
  );

  // Fetch cell size from renderer (retry briefly if renderer isn't ready)
  await fetchCellSize();
  if (cellWidth === 0) {
    setTimeout(fetchCellSize, 500);
  }

  // Wire keyboard input
  document.addEventListener("keydown", handleKeyInput);

  // Wire window resize
  window.addEventListener("resize", handleResize);

  // Clean up on unload
  window.addEventListener("beforeunload", teardown);

  // Create initial tab
  await createTab();

  // Initial resize
  handleResize();
});
