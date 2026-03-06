/**
 * @marauder/config — Default configuration values
 *
 * Single source of truth for TypeScript defaults.
 * Matches `pkg/config-store/src/defaults.rs`.
 */

import type { MarauderConfig, TerminalConfig, FontConfig, CursorConfig, WindowConfig } from "./schema.ts";

/** Default terminal settings. */
export const DEFAULT_TERMINAL: TerminalConfig = {
  shell: Deno.env.get("SHELL") ?? "/bin/sh",
  scrollback: 10000,
  rows: 24,
  cols: 80,
};

/** Default font settings. */
export const DEFAULT_FONT: FontConfig = {
  family: "monospace",
  size: 14,
  line_height: 1.2,
};

/** Default cursor settings. */
export const DEFAULT_CURSOR: CursorConfig = {
  style: "block",
  blink: true,
};

/** Default window settings. */
export const DEFAULT_WINDOW: WindowConfig = {
  opacity: 1.0,
  decorations: true,
};

/** Complete default configuration. */
export const DEFAULT_CONFIG: MarauderConfig = {
  terminal: DEFAULT_TERMINAL,
  font: DEFAULT_FONT,
  cursor: DEFAULT_CURSOR,
  window: DEFAULT_WINDOW,
  keybindings: {},
};

