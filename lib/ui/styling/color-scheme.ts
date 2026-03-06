/// <reference lib="dom" />
/**
 * Color scheme utilities — RGBA manipulation, WCAG contrast, and ColorScheme class.
 */

import type { ThemeConfig } from "../../config/schema.ts";

/** RGBA tuple: [red, green, blue, alpha] each in 0-255 range. */
export type RGBA = [number, number, number, number];

// ---------------------------------------------------------------------------
// Catppuccin Mocha defaults
// ---------------------------------------------------------------------------

const MOCHA_BG: RGBA = [30, 30, 46, 255];
const MOCHA_FG: RGBA = [205, 214, 244, 255];
const MOCHA_CURSOR: RGBA = [243, 139, 168, 255];
const MOCHA_SELECTION: RGBA = [88, 91, 112, 180];

/** Catppuccin Mocha 16-color ANSI palette (alpha=255). */
const MOCHA_PALETTE: RGBA[] = [
  [69, 71, 90, 255],   // 0  black
  [243, 139, 168, 255], // 1  red
  [166, 227, 161, 255], // 2  green
  [249, 226, 175, 255], // 3  yellow
  [137, 180, 250, 255], // 4  blue
  [245, 194, 231, 255], // 5  magenta
  [148, 226, 213, 255], // 6  cyan
  [186, 194, 222, 255], // 7  white
  [88, 91, 112, 255],   // 8  bright black
  [243, 139, 168, 255], // 9  bright red
  [166, 227, 161, 255], // 10 bright green
  [249, 226, 175, 255], // 11 bright yellow
  [137, 180, 250, 255], // 12 bright blue
  [245, 194, 231, 255], // 13 bright magenta
  [148, 226, 213, 255], // 14 bright cyan
  [205, 214, 244, 255], // 15 bright white
];

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/** Convert an RGBA tuple to a CSS hex string (e.g. "#1e1e2eff"). */
export function rgbaToHex(rgba: RGBA): string {
  const [r, g, b, a] = rgba;
  const toHex = (n: number): string =>
    Math.round(Math.max(0, Math.min(255, n))).toString(16).padStart(2, "0");
  return `#${toHex(r)}${toHex(g)}${toHex(b)}${toHex(a)}`;
}

/**
 * Parse a CSS hex string into an RGBA tuple.
 * Supports: #rgb, #rgba, #rrggbb, #rrggbbaa (case-insensitive, with or without #).
 */
export function hexToRgba(hex: string): RGBA {
  const raw = hex.startsWith("#") ? hex.slice(1) : hex;
  let r: number, g: number, b: number, a: number;

  if (raw.length === 3) {
    r = parseInt(raw.charAt(0) + raw.charAt(0), 16);
    g = parseInt(raw.charAt(1) + raw.charAt(1), 16);
    b = parseInt(raw.charAt(2) + raw.charAt(2), 16);
    a = 255;
  } else if (raw.length === 4) {
    r = parseInt(raw.charAt(0) + raw.charAt(0), 16);
    g = parseInt(raw.charAt(1) + raw.charAt(1), 16);
    b = parseInt(raw.charAt(2) + raw.charAt(2), 16);
    a = parseInt(raw.charAt(3) + raw.charAt(3), 16);
  } else if (raw.length === 6) {
    r = parseInt(raw.slice(0, 2), 16);
    g = parseInt(raw.slice(2, 4), 16);
    b = parseInt(raw.slice(4, 6), 16);
    a = 255;
  } else if (raw.length === 8) {
    r = parseInt(raw.slice(0, 2), 16);
    g = parseInt(raw.slice(2, 4), 16);
    b = parseInt(raw.slice(4, 6), 16);
    a = parseInt(raw.slice(6, 8), 16);
  } else {
    throw new Error(`Invalid hex color: "${hex}"`);
  }

  return [r, g, b, a];
}

/**
 * Lighten an RGBA color by mixing toward white.
 * @param rgba - Source color.
 * @param amount - 0 (no change) to 1 (pure white).
 */
export function lighten(rgba: RGBA, amount: number): RGBA {
  const t = Math.max(0, Math.min(1, amount));
  return [
    Math.round(rgba[0] + (255 - rgba[0]) * t),
    Math.round(rgba[1] + (255 - rgba[1]) * t),
    Math.round(rgba[2] + (255 - rgba[2]) * t),
    rgba[3],
  ];
}

/**
 * Darken an RGBA color by mixing toward black.
 * @param rgba - Source color.
 * @param amount - 0 (no change) to 1 (pure black).
 */
export function darken(rgba: RGBA, amount: number): RGBA {
  const t = Math.max(0, Math.min(1, amount));
  return [
    Math.round(rgba[0] * (1 - t)),
    Math.round(rgba[1] * (1 - t)),
    Math.round(rgba[2] * (1 - t)),
    rgba[3],
  ];
}

