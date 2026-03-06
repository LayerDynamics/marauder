/**
 * Default configuration values matching `pkg/config-store/src/defaults.rs`.
 */

import type { MarauderConfig } from "./schema.ts";

/** Default configuration matching Rust defaults in `pkg/config-store/src/defaults.rs`. */
export const DEFAULT_CONFIG: MarauderConfig = {
  terminal: {
    shell: Deno.env.get("SHELL") ?? "/bin/sh",
    scrollback: 10000,
    rows: 24,
    cols: 80,
  },
  font: {
    family: "monospace",
    size: 14,
    line_height: 1.2,
  },
  cursor: {
    style: "block",
    blink: true,
  },
  window: {
    opacity: 1.0,
    decorations: true,
  },
  theme: {},
};
