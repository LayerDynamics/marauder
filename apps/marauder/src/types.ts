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