/** Compute relative luminance for a linear RGB channel value (0-1). */
function linearize(channel: number): number {
  const c = channel / 255;
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

/** Compute WCAG 2.1 relative luminance of an RGBA color. */
function relativeLuminance(rgba: RGBA): number {
  const r = linearize(rgba[0]);
  const g = linearize(rgba[1]);
  const b = linearize(rgba[2]);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * Compute WCAG 2.1 contrast ratio between two colors.
 * Returns a value between 1 (no contrast) and 21 (black on white).
 */
export function contrastRatio(fg: RGBA, bg: RGBA): number {
  const l1 = relativeLuminance(fg);
  const l2 = relativeLuminance(bg);
  const lighter = Math.max(l1, l2);
  const darker = Math.min(l1, l2);
  return (lighter + 0.05) / (darker + 0.05);
}

/**
 * Adjust fg color to meet a minimum WCAG contrast ratio against bg.
 * Iteratively lightens or darkens fg in small steps until the ratio is met
 * or the adjustment limit is reached. Returns the adjusted color.
 *
 * @param fg - Foreground color to adjust.
 * @param bg - Background color.
 * @param minRatio - Minimum acceptable contrast ratio (default 4.5, WCAG AA).
 */
export function ensureContrast(fg: RGBA, bg: RGBA, minRatio = 4.5): RGBA {
  if (contrastRatio(fg, bg) >= minRatio) return fg;

  const bgLum = relativeLuminance(bg);
  // Decide direction: lighten if bg is dark, darken if bg is light.
  const shouldLighten = bgLum < 0.5;

  let adjusted: RGBA = [...fg] as RGBA;
  const step = 0.02;

  for (let i = 0; i < 50; i++) {
    adjusted = shouldLighten
      ? lighten(adjusted, step)
      : darken(adjusted, step);
    if (contrastRatio(adjusted, bg) >= minRatio) break;
  }

  return adjusted;
}

// ---------------------------------------------------------------------------
// ColorScheme class
// ---------------------------------------------------------------------------

/**
 * Provides typed color accessors and CSS variable generation from a ThemeConfig.
 */
export class ColorScheme {
  readonly #background: RGBA;
  readonly #foreground: RGBA;
  readonly #cursor: RGBA;
  readonly #selection: RGBA;
  readonly #palette: RGBA[];

  constructor(theme: ThemeConfig) {
    this.#background = (theme.background as RGBA | undefined) ?? [...MOCHA_BG] as RGBA;
    this.#foreground = (theme.foreground as RGBA | undefined) ?? [...MOCHA_FG] as RGBA;
    this.#cursor = (theme.cursor as RGBA | undefined) ?? [...MOCHA_CURSOR] as RGBA;
    this.#selection = (theme.selection as RGBA | undefined) ?? [...MOCHA_SELECTION] as RGBA;

    if (theme.palette && theme.palette.length > 0) {
      this.#palette = theme.palette.map(
        ([r, g, b]) => [r, g, b, 255] as RGBA,
      );
      // Pad to 16 if fewer entries provided.
      while (this.#palette.length < 16) {
        this.#palette.push([...MOCHA_PALETTE[this.#palette.length]!] as RGBA);
      }
    } else {
      this.#palette = MOCHA_PALETTE.map((c) => [...c] as RGBA);
    }
  }

  /** Terminal background color. */
  get background(): RGBA {
    return [...this.#background] as RGBA;
  }

  /** Default text foreground color. */
  get foreground(): RGBA {
    return [...this.#foreground] as RGBA;
  }

  /** Cursor color. */
  get cursor(): RGBA {
    return [...this.#cursor] as RGBA;
  }

  /** Selection highlight color. */
  get selection(): RGBA {
    return [...this.#selection] as RGBA;
  }

  /**
   * Get an ANSI color by palette index (0-15).
   * Indices 16+ clamp to the nearest valid index.
   */
  getAnsiColor(index: number): RGBA {
    const i = Math.max(0, Math.min(15, Math.floor(index)));
    return [...this.#palette[i]!] as RGBA;
  }

  /**
   * Generate a CSS variable map for use with `element.style.setProperty`.
   * Keys are in the form `--var-name`, values are CSS color strings.
   *
   * Maps to the CSS variables defined in apps/marauder/src/styles.css:
   *   --bg-chrome, --fg-chrome, --accent, --tab-active, --tab-hover,
   *   --status-bg, --font-mono
   *
   * Also emits all 16 ANSI palette colors as --ansi-0 … --ansi-15.
   */
  toCssVariables(): Record<string, string> {
    const toRgbaStr = (c: RGBA): string =>
      `rgba(${c[0]}, ${c[1]}, ${c[2]}, ${(c[3] / 255).toFixed(3)})`;

    const accent = lighten(this.#cursor, 0.1);
    const tabActive = lighten(this.#background, 0.12);
    const tabHover = lighten(this.#background, 0.06);
    const statusBg = darken(this.#background, 0.08);

    const vars: Record<string, string> = {
      "--bg-chrome": toRgbaStr(this.#background),
      "--fg-chrome": toRgbaStr(this.#foreground),
      "--accent": toRgbaStr(accent),
      "--tab-active": toRgbaStr(tabActive),
      "--tab-hover": toRgbaStr(tabHover),
      "--status-bg": toRgbaStr(statusBg),
      // --font-mono is set by FontMetrics.toCssVariables()
      "--terminal-bg": toRgbaStr(this.#background),
      "--terminal-fg": toRgbaStr(this.#foreground),
      "--cursor-color": toRgbaStr(this.#cursor),
      "--selection-color": toRgbaStr(this.#selection),
    };

    for (let i = 0; i < 16; i++) {
      vars[`--ansi-${i}`] = toRgbaStr(this.#palette[i]!);
    }

    return vars;
  }

  /** Construct a ColorScheme using Catppuccin Mocha defaults. */
  static fromDefaults(): ColorScheme {
    return new ColorScheme({});
  }
}
