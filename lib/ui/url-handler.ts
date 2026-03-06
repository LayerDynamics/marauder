/**
 * URL detection and click handling for the terminal grid.
 *
 * Detects URLs in visible terminal rows and provides click-to-open behavior.
 * Uses the GPU compute engine for URL detection when available, with a
 * JavaScript fallback for environments without compute shader support.
 */

/** A detected URL match with its cell coordinates. */
export interface UrlMatch {
  row: number;
  startCol: number;
  endCol: number;
  url: string;
}

/** Common URL pattern for JavaScript-side detection fallback. */
const URL_REGEX = /https?:\/\/[^\s<>"'`)\]},;]+/g;

/** Trailing characters that are valid URL chars but almost never end a URL in prose. */
const TRAILING_PUNCT_RE = /[.:,;!?)]+$/;

/**
 * Detect URLs in a screen snapshot row.
 * This is the JS fallback — the GPU compute shader (url_detect.wgsl)
 * is preferred for performance on large scrollback buffers.
 */
export function detectUrlsInRow(
  row: number,
  text: string
): UrlMatch[] {
  const matches: UrlMatch[] = [];
  let m: RegExpExecArray | null;
  URL_REGEX.lastIndex = 0;
  while ((m = URL_REGEX.exec(text)) !== null) {
    // Strip trailing punctuation that's valid in URLs but rarely ends one in prose
    const cleaned = m[0].replace(TRAILING_PUNCT_RE, "");
    if (cleaned.length < 10) continue; // Too short after stripping — skip (e.g., "http://a.")
    matches.push({
      row,
      startCol: m.index,
      endCol: m.index + cleaned.length - 1,
      url: cleaned,
    });
  }
  return matches;
}

/**
 * Check if a cell position is within any detected URL match.
 * Returns the URL if found, null otherwise.
 */
export function findUrlAtCell(
  matches: UrlMatch[],
  row: number,
  col: number
): string | null {
  for (const m of matches) {
    if (m.row === row && col >= m.startCol && col <= m.endCol) {
      return m.url;
    }
  }
  return null;
}

/**
 * Open a URL in the system default browser.
 * Uses Tauri's opener plugin if available, falls back to window.open.
 */
export async function openUrl(url: string): Promise<void> {
  try {
    const { open } = await import("@tauri-apps/plugin-opener");
    await open(url);
  } catch {
    window.open(url, "_blank");
  }
}
