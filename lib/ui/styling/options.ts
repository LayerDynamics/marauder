/**
 * Styling options — enumerations for renderer and UI configuration.
 */

/** Render mode: GPU-accelerated or software fallback. */
export enum RenderMode {
  Gpu = "gpu",
  Software = "software",
}

/** Cursor shape variants. */
export enum CursorShape {
  Block = "block",
  Underline = "underline",
  Bar = "bar",
}

/** Scrollbar visual style. */
export enum ScrollbarStyle {
  Overlay = "overlay",
  Always = "always",
  Hidden = "hidden",
}

/** Tab bar position. */
export enum TabPosition {
  Top = "top",
  Bottom = "bottom",
  Hidden = "hidden",
}
