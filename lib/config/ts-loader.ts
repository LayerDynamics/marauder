/**
 * @marauder/config — TypeScript config file loader
 *
 * Supports loading config from `.ts` files alongside TOML.
 * Uses dynamic import to load TypeScript config with full type safety.
 */

import type { MarauderConfig } from "./schema.ts";
import { validateConfig } from "./schema.ts";

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
    // Dynamic import of the TS config file
    const mod = await import(path);
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
            // Add cache-busting query param so Deno re-imports
            const config = await loadTsConfig(`${path}#${Date.now()}`);
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
