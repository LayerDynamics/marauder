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

/** Allowed directory prefixes for config file loading (resolved at runtime). */
function getAllowedRoots(): string[] {
  const roots: string[] = [Deno.cwd()];
  const home = Deno.env.get("HOME") ?? Deno.env.get("USERPROFILE");
  if (home) roots.push(home);
  return roots.map((r) => r.endsWith("/") ? r : `${r}/`);
}

/**
 * Validate that a config path is safe to import:
 * - Must end with `.ts`
 * - Must resolve to a path under CWD or HOME
 * - Must not contain path traversal after resolution
 */
function validateConfigPath(path: string): string {
  const resolved = Deno.realPathSync(path);

  if (!resolved.endsWith(".ts")) {
    throw new Error(`Config path must be a .ts file: ${resolved}`);
  }

  const allowedRoots = getAllowedRoots();
  const isAllowed = allowedRoots.some((root) => resolved.startsWith(root));
  if (!isAllowed) {
    throw new Error(`Config path must be under CWD or HOME: ${resolved}`);
  }

  return resolved;
}

/** Well-known config file paths, checked in order. */
const CONFIG_DIRS = [
  () => `${Deno.env.get("HOME") ?? "~"}/.config/marauder`,
  () => `${Deno.cwd()}/.marauder`,
];

const CONFIG_FILENAMES = ["config.ts", "config.toml"];

/**
 * Monotonic counter for cache-busting — avoids unbounded module cache growth
 * from Date.now() which creates a new entry per millisecond.
 */
let reloadCounter = 0;

/**
 * Load a TypeScript config file and validate it against the schema.
 * Expects the file to have a default export matching Partial<MarauderConfig>.
 *
 * @param path Absolute path to the .ts config file
 * @returns Validated partial config
 */
export async function loadTsConfig(path: string): Promise<Partial<MarauderConfig>> {
  try {
    const resolved = validateConfigPath(path);
    const specifier = toFileUrl(resolved);
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
 * Returns an object with a close() method to stop watching.
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
            // Validate path and use monotonic counter for cache-busting
            const resolved = validateConfigPath(path);
            reloadCounter++;
            const url = `${toFileUrl(resolved)}?v=${reloadCounter}`;
            const mod = await import(url);
            const raw = mod.default ?? mod;
            const config = validateConfig(raw);
            const result: Partial<MarauderConfig> = {};
            if (raw.terminal) result.terminal = config.terminal;
            if (raw.font) result.font = config.font;
            if (raw.cursor) result.cursor = config.cursor;
            if (raw.window) result.window = config.window;
            if (raw.keybindings) result.keybindings = config.keybindings;
            if (raw.theme) result.theme = config.theme;
            callback(result);
          } catch (e) {
            console.error(`Failed to reload config from ${path}:`, e);
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
