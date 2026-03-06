/**
 * Shared TypeScript types mirroring Rust event bus and PTY types.
 */

/** Event type discriminants matching `EventType` in `pkg/event-bus/src/events.rs`. */
export const EventType = {
  // Input layer
  KeyInput: 0,
  MouseInput: 1,
  PasteInput: 2,

  // PTY layer
  PtyOutput: 3,
  PtyExit: 4,
  PtyError: 5,

  // Parser layer
  ParserAction: 6,

  // Grid layer
  GridUpdated: 7,
  GridResized: 8,
  GridScrolled: 9,
  SelectionChanged: 10,

  // Shell layer
  ShellPromptDetected: 11,
  ShellCommandStarted: 12,
  ShellCommandFinished: 13,
  ShellCwdChanged: 14,

  // Render layer
  RenderFrameRequested: 15,
  RenderFrameCompleted: 16,
  OverlayChanged: 17,
  RendererReady: 31,

  // Config layer
  ConfigChanged: 18,
  ConfigError: 19,

  // Lifecycle
  SessionCreated: 20,
  SessionClosed: 21,
  PaneCreated: 22,
  PaneClosed: 23,
  PaneFocused: 24,
  TabCreated: 25,
  TabClosed: 26,
  TabFocused: 27,

  // Extension layer
  ExtensionLoaded: 28,
  ExtensionUnloaded: 29,
  ExtensionMessage: 30,
} as const;

export type EventTypeValue = (typeof EventType)[keyof typeof EventType];

/** Mirrors Rust `Event` struct serialized via serde_json. */
export interface BusEvent {
  event_type: EventTypeValue;
  payload: number[];
  timestamp_us: number;
  source?: string;
}

/** Request to create a PTY session. Mirrors `CreatePtyRequest`. */
export interface CreatePtyRequest {
  shell?: string;
  cwd?: string;
  env?: Record<string, string>;
  rows: number;
  cols: number;
}

/** PTY session info returned from creation. Mirrors `PtyInfo`. */
export interface PtyInfo {
  pane_id: number;
  pid: number | null;
  shell: string;
  rows: number;
  cols: number;
}

/** Payload for PaneCreated / PaneClosed events. */
export interface PanePayload {
  pane_id: number;
}

/** Payload for ShellCwdChanged event. */
export interface ShellCwdPayload {
  cwd: string;
}

/** Payload for ShellCommandStarted event. */
export interface ShellCommandStartedPayload {
  command: string;
}

/** Payload for GridResized event. */
export interface GridResizedPayload {
  rows: number;
  cols: number;
}

/** Terminal cell color. Mirrors Rust `Color` enum. */
export type CellColor =
  | "Default"
  | { Named: number }
  | { Indexed: number }
  | { Rgb: { r: number; g: number; b: number } }
  | { Rgba: { r: number; g: number; b: number; a: number } };

/** Cell attribute bitflags. Mirrors `CellAttributes` in `pkg/grid/src/cell.rs`. */
export const CellAttr = {
  BOLD:          0b0000_0001,
  ITALIC:        0b0000_0010,
  UNDERLINE:     0b0000_0100,
  STRIKETHROUGH: 0b0000_1000,
  BLINK:         0b0001_0000,
  DIM:           0b0010_0000,
  INVERSE:       0b0100_0000,
  HIDDEN:        0b1000_0000,
} as const;

/** Test whether a cell attribute flag is set. */
export function hasAttr(attrs: number, flag: number): boolean {
  return (attrs & flag) !== 0;
}

/** Terminal cell info. Mirrors Rust `Cell` struct. */
export interface CellInfo {
  c: string;
  fg: CellColor;
  bg: CellColor;
  attrs: number;
  hyperlink_id: number | null;
  width: number;
}

/** Cursor position returned from grid commands. */
export interface CursorPosition {
  row: number;
  col: number;
}

/** Grid dimensions returned from grid commands. */
export interface GridDimensions {
  rows: number;
  cols: number;
}

/** Full screen snapshot returned in a single IPC round-trip. */
export interface ScreenSnapshot {
  rows: number;
  cols: number;
  cursor: CursorPosition;
  cells: CellInfo[][];
}
