/**
 * @marauder/io — VT/ANSI key encoding library
 *
 * Encodes keyboard input into VT/ANSI escape sequences for PTY write.
 * Covers arrows, function keys F1-F24, Home/End/Insert/Delete/PageUp/PageDown,
 * modifier-aware encoding (xterm-style).
 */

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
    // Ctrl+special
    if (key === "[") return new Uint8Array([0x1b]); // Escape
    if (key === "\\") return new Uint8Array([0x1c]); // SIGQUIT
    if (key === "]") return new Uint8Array([0x1d]);
    if (key === "^") return new Uint8Array([0x1e]);
    if (key === "_") return new Uint8Array([0x1f]);
    if (key === " ") return new Uint8Array([0x00]); // Ctrl+Space = NUL
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
  const mod = computeModifier(modifiers);

  switch (key) {
    case "Enter": return encoder.encode("\r");
    case "Backspace": return modifiers.alt ? new Uint8Array([0x1b, 0x7f]) : new Uint8Array([0x7f]);
    case "Tab": return modifiers.shift ? encoder.encode("\x1b[Z") : encoder.encode("\t");
    case "Escape": return new Uint8Array([0x1b]);
    case "Delete": return encodeCSI("3~", mod);
    case "Insert": return encodeCSI("2~", mod);
    case "PageUp": return encodeCSI("5~", mod);
    case "PageDown": return encodeCSI("6~", mod);

    // Arrow keys
    case "ArrowUp": return encodeSS3OrCSI("A", mod);
    case "ArrowDown": return encodeSS3OrCSI("B", mod);
    case "ArrowRight": return encodeSS3OrCSI("C", mod);
    case "ArrowLeft": return encodeSS3OrCSI("D", mod);

    // Home/End
    case "Home": return encodeSS3OrCSI("H", mod);
    case "End": return encodeSS3OrCSI("F", mod);

    // Function keys F1-F4 (SS3 prefix in unmodified mode)
    case "F1": return mod > 1 ? encodeCSI("1;{mod}P".replace("{mod}", String(mod)), 0) : encoder.encode("\x1bOP");
    case "F2": return mod > 1 ? encodeCSI("1;{mod}Q".replace("{mod}", String(mod)), 0) : encoder.encode("\x1bOQ");
    case "F3": return mod > 1 ? encodeCSI("1;{mod}R".replace("{mod}", String(mod)), 0) : encoder.encode("\x1bOR");
    case "F4": return mod > 1 ? encodeCSI("1;{mod}S".replace("{mod}", String(mod)), 0) : encoder.encode("\x1bOS");

    // Function keys F5-F12
    case "F5": return encodeCSI("15~", mod);
    case "F6": return encodeCSI("17~", mod);
    case "F7": return encodeCSI("18~", mod);
    case "F8": return encodeCSI("19~", mod);
    case "F9": return encodeCSI("20~", mod);
    case "F10": return encodeCSI("21~", mod);
    case "F11": return encodeCSI("23~", mod);
    case "F12": return encodeCSI("24~", mod);

    // Function keys F13-F24
    case "F13": return encodeCSI("25~", mod);
    case "F14": return encodeCSI("26~", mod);
    case "F15": return encodeCSI("28~", mod);
    case "F16": return encodeCSI("29~", mod);
    case "F17": return encodeCSI("31~", mod);
    case "F18": return encodeCSI("32~", mod);
    case "F19": return encodeCSI("33~", mod);
    case "F20": return encodeCSI("34~", mod);
    case "F21": return encodeCSI("42~", mod);
    case "F22": return encodeCSI("43~", mod);
    case "F23": return encodeCSI("44~", mod);
    case "F24": return encodeCSI("45~", mod);

    default: return null;
  }
}

/**
 * Compute the xterm modifier parameter.
 * 1 = none, 2 = Shift, 3 = Alt, 4 = Shift+Alt, 5 = Ctrl,
 * 6 = Shift+Ctrl, 7 = Alt+Ctrl, 8 = Shift+Alt+Ctrl
 */
function computeModifier(m: KeyModifiers): number {
  let val = 1;
  if (m.shift) val += 1;
  if (m.alt) val += 2;
  if (m.ctrl) val += 4;
  return val;
}

/**
 * Encode a CSI sequence with optional modifier.
 * For tilde-sequences like "3~", becomes \x1b[3;{mod}~ when modified.
 * When unmodified (mod=1), becomes \x1b[3~.
 */
function encodeCSI(seq: string, mod: number): Uint8Array {
  if (mod > 1 && seq.endsWith("~")) {
    // Insert modifier before tilde: "3~" → "3;2~"
    const base = seq.slice(0, -1);
    return encoder.encode(`\x1b[${base};${mod}~`);
  }
  return encoder.encode(`\x1b[${seq}`);
}

/**
 * Encode arrow/Home/End keys.
 * Unmodified: \x1b[A (CSI). Modified: \x1b[1;{mod}A.
 */
function encodeSS3OrCSI(letter: string, mod: number): Uint8Array {
  if (mod > 1) {
    return encoder.encode(`\x1b[1;${mod}${letter}`);
  }
  return encoder.encode(`\x1b[${letter}`);
}
