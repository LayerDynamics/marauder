/// <reference lib="dom" />
/**
 * StyleConfig — unified theme + font + window configuration, and DOM helpers
 * for applying/removing CSS custom properties on elements.
 */

import type { ThemeConfig, FontConfig, WindowConfig } from "../../config/schema.ts";
import { ColorScheme } from "./color-scheme.ts";
import { FontMetrics } from "./font.ts";

// ---------------------------------------------------------------------------
// StyleConfig interface
// ---------------------------------------------------------------------------

/**
 * Unified styling configuration that combines all visual subsystems.
 * Extends the individual config slices so consumers can pass a single object
 * to all styling utilities.
 */
export interface StyleConfig {
  theme: ThemeConfig;
  font: FontConfig;
  window: WindowConfig;
}

// ---------------------------------------------------------------------------
// CSS variable generation
// ---------------------------------------------------------------------------

/**
 * Generate the full set of CSS custom property assignments for a StyleConfig.
 *
 * The returned map covers:
 *   - Color variables (from ColorScheme): --bg-chrome, --fg-chrome, --accent,
 *     --tab-active, --tab-hover, --status-bg, --terminal-bg, --terminal-fg,
 *     --cursor-color, --selection-color, --ansi-0 … --ansi-15
 *   - Font variables (from FontMetrics): --font-mono, --font-size,
 *     --line-height, --cell-width, --cell-height
 *   - Window variables: --window-opacity
 *
 * Keys are CSS custom property names (e.g. "--bg-chrome").
 * Values are ready-to-use CSS value strings.
 */
export function generateCssVariables(config: StyleConfig): Record<string, string> {
  const scheme = new ColorScheme(config.theme);
  const metrics = new FontMetrics(config.font);

  const vars: Record<string, string> = {
    ...scheme.toCssVariables(),
    ...metrics.toCssVariables(),
    "--window-opacity": String(
      Math.max(0, Math.min(1, config.window.opacity)),
    ),
  };

  return vars;
}

// ---------------------------------------------------------------------------
// DOM helpers
// ---------------------------------------------------------------------------

/**
 * Apply all StyleConfig CSS variables to an element's inline style.
 * Typically called on `document.documentElement` or a container element.
 *
 * @param element - Target HTML element.
 * @param config  - Styling configuration to apply.
 */
export function applyTheme(element: HTMLElement, config: StyleConfig): void {
  const vars = generateCssVariables(config);
  for (const [key, value] of Object.entries(vars)) {
    element.style.setProperty(key, value);
  }

  // Also set window opacity directly — the webview background is transparent
  // and the chrome opacity is controlled via the --window-opacity variable,
  // but some platforms need it on the element directly.
  element.style.setProperty(
    "--window-opacity",
    String(Math.max(0, Math.min(1, config.window.opacity))),
  );
}

/**
 * Remove all StyleConfig CSS variables from an element's inline style.
 * Restores the element to stylesheet-controlled values.
 *
 * @param element - Target HTML element.
 */
export function removeTheme(element: HTMLElement): void {
  // Remove color variables.
  const colorKeys = [
    "--bg-chrome",
    "--fg-chrome",
    "--accent",
    "--tab-active",
    "--tab-hover",
    "--status-bg",
    "--terminal-bg",
    "--terminal-fg",
    "--cursor-color",
    "--selection-color",
  ];

  // Remove ANSI palette variables.
  for (let i = 0; i < 16; i++) {
    colorKeys.push(`--ansi-${i}`);
  }

  // Remove font variables.
  const fontKeys = [
    "--font-mono",
    "--font-size",
    "--line-height",
    "--cell-width",
    "--cell-height",
  ];

  // Remove window variables.
  const windowKeys = ["--window-opacity"];

  for (const key of [...colorKeys, ...fontKeys, ...windowKeys]) {
    element.style.removeProperty(key);
  }
}
