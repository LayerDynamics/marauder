/**
 * @marauder/io — Shared VT key mapping data
 *
 * SINGLE SOURCE OF TRUTH for special key → ANSI/VT sequence mappings.
 * Used by both:
 *   - lib/io/vt.ts (Deno-side encoder)
 *   - apps/marauder/src/main.ts (webview-side encoder)
 *
 * If you add or change key encodings, update this file and both consumers
 * will stay in sync.
 *
 * This module is intentionally free of Deno-specific or browser-specific APIs
 * so it can be consumed from either environment.
 */

/**
 * CSI tilde-sequence keys: the numeric code before the ~.
 * e.g. Delete → "3" means \x1b[3~ unmodified, \x1b[3;{mod}~ with modifiers.
 */
export const CSI_TILDE_KEYS: Record<string, string> = {
  Delete: "3",
  Insert: "2",
  PageUp: "5",
  PageDown: "6",
  F5: "15",
  F6: "17",
  F7: "18",
  F8: "19",
  F9: "20",
  F10: "21",
  F11: "23",
  F12: "24",
  F13: "25",
  F14: "26",
  F15: "28",
  F16: "29",
  F17: "31",
  F18: "32",
  F19: "33",
  F20: "34",
  F21: "42",
  F22: "43",
  F23: "44",
  F24: "45",
};

/**
 * Arrow/navigation keys using CSI letter encoding.
 * Unmodified: \x1b[{letter}. Modified: \x1b[1;{mod}{letter}.
 */
export const CSI_LETTER_KEYS: Record<string, string> = {
  ArrowUp: "A",
  ArrowDown: "B",
  ArrowRight: "C",
  ArrowLeft: "D",
  Home: "H",
  End: "F",
};

/**
 * Function keys F1-F4 use SS3 encoding unmodified (\x1bO{letter})
 * and CSI encoding modified (\x1b[1;{mod}{letter}).
 */
export const SS3_FUNCTION_KEYS: Record<string, string> = {
  F1: "P",
  F2: "Q",
  F3: "R",
  F4: "S",
};

/**
 * Ctrl+special character mappings → byte value.
 */
export const CTRL_SPECIAL: Record<string, number> = {
  "[": 0x1b,  // Escape
  "\\": 0x1c, // SIGQUIT
  "]": 0x1d,
  "^": 0x1e,
  "_": 0x1f,
  " ": 0x00,  // NUL
};

/**
 * Compute the xterm modifier parameter from modifier flags.
 * 1=none, 2=Shift, 3=Alt, 4=Shift+Alt, 5=Ctrl,
 * 6=Shift+Ctrl, 7=Alt+Ctrl, 8=Shift+Alt+Ctrl
 */
export function computeXtermModifier(
  shift: boolean,
  alt: boolean,
  ctrl: boolean,
): number {
  let val = 1;
  if (shift) val += 1;
  if (alt) val += 2;
  if (ctrl) val += 4;
  return val;
}
