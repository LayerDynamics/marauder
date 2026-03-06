/**
 * @marauder/config — Typed configuration schema
 *
 * Defines the full MarauderConfig type hierarchy with validation.
 */

// defaults.ts only imports *types* from this file, so no circular dependency at runtime.
import { DEFAULT_CONFIG } from "./defaults.ts";

/** Terminal emulator settings. */
export interface TerminalConfig {
  /** Shell executable path. Defaults to $SHELL or /bin/sh. */
  shell: string;
  /** Scrollback buffer size in lines. */
  scrollback: number;
  /** Initial row count. */
  rows: number;
  /** Initial column count. */
  cols: number;
}

/** Font rendering settings. */
export interface FontConfig {
  /** Font family name. */
  family: string;
  /** Font size in points. */
  size: number;
  /** Line height multiplier. */
  line_height: number;
}

/** Cursor appearance settings. */
export interface CursorConfig {
  /** Cursor style: "block", "underline", or "bar". */
  style: "block" | "underline" | "bar";
  /** Whether the cursor blinks. */
  blink: boolean;
}

/** Window chrome settings. */
export interface WindowConfig {
  /** Window opacity (0.0 to 1.0). */
  opacity: number;
  /** Whether to show window decorations. */
  decorations: boolean;
}

/** Theme color configuration. */
export interface ThemeConfig {
  /** Background color as [R, G, B, A] (0-255). */
  background?: [number, number, number, number];
  /** Foreground color as [R, G, B, A] (0-255). */
  foreground?: [number, number, number, number];
  /** Cursor color as [R, G, B, A] (0-255). */
  cursor?: [number, number, number, number];
  /** Selection highlight color as [R, G, B, A] (0-255). */
  selection?: [number, number, number, number];
  /** ANSI color palette (16 colors). */
  palette?: Array<[number, number, number]>;
}

/** Top-level Marauder configuration. */
export interface MarauderConfig {
  terminal: TerminalConfig;
  font: FontConfig;
  cursor: CursorConfig;
  window: WindowConfig;
  keybindings: Record<string, string>;
  theme?: ThemeConfig;
}

/**
 * Validate and fill defaults for a raw config object.
 * Returns a complete MarauderConfig with all required fields populated.
 */
export function validateConfig(raw: unknown): MarauderConfig {
  if (raw === null || raw === undefined || typeof raw !== "object") {
    return { ...DEFAULT_CONFIG };
  }

  const obj = raw as Record<string, unknown>;

  return {
    terminal: mergeSection(DEFAULT_CONFIG.terminal, obj.terminal),
    font: mergeSection(DEFAULT_CONFIG.font, obj.font),
    cursor: mergeSection(DEFAULT_CONFIG.cursor, obj.cursor),
    window: mergeSection(DEFAULT_CONFIG.window, obj.window),
    keybindings: (typeof obj.keybindings === "object" && obj.keybindings !== null)
      ? obj.keybindings as Record<string, string>
      : { ...DEFAULT_CONFIG.keybindings },
    theme: obj.theme as ThemeConfig | undefined,
  };
}

/** Merge a partial section with its defaults. */
function mergeSection<T extends Record<string, unknown>>(
  defaults: T,
  partial: unknown,
): T {
  if (partial === null || partial === undefined || typeof partial !== "object") {
    return { ...defaults };
  }
  return { ...defaults, ...partial as Partial<T> };
}

// Re-export DEFAULT_CONFIG so consumers can import from either module.
export { DEFAULT_CONFIG } from "./defaults.ts";
