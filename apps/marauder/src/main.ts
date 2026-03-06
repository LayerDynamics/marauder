/**
 * Marauder frontend bootstrap — wires tab bar, status bar, and IPC.
 */

import { invoke } from "@tauri-apps/api/core";
import { writeText, readText } from "@tauri-apps/plugin-clipboard-manager";
import { EventBusClient, PtyClient, GridClient } from "./ipc";
import { detectUrlsInRow, findUrlAtCell, openUrl, type UrlMatch } from "../../../lib/ui/url-handler";
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
const gridClient = new GridClient();

let tabBar: TabBar;
let statusBar: StatusBar;
let activePaneId: number | null = null;
let tabCounter = 0;

/** Cached cell size from the renderer. Updated on init and font changes. */
let cellWidth = 0;
let cellHeight = 0;

/** Mouse selection tracking state. */
let isSelecting = false;
let selectionAnchorRow = 0;
let selectionAnchorCol = 0;

/** Cached URL matches for the visible grid area. */
let cachedUrlMatches: UrlMatch[] = [];

/** Convert pixel coordinates relative to grid element to cell coordinates. */
function pixelToCell(x: number, y: number): { row: number; col: number } {
  const cw = cellWidth || 8.4;
  const ch = cellHeight || 16.8;
  return {
    col: Math.floor(x / cw),
    row: Math.floor(y / ch),
  };
}

let urlDetectTimer: ReturnType<typeof setTimeout> | null = null;

/** Schedule debounced URL detection (300ms after last grid update). */
function scheduleUrlDetection(): void {
  if (urlDetectTimer !== null) clearTimeout(urlDetectTimer);
  urlDetectTimer = setTimeout(() => {
    urlDetectTimer = null;
    detectVisibleUrls();
  }, 300);
}

/** Detect URLs in visible grid rows. Triggered by GridUpdated events. */
async function detectVisibleUrls(): Promise<void> {
  if (activePaneId === null) return;
  try {
    const snapshot = await gridClient.getScreenSnapshot(activePaneId);
    const matches: UrlMatch[] = [];
    for (let row = 0; row < snapshot.cells.length; row++) {
      const rowText = snapshot.cells[row].map((c: { c: string }) => c.c).join("");
      matches.push(...detectUrlsInRow(row, rowText));
    }
    cachedUrlMatches = matches;
  } catch {
    // Grid snapshot not available
  }
}

/** Handle mousedown on terminal grid — start selection. */
function handleMouseDown(e: MouseEvent): void {
  if (activePaneId === null || e.button !== 0) return;

  // Ctrl+Click (or Cmd+Click on macOS) opens URLs
  if (e.ctrlKey || e.metaKey) {
    const gridEl = e.currentTarget as HTMLElement;
    const rect = gridEl.getBoundingClientRect();
    const { row, col } = pixelToCell(e.clientX - rect.left, e.clientY - rect.top);
    const url = findUrlAtCell(cachedUrlMatches, row, col);
    if (url) {
      e.preventDefault();
      openUrl(url).catch(console.error);
      return;
    }
  }

  const gridEl = e.currentTarget as HTMLElement;
  const rect = gridEl.getBoundingClientRect();
  const { row, col } = pixelToCell(e.clientX - rect.left, e.clientY - rect.top);

  isSelecting = true;
  selectionAnchorRow = row;
  selectionAnchorCol = col;

  // Clear previous selection
  gridClient.clearSelection(activePaneId).catch(console.error);

  e.preventDefault();
}

/** Handle mousemove on terminal grid — update selection while dragging. */
function handleMouseMove(e: MouseEvent): void {
  // When NOT selecting, update cursor for URL hover
  if (!isSelecting) {
    const gridEl = document.getElementById("terminal-grid");
    if (gridEl) {
      const rect = gridEl.getBoundingClientRect();
      const { row, col } = pixelToCell(e.clientX - rect.left, e.clientY - rect.top);
      const url = findUrlAtCell(cachedUrlMatches, row, col);
      gridEl.style.cursor = (url && (e.ctrlKey || e.metaKey)) ? "pointer" : "";
    }
  }

  if (!isSelecting || activePaneId === null) return;

  const gridEl = document.getElementById("terminal-grid");
  if (!gridEl) return;
  const rect = gridEl.getBoundingClientRect();
  const { row, col } = pixelToCell(e.clientX - rect.left, e.clientY - rect.top);

  // Normalize: ensure start <= end
  const startRow = Math.min(selectionAnchorRow, row);
  const startCol = (selectionAnchorRow < row || (selectionAnchorRow === row && selectionAnchorCol <= col))
    ? selectionAnchorCol : col;
  const endRow = Math.max(selectionAnchorRow, row);
  const endCol = (selectionAnchorRow < row || (selectionAnchorRow === row && selectionAnchorCol <= col))
    ? col : selectionAnchorCol;

  gridClient.setSelection(activePaneId, startRow, startCol, endRow, endCol).catch(console.error);
}

