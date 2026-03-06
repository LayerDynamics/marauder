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
  /** Environment variables injected into the shell. */
  env?: Record<string, string>;
  /** Working directory for new sessions. */
  cwd?: string;
}

/** Font rendering settings. */
export interface FontConfig {
  /** Font family name (e.g. "monospace", "JetBrains Mono"). */
  family: string;
  /** Font size in points. */
  size: number;
  /** Line height multiplier (e.g. 1.2). */
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
export function validateConfig(raw: unknown, defaults: MarauderConfig): MarauderConfig {
  const effectiveDefaults = defaults;

  if (raw === null || raw === undefined || typeof raw !== "object") {
    return { ...effectiveDefaults };
  }

  const obj = raw as Record<string, unknown>;

  const config: MarauderConfig = {
    terminal: mergeSection(effectiveDefaults.terminal, obj.terminal),
    font: mergeSection(effectiveDefaults.font, obj.font),
    cursor: mergeSection(effectiveDefaults.cursor, obj.cursor),
    window: mergeSection(effectiveDefaults.window, obj.window),
    keybindings: (typeof obj.keybindings === "object" && obj.keybindings !== null)
      ? obj.keybindings as Record<string, string>
      : { ...effectiveDefaults.keybindings },
    theme: obj.theme as ThemeConfig | undefined,
  };

  return enforceConstraints(config, effectiveDefaults);
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

const VALID_CURSOR_STYLES = new Set(["block", "underline", "bar"]);

/**
 * Enforce runtime type and range constraints on config values.
 * Invalid values are replaced with their defaults.
 */
function enforceConstraints(config: MarauderConfig, defaults: MarauderConfig): MarauderConfig {
  const t = config.terminal;
  const td = defaults.terminal;
  if (typeof t.shell !== "string" || t.shell.length === 0) t.shell = td.shell;
  if (typeof t.scrollback !== "number" || !Number.isFinite(t.scrollback) || t.scrollback < 0) t.scrollback = td.scrollback;
  if (typeof t.rows !== "number" || !Number.isInteger(t.rows) || t.rows < 1) t.rows = td.rows;
  if (typeof t.cols !== "number" || !Number.isInteger(t.cols) || t.cols < 1) t.cols = td.cols;
  if (t.env != null && typeof t.env !== "object") t.env = undefined;
  if (t.cwd != null && typeof t.cwd !== "string") t.cwd = undefined;

  const f = config.font;
  const fd = defaults.font;
  if (typeof f.family !== "string" || f.family.length === 0) f.family = fd.family;
  if (typeof f.size !== "number" || !Number.isFinite(f.size) || f.size < 1 || f.size > 200) f.size = fd.size;
  if (typeof f.line_height !== "number" || !Number.isFinite(f.line_height) || f.line_height < 0.5 || f.line_height > 5) f.line_height = fd.line_height;

  const c = config.cursor;
  const cd = defaults.cursor;
  if (!VALID_CURSOR_STYLES.has(c.style)) c.style = cd.style;
  if (typeof c.blink !== "boolean") c.blink = cd.blink;

  const w = config.window;
  const wd = defaults.window;
  if (typeof w.opacity !== "number" || !Number.isFinite(w.opacity)) w.opacity = wd.opacity;
  else w.opacity = Math.max(0, Math.min(1, w.opacity));
  if (typeof w.decorations !== "boolean") w.decorations = wd.decorations;

  const th = config.theme;
  if (th) {
    if (th.background != null && !isRGBA(th.background)) th.background = undefined;
    if (th.foreground != null && !isRGBA(th.foreground)) th.foreground = undefined;
    if (th.cursor != null && !isRGBA(th.cursor)) th.cursor = undefined;
    if (th.selection != null && !isRGBA(th.selection)) th.selection = undefined;
    if (th.palette != null && !isValidPalette(th.palette)) th.palette = undefined;
  }

  return config;
}

function isRGBA(v: unknown): v is [number, number, number, number] {
  return Array.isArray(v) && v.length === 4 && v.every((n) => typeof n === "number" && n >= 0 && n <= 255);
}

function isValidPalette(v: unknown): v is Array<[number, number, number]> {
  return Array.isArray(v) && v.length === 16 &&
    v.every((entry) => Array.isArray(entry) && entry.length === 3 && entry.every((n: unknown) => typeof n === "number" && n >= 0 && n <= 255));
}

// Re-export DEFAULT_CONFIG so consumers can import from either module.
export { DEFAULT_CONFIG } from "./defaults.ts";
