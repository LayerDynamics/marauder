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
    activePaneId = null;
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
    case EventType.RendererReady: {
      document.body.classList.add("wgpu-ready");
      // Re-fetch cell size now that renderer is fully initialized
      fetchCellSize().catch((e) => console.error("Failed to fetch cell size on RendererReady:", e));
      break;
    }
  }
}

/** Forward keyboard input to the active PTY, including Ctrl sequences. */
function handleKeyInput(e: KeyboardEvent): void {
  if (activePaneId === null) return;

  // Skip modifier-only key presses
  if (e.key === "Control" || e.key === "Shift" || e.key === "Alt" || e.key === "Meta") return;

  // Let Meta (Cmd on macOS) shortcuts pass through to the system
  if (e.metaKey) return;

  let data: string;

  if (e.ctrlKey && e.key.length === 1) {
    // Map Ctrl+letter to control characters (Ctrl+A=0x01, Ctrl+C=0x03, etc.)
    const code = e.key.toLowerCase().charCodeAt(0);
    if (code >= 0x61 && code <= 0x7a) {
      // a-z → 0x01-0x1a
      data = String.fromCharCode(code - 0x60);
    } else if (e.key === "[") {
      data = "\x1b"; // Ctrl+[ = Escape
    } else if (e.key === "\\") {
      data = "\x1c"; // Ctrl+\ = SIGQUIT
    } else if (e.key === "]") {
      data = "\x1d"; // Ctrl+]
    } else {
      return;
    }
  } else if (e.ctrlKey) {
    // Ctrl+non-letter (arrows, etc.) — let browser handle or ignore
    return;
  } else if (e.key.length === 1) {
    data = e.key;
  } else {
    // Map special keys to ANSI sequences
    switch (e.key) {
      case "Enter": data = "\r"; break;
      case "Backspace": data = "\x7f"; break;
      case "Tab": data = "\t"; break;
      case "Escape": data = "\x1b"; break;
      case "ArrowUp": data = "\x1b[A"; break;
      case "ArrowDown": data = "\x1b[B"; break;
      case "ArrowRight": data = "\x1b[C"; break;
      case "ArrowLeft": data = "\x1b[D"; break;
      case "Home": data = "\x1b[H"; break;
      case "End": data = "\x1b[F"; break;
      case "Delete": data = "\x1b[3~"; break;
      case "PageUp": data = "\x1b[5~"; break;
      case "PageDown": data = "\x1b[6~"; break;
      default: return;
    }
  }

  e.preventDefault();

  const bytes = Array.from(new TextEncoder().encode(data));
  ptyClient.write(activePaneId, bytes).catch((err) => {
    console.error("Failed to write to PTY:", err);
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
      EventType.RendererReady,
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
