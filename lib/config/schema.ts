/**
 * Configuration schema — typed interfaces for all Marauder config sections.
 */

import type { RGBA } from "../ui/styling/color-scheme.ts";
import { DEFAULT_CONFIG } from "./defaults.ts";

/** Terminal emulator settings. */
export interface TerminalConfig {
  /** Shell executable path. */
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
  /** Cursor shape: "block", "underline", or "bar". */
  style: "block" | "underline" | "bar";
  /** Whether the cursor blinks. */
  blink: boolean;
}

/** Window appearance settings. */
export interface WindowConfig {
  /** Window opacity (0.0 = fully transparent, 1.0 = fully opaque). */
  opacity: number;
  /** Whether to show native window decorations. */
  decorations: boolean;
}

/** Theme / color scheme settings. */
export interface ThemeConfig {
  /** Terminal background color as RGBA tuple. */
  background?: RGBA;
  /** Default text foreground color as RGBA tuple. */
  foreground?: RGBA;
  /** Cursor color as RGBA tuple. */
  cursor?: RGBA;
  /** Selection highlight color as RGBA tuple. */
  selection?: RGBA;
  /** 16-color ANSI palette. Each entry is an RGBA tuple. */
  palette?: RGBA[];
}

/** Top-level Marauder configuration. */
export interface MarauderConfig {
  terminal: TerminalConfig;
  font: FontConfig;
  cursor: CursorConfig;
  window: WindowConfig;
  theme: ThemeConfig;
}

/**
 * Validate and coerce a raw config object into a MarauderConfig.
 * Missing sections are filled with defaults from DEFAULT_CONFIG.
 */
export function validateConfig(raw: unknown, defaults?: MarauderConfig): MarauderConfig {
  const effectiveDefaults = defaults ?? DEFAULT_CONFIG;

  if (raw == null || typeof raw !== "object") {
    return { ...effectiveDefaults };
  }

  const obj = raw as Record<string, unknown>;

  const config: MarauderConfig = {
    terminal: mergeSection(obj.terminal, effectiveDefaults.terminal) as TerminalConfig,
    font: mergeSection(obj.font, effectiveDefaults.font) as FontConfig,
    cursor: mergeSection(obj.cursor, effectiveDefaults.cursor) as CursorConfig,
    window: mergeSection(obj.window, effectiveDefaults.window) as WindowConfig,
    theme: mergeSection(obj.theme, effectiveDefaults.theme) as ThemeConfig,
  };

  return enforceConstraints(config, effectiveDefaults);
}

/** Merge a partial section with defaults. */
function mergeSection<T extends Record<string, unknown>>(
  partial: unknown,
  defaults: T,
): T {
  if (partial == null || typeof partial !== "object") {
    return { ...defaults };
  }
  return { ...defaults, ...(partial as Partial<T>) };
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
  if (th.background != null && !isRGBA(th.background)) th.background = undefined;
  if (th.foreground != null && !isRGBA(th.foreground)) th.foreground = undefined;
  if (th.cursor != null && !isRGBA(th.cursor)) th.cursor = undefined;
  if (th.selection != null && !isRGBA(th.selection)) th.selection = undefined;
  if (th.palette != null && !isValidPalette(th.palette)) th.palette = undefined;

  return config;
}

function isRGBA(v: unknown): v is RGBA {
  return Array.isArray(v) && v.length === 4 && v.every((n) => typeof n === "number" && n >= 0 && n <= 255);
}

function isValidPalette(v: unknown): v is RGBA[] {
  return Array.isArray(v) && v.length === 16 && v.every((entry) => isRGBA(entry));
}

