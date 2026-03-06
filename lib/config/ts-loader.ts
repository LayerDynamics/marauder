/**
 * @marauder/config — TypeScript config file loader
 *
 * Supports loading config from `.ts` files alongside TOML.
 * Uses dynamic import to load TypeScript config with full type safety.
 */

import type { MarauderConfig } from "./schema.ts";
import { validateConfig } from "./schema.ts";

/**
 * Convert an absolute filesystem path to a file:// URL string.
 * Handles both Unix and Windows paths.
 */
function toFileUrl(path: string): string {
  // On Windows, paths like C:\Users\... need the extra slash: file:///C:/Users/...
  if (path.match(/^[A-Za-z]:\\/)) {
    return `file:///${path.replace(/\\/g, "/")}`;
  }
  // Unix absolute path: /home/user/... → file:///home/user/...
  return `file://${path}`;
}

/** Well-known config file paths, checked in order. */
const CONFIG_DIRS = [
  () => `${Deno.env.get("HOME") ?? "~"}/.config/marauder`,
  () => `${Deno.cwd()}/.marauder`,
];

const CONFIG_FILENAMES = ["config.ts", "config.toml"];

/**
 * Load a TypeScript config file and validate it against the schema.
 * Expects the file to have a default export matching Partial<MarauderConfig>.
 *
 * @param path Absolute path to the .ts config file
 * @returns Validated partial config
 */
export async function loadTsConfig(path: string): Promise<Partial<MarauderConfig>> {
  try {
    // Deno requires file:// URLs for absolute path imports.
    // Convert filesystem paths to proper URL specifiers, handling both
    // Unix (/home/user/...) and Windows (C:\Users\...) paths.
    const specifier = path.startsWith("file://") ? path : toFileUrl(path);
    const mod = await import(specifier);
    const raw = mod.default ?? mod;

    if (typeof raw !== "object" || raw === null) {
      throw new Error(`Config file ${path} must export an object`);
    }

    // Validate returns a full config, but we only want the partial overrides
    // that were actually specified in the file
    const validated = validateConfig(raw);
    const result: Partial<MarauderConfig> = {};

    if (raw.terminal) result.terminal = validated.terminal;
    if (raw.font) result.font = validated.font;
    if (raw.cursor) result.cursor = validated.cursor;
    if (raw.window) result.window = validated.window;
    if (raw.keybindings) result.keybindings = validated.keybindings;
    if (raw.theme) result.theme = validated.theme;

    return result;
  } catch (err) {
    if (err instanceof Deno.errors.NotFound) {
      return {};
    }
    throw err;
  }
}

/**
 * Discover config files in well-known locations.
 * Returns discovered paths grouped by type.
 */
export function discoverConfigPaths(): { toml: string[]; ts: string[] } {
  const toml: string[] = [];
  const ts: string[] = [];

  for (const dirFn of CONFIG_DIRS) {
    try {
      const dir = dirFn();
      for (const filename of CONFIG_FILENAMES) {
        const path = `${dir}/${filename}`;
        try {
          Deno.statSync(path);
          if (filename.endsWith(".ts")) {
            ts.push(path);
          } else {
            toml.push(path);
          }
        } catch {
          // File doesn't exist — skip
        }
      }
    } catch {
      // Dir resolution failed — skip
    }
  }

  return { toml, ts };
}

/**
 * Watch a TypeScript config file for changes.
 * Returns an async iterator of change events.
 */
export function watchTsConfig(
  path: string,
  callback: (config: Partial<MarauderConfig>) => void,
): { close: () => void } {
  const watcher = Deno.watchFs(path);
  let closed = false;

  (async () => {
    try {
      for await (const event of watcher) {
        if (closed) break;
        if (event.kind === "modify" || event.kind === "create") {
          try {
            // Cache-bust via query param on the file:// URL so Deno re-imports
            const url = `${toFileUrl(path)}?v=${Date.now()}`;
            const config = await loadTsConfig(url);
            callback(config);
          } catch {
            // Ignore errors from in-progress saves
          }
        }
      }
    } catch {
      // Watcher closed or errored
    }
  })();

  return {
    close() {
      closed = true;
      try {
        watcher.close();
      } catch {
        // Already closed
      }
    },
  };
}
