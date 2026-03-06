/**
 * @marauder/io — VT/ANSI key encoding library
 *
 * Encodes keyboard input into VT/ANSI escape sequences for PTY write.
 * Covers arrows, function keys F1-F24, Home/End/Insert/Delete/PageUp/PageDown,
 * modifier-aware encoding (xterm-style).
 *
 * Key mappings are sourced from ./vt-keymap.ts (the single source of truth
 * shared with the webview encoder in apps/marauder/src/main.ts).
 */

import {
  CSI_TILDE_KEYS,
  CSI_LETTER_KEYS,
  SS3_FUNCTION_KEYS,
  CTRL_SPECIAL,
  computeXtermModifier,
} from "./vt-keymap.ts";

const encoder = new TextEncoder();

/** Modifier flags for encoding. */
export interface KeyModifiers {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

/**
 * Encode a key press into VT/ANSI bytes for PTY consumption.
 * Returns null for unrecognized keys (caller decides what to do).
 */
export function encodeKey(key: string, modifiers: KeyModifiers): Uint8Array | null {
  // Ctrl+letter → control character (0x01-0x1A)
  if (modifiers.ctrl && key.length === 1) {
    const code = key.toLowerCase().charCodeAt(0);
    if (code >= 0x61 && code <= 0x7a) {
      return new Uint8Array([code - 0x60]);
    }
    // Ctrl+special characters
    const ctrlByte = CTRL_SPECIAL[key];
    if (ctrlByte !== undefined) {
      return new Uint8Array([ctrlByte]);
    }
  }

  // Simple printable character
  if (key.length === 1 && !modifiers.ctrl && !modifiers.meta) {
    if (modifiers.alt) {
      // Alt+char → ESC + char
      const charBytes = encoder.encode(key);
      const result = new Uint8Array(1 + charBytes.length);
      result[0] = 0x1b;
      result.set(charBytes, 1);
      return result;
    }
    return encoder.encode(key);
  }

  // Special keys
  const mod = computeXtermModifier(modifiers.shift, modifiers.alt, modifiers.ctrl);

  // Simple special keys
  switch (key) {
    case "Enter": return encoder.encode("\r");
    case "Backspace": return modifiers.alt ? new Uint8Array([0x1b, 0x7f]) : new Uint8Array([0x7f]);
    case "Tab": return modifiers.shift ? encoder.encode("\x1b[Z") : encoder.encode("\t");
    case "Escape": return new Uint8Array([0x1b]);
    default: break;
  }

  // CSI tilde-sequences (Delete, Insert, PageUp/Down, F5-F24)
  const tildeCode = CSI_TILDE_KEYS[key];
  if (tildeCode !== undefined) {
    if (mod > 1) {
      return encoder.encode(`\x1b[${tildeCode};${mod}~`);
    }
    return encoder.encode(`\x1b[${tildeCode}~`);
  }

  // Arrow/Home/End — CSI letter encoding
  const letter = CSI_LETTER_KEYS[key];
  if (letter !== undefined) {
    if (mod > 1) {
      return encoder.encode(`\x1b[1;${mod}${letter}`);
    }
    return encoder.encode(`\x1b[${letter}`);
  }

  // F1-F4 — SS3 unmodified, CSI modified
  const ss3Letter = SS3_FUNCTION_KEYS[key];
  if (ss3Letter !== undefined) {
    if (mod > 1) {
      return encoder.encode(`\x1b[1;${mod}${ss3Letter}`);
    }
    return encoder.encode(`\x1bO${ss3Letter}`);
  }

  return null;
}
