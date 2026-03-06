/// <reference lib="dom" />
/**
 * Accessibility helpers — media query checks, ARIA attributes, focus management,
 * and screen-reader announcements for the terminal UI.
 */

// ---------------------------------------------------------------------------
// Media query helpers
// ---------------------------------------------------------------------------

/**
 * Returns true if the user's OS is configured to prefer high-contrast mode.
 * Falls back to false in non-browser environments (e.g. Deno CLI).
 */
export function prefersHighContrast(): boolean {
  try {
    return globalThis.matchMedia?.("(prefers-contrast: more)")?.matches ?? false;
  } catch {
    return false;
  }
}

/**
 * Returns true if the user's OS requests reduced motion (e.g. for cursor blink).
 * Falls back to false in non-browser environments.
 */
export function prefersReducedMotion(): boolean {
  try {
    return (
      globalThis.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches ??
      false
    );
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// ARIA attribute helpers
// ---------------------------------------------------------------------------

/**
 * Returns the ARIA attributes that should be applied to the terminal grid
 * container element. The grid is a live region so screen readers announce
 * new output, but the rate is controlled to avoid flooding.
 */
export function ariaTerminalAttrs(): Record<string, string> {
  return {
    role: "region",
    "aria-label": "Terminal",
    "aria-live": "polite",
    "aria-atomic": "false",
    "aria-relevant": "additions text",
    "aria-description":
      "Interactive terminal emulator. Use keyboard to interact with the shell.",
  };
}

// ---------------------------------------------------------------------------
// Focus management
// ---------------------------------------------------------------------------

/**
 * Configure an element to participate correctly in the tab order and receive
 * keyboard events for terminal input.
 *
 * - Sets tabindex="0" so the element is reachable via Tab.
 * - Focuses the element immediately if it is not already focused.
 * - Adds a visible focus indicator via the `data-focused` attribute so CSS
 *   can style it without relying solely on :focus (which may be suppressed
 *   by `outline: none` rules on the transparent webview body).
 */
export function manageFocus(element: HTMLElement): void {
  element.tabIndex = 0;

  const onFocus = (): void => {
    element.dataset.focused = "true";
  };

  const onBlur = (): void => {
    delete element.dataset.focused;
  };

  element.addEventListener("focus", onFocus);
  element.addEventListener("blur", onBlur);

  // Give focus if the document already has focus (i.e. the window is active).
  if (
    typeof document !== "undefined" &&
    document.hasFocus() &&
    document.activeElement !== element
  ) {
    element.focus({ preventScroll: true });
  }
}

// ---------------------------------------------------------------------------
// Screen-reader announcements
// ---------------------------------------------------------------------------

/** Singleton live region element used for polite announcements. */
let _liveRegion: HTMLElement | null = null;

function getLiveRegion(): HTMLElement | null {
  if (typeof document === "undefined") return null;

  if (_liveRegion) return _liveRegion;

  const el = document.createElement("div");
  el.setAttribute("aria-live", "polite");
  el.setAttribute("aria-atomic", "true");
  el.setAttribute("aria-relevant", "additions");

  // Visually hidden but accessible to screen readers.
  Object.assign(el.style, {
    position: "absolute",
    width: "1px",
    height: "1px",
    padding: "0",
    margin: "-1px",
    overflow: "hidden",
    clip: "rect(0,0,0,0)",
    whiteSpace: "nowrap",
    border: "0",
  });

  document.body.appendChild(el);
  _liveRegion = el;
  return el;
}

/**
 * Announce a short text message to screen readers via a polite ARIA live
 * region. The announcement clears after 1 second to allow the same message
 * to be repeated if needed.
 *
 * Has no effect in non-browser environments.
 *
 * @param text - The message to announce (keep brief: <100 chars).
 */
export function announceToScreenReader(text: string): void {
  const region = getLiveRegion();
  if (!region) return;

  // Clear first to force re-announcement if the same text is sent twice.
  region.textContent = "";

  // Use a microtask so the DOM mutation is observed as two separate changes.
  queueMicrotask(() => {
    region.textContent = text;

    // Clear after 1 second to reset state.
    setTimeout(() => {
      if (region.textContent === text) {
        region.textContent = "";
      }
    }, 1_000);
  });
}
