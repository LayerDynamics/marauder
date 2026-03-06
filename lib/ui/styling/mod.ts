/**
 * @marauder/ui/styling — Public surface for all styling utilities.
 *
 * Re-exports:
 *   options.ts        — RenderMode, CursorShape, ScrollbarStyle, TabPosition
 *   color-scheme.ts   — RGBA, color math helpers, ColorScheme class
 *   font.ts           — FontMetrics class, computeGridDimensions
 *   accessibility.ts  — prefersHighContrast, prefersReducedMotion, ARIA helpers
 *   style-config.ts   — StyleConfig interface, generateCssVariables, applyTheme, removeTheme
 */

export { RenderMode, CursorShape, ScrollbarStyle, TabPosition } from "./options.ts";

export type { RGBA } from "./color-scheme.ts";
export {
  rgbaToHex,
  hexToRgba,
  lighten,
  darken,
  contrastRatio,
  ensureContrast,
  ColorScheme,
} from "./color-scheme.ts";

export { FontMetrics, computeGridDimensions } from "./font.ts";

export {
  prefersHighContrast,
  prefersReducedMotion,
  ariaTerminalAttrs,
  manageFocus,
  announceToScreenReader,
} from "./accessibility.ts";

export type { StyleConfig } from "./style-config.ts";
export {
  generateCssVariables,
  applyTheme,
  removeTheme,
} from "./style-config.ts";