/** Handle mouseup — finalize selection. */
function handleMouseUp(_e: MouseEvent): void {
  isSelecting = false;
}

/** Characters considered part of a "word" for double-click selection. */
const WORD_CHAR_RE = /[A-Za-z0-9_\-./~]/;

/** Handle double-click — select word at cursor position. */
async function handleDblClick(e: MouseEvent): Promise<void> {
  if (activePaneId === null) return;

  const gridEl = e.currentTarget as HTMLElement;
  const rect = gridEl.getBoundingClientRect();
  const { row, col } = pixelToCell(e.clientX - rect.left, e.clientY - rect.top);
  const paneId = activePaneId;

  try {
    // Get the full row via screen snapshot to scan word boundaries
    const snapshot = await gridClient.getScreenSnapshot(paneId);
    if (row >= snapshot.cells.length) return;
    const rowCells = snapshot.cells[row];
    const rowLen = rowCells.length;

    // Check if the clicked cell is a word character
    if (col >= rowLen || !WORD_CHAR_RE.test(rowCells[col].c)) {
      // Not a word char — select the single cell
      gridClient.setSelection(paneId, row, col, row, col).catch(console.error);
      return;
    }

    // Scan backward for word start
    let startCol = col;
    while (startCol > 0 && WORD_CHAR_RE.test(rowCells[startCol - 1].c)) {
      startCol--;
    }

    // Scan forward for word end
    let endCol = col;
    while (endCol < rowLen - 1 && WORD_CHAR_RE.test(rowCells[endCol + 1].c)) {
      endCol++;
    }

    gridClient.setSelection(paneId, row, startCol, row, endCol).catch(console.error);
  } catch {
    // Snapshot unavailable — fall back to single-cell selection
    gridClient.setSelection(paneId, row, col, row, col).catch(console.error);
  }
}

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
    case EventType.GridUpdated: {
      // Debounce URL detection — grid updates can be very frequent
      scheduleUrlDetection();
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
 * Key mappings are sourced from lib/io/vt-keymap.ts (single source of truth
 * shared with the Deno-side encoder lib/io/vt.ts). Vite resolves the import
 * at build time — no Deno-specific APIs are used in the keymap module.
 */
import {
  CSI_TILDE_KEYS,
  CSI_LETTER_KEYS,
  SS3_FUNCTION_KEYS,
  CTRL_SPECIAL,
  computeXtermModifier,
} from "../../../lib/io/vt-keymap";

const enc = new TextEncoder();

function encodeKeyForPty(e: KeyboardEvent): Uint8Array | null {
  // Ctrl+letter → control character (0x01-0x1A)
  if (e.ctrlKey && e.key.length === 1) {
    const code = e.key.toLowerCase().charCodeAt(0);
    if (code >= 0x61 && code <= 0x7a) return new Uint8Array([code - 0x60]);
    // Ctrl+special characters (from shared keymap)
    const ctrlByte = CTRL_SPECIAL[e.key];
    if (ctrlByte !== undefined) return new Uint8Array([ctrlByte]);
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

  // xterm-style modifier parameter (from shared keymap)
  const mod = computeXtermModifier(e.shiftKey, e.altKey, e.ctrlKey);

  // Simple special keys
  switch (e.key) {
    case "Enter": return enc.encode("\r");
    case "Backspace": return e.altKey ? new Uint8Array([0x1b, 0x7f]) : new Uint8Array([0x7f]);
    case "Tab": return e.shiftKey ? enc.encode("\x1b[Z") : enc.encode("\t");
    case "Escape": return new Uint8Array([0x1b]);
    default: break;
  }

  // CSI tilde-sequences (Delete, Insert, PageUp/Down, F5-F24)
  const tildeCode = CSI_TILDE_KEYS[e.key];
  if (tildeCode !== undefined) {
    if (mod > 1) return enc.encode(`\x1b[${tildeCode};${mod}~`);
    return enc.encode(`\x1b[${tildeCode}~`);
  }

  // Arrow/Home/End — CSI letter encoding
  const letter = CSI_LETTER_KEYS[e.key];
  if (letter !== undefined) {
    if (mod > 1) return enc.encode(`\x1b[1;${mod}${letter}`);
    return enc.encode(`\x1b[${letter}`);
  }

  // F1-F4 — SS3 unmodified, CSI modified
  const ss3Letter = SS3_FUNCTION_KEYS[e.key];
  if (ss3Letter !== undefined) {
    if (mod > 1) return enc.encode(`\x1b[1;${mod}${ss3Letter}`);
    return enc.encode(`\x1bO${ss3Letter}`);
  }

  return null;
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

  // Prevent default early so the browser doesn't act on the key while we await
  e.preventDefault();

  // Capture paneId synchronously before any async calls to avoid races
  const paneId = activePaneId;

  // Built-in keybindings checked synchronously before async backend call
  // Clipboard: Ctrl+Shift+C (copy), Ctrl+Shift+V (paste)
  if (e.ctrlKey && e.shiftKey && e.key === "C") {
    gridClient.getSelectionText(paneId).then((text) => {
      if (text) writeText(text).catch(console.error);
    }).catch(console.error);
    return;
  }
  if (e.ctrlKey && e.shiftKey && e.key === "V") {
    readText().then((text) => {
      if (text && paneId !== null) {
        const bytes = Array.from(new TextEncoder().encode(text));
        ptyClient.write(paneId, bytes).catch(console.error);
      }
    }).catch(console.error);
    return;
  }

  // Scrollback navigation: Shift+PageUp/PageDown
  if (e.shiftKey && e.key === "PageUp") {
    gridClient.scrollViewportBy(paneId, 24).catch(console.error);
    return;
  }
  if (e.shiftKey && e.key === "PageDown") {
    gridClient.scrollViewportBy(paneId, -24).catch(console.error);
    return;
  }

  // Phase 1: Check for keybinding action via backend
  const keySeq = buildKeySequence(e);

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
  ptyClient.write(paneId, bytes).catch((err) => {
    console.error("Failed to write to PTY:", err);
  });

  // Publish KeyInput event for extensions via event bus bridge
  invoke("event_bus_emit", {
    event_type: EventType.KeyInput,
    payload: JSON.stringify({ paneId: paneId, keySeq }),
  }).catch((err) => {
    console.warn("Failed to emit KeyInput event:", err);
  });
}

let resizeTimer: ReturnType<typeof setTimeout> | null = null;

/** Handle window resize — debounced to avoid excessive IPC during window drag. */
function handleResize(): void {
  if (resizeTimer !== null) clearTimeout(resizeTimer);
  resizeTimer = setTimeout(handleResizeImpl, 50);
}

function handleResizeImpl(): void {
  resizeTimer = null;
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
  document.removeEventListener("mouseup", handleMouseUp);
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
      EventType.GridUpdated,
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

  // Wire mouse wheel for scrollback navigation
  const gridEl = document.getElementById("terminal-grid");
  if (gridEl) {
    gridEl.addEventListener("wheel", (e: WheelEvent) => {
      if (activePaneId === null) return;
      e.preventDefault();
      const lines = Math.round(e.deltaY / (cellHeight || 16.8));
      if (lines !== 0) {
        // Positive deltaY = scroll down in browser = scroll up into history (positive offset)
        gridClient.scrollViewportBy(activePaneId, lines).catch((err) => {
          console.error("Scroll failed:", err);
        });
      }
    }, { passive: false });
  }

  // Wire mouse selection handlers
  if (gridEl) {
    gridEl.addEventListener("mousedown", handleMouseDown);
    gridEl.addEventListener("mousemove", handleMouseMove);
    gridEl.addEventListener("dblclick", handleDblClick);
  }
  document.addEventListener("mouseup", handleMouseUp);

  // Wire window resize
  window.addEventListener("resize", handleResize);

  // Clean up on unload
  window.addEventListener("beforeunload", teardown);

  // Create initial tab
  await createTab();

  // Initial URL detection (subsequent runs triggered by GridUpdated events)
  detectVisibleUrls();

  // Initial resize
  handleResize();
});
