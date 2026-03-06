/**
 * @marauder/ui/user — Key sequence parser
 *
 * Normalizes KeyboardEvent objects into canonical key sequence strings
 * like "Ctrl+Shift+T", "Alt+A", "F5".
 */

/**
 * Parse a KeyboardEvent into a canonical key sequence string.
 *
 * Modifier ordering is deterministic: Ctrl -> Alt -> Shift -> Meta.
 * The key itself is normalized to uppercase for letters.
 *
 * Examples: "Ctrl+C", "Alt+F4", "Ctrl+Shift+Tab", "F12", "Enter"
 */
export function parseKeySequence(e: KeyboardEvent): string {
  const parts: string[] = [];

  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Meta");

  const key = normalizeKey(e.key, e.code);

  // Don't include modifier keys as the main key
  if (isModifierKey(key)) return parts.join("+");

  parts.push(key);
  return parts.join("+");
}

/** Normalize the key value to a canonical form. */
function normalizeKey(key: string, code: string): string {
  // Check special single-char keys before the generic single-char path
  switch (key) {
    case " ": return "Space";
    case "+": return "Plus";
    case "-": return "Minus";
    case "ArrowUp": return "Up";
    case "ArrowDown": return "Down";
    case "ArrowLeft": return "Left";
    case "ArrowRight": return "Right";
    default: break;
  }

  // Single printable character
  if (key.length === 1) {
    if (key >= "a" && key <= "z") return key.toUpperCase();
    return key;
  }

  // Handle numpad via code if key is ambiguous
  if (code.startsWith("Numpad")) {
    return code;
  }

  return key;
}

/** Check if a key name is a modifier-only key. */
function isModifierKey(key: string): boolean {
  return key === "Control" || key === "Shift" || key === "Alt" || key === "Meta";
}
