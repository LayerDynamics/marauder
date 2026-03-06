/**
 * Font metrics — wraps FontConfig with cell dimension calculations and CSS helpers.
 */

import type { FontConfig } from "../../config/schema.ts";

/**
 * FontMetrics wraps a FontConfig and provides computed layout values needed
 * by both the GPU renderer and the webview chrome.
 */
export class FontMetrics {
  readonly #config: FontConfig;

  constructor(config: FontConfig) {
    this.#config = { ...config };
  }

  /**
   * Estimated cell width in pixels.
   * Monospace fonts have a glyph width approximately 0.6× the point size.
   * The GPU renderer will override this with the actual rasterized width,
   * but this estimate is used for initial grid calculations before the first frame.
   */
  get cellWidth(): number {
    return this.#config.size * 0.6;
  }

  /**
   * Cell height in pixels — point size multiplied by the line height multiplier.
   */
  get cellHeight(): number {
    return this.#config.size * this.#config.line_height;
  }

  /**
   * CSS font shorthand string suitable for use in `element.style.font` or
   * canvas `context.font`.
   */
  get cssFont(): string {
    return `${this.#config.size}px ${this.#config.family}`;
  }

  /**
   * Generate a CSS @font-face declaration string.
   * Assumes the font files are served from the `/fonts/` path by Tauri's
   * asset protocol with the family name kebab-cased as the filename stem.
   */
  toCssFontFace(): string {
    const stem = this.#config.family
      .toLowerCase()
      .replace(/\s+/g, "-")
      .replace(/[^a-z0-9-]/g, "");

    return [
      "@font-face {",
      `  font-family: "${this.#config.family}";`,
      `  src: url("/fonts/${stem}.woff2") format("woff2"),`,
      `       url("/fonts/${stem}.woff") format("woff"),`,
      `       url("/fonts/${stem}.ttf") format("truetype");`,
      "  font-weight: normal;",
      "  font-style: normal;",
      "  font-display: swap;",
      "}",
    ].join("\n");
  }

  /**
   * Return the raw config in the shape expected by pkg/renderer's C ABI
   * (`renderer_set_font`), mirrored in ffi/renderer.
   */
  toRendererConfig(): { family: string; size: number; line_height: number } {
    return {
      family: this.#config.family,
      size: this.#config.size,
      line_height: this.#config.line_height,
    };
  }

  /**
   * Emit CSS custom properties for the font.
   * --font-mono matches the variable declared in apps/marauder/src/styles.css.
   */
  toCssVariables(): Record<string, string> {
    return {
      "--font-mono": `"${this.#config.family}", monospace`,
      "--font-size": `${this.#config.size}px`,
      "--line-height": String(this.#config.line_height),
      "--cell-width": `${this.cellWidth}px`,
      "--cell-height": `${this.cellHeight}px`,
    };
  }
}

/**
 * Calculate the number of terminal columns and rows that fit within a
 * pixel viewport given the cell dimensions from FontMetrics.
 *
 * @param metrics - Font metrics providing cellWidth and cellHeight.
 * @param width   - Viewport width in pixels (e.g. window.innerWidth).
 * @param height  - Viewport height in pixels (e.g. window.innerHeight minus chrome).
 * @returns cols and rows as positive integers (minimum 1×1).
 */
export function computeGridDimensions(
  metrics: FontMetrics,
  width: number,
  height: number,
): { cols: number; rows: number } {
  const cols = Math.max(1, Math.floor(width / metrics.cellWidth));
  const rows = Math.max(1, Math.floor(height / metrics.cellHeight));
  return { cols, rows };
}
