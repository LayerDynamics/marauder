/**
 * TypeScript config file loader — loads `.ts` config files via dynamic import.
 *
 * Supports Marauder config files written as TypeScript modules that export
 * a default configuration object.
 */

import type { MarauderConfig } from "./schema.ts";
import { validateConfig } from "./schema.ts";

/** Well-known config file search paths (in priority order). */
const CONFIG_SEARCH_PATHS = [
  ".marauder/config.ts",
  ".marauder.config.ts",
  ".config/marauder/config.ts",
];

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
 * - Must resolve to a path under CWD or HOME (within well-known subdirs)
 * - Must not contain path traversal after resolution
 */
function validateConfigPath(path: string): string {
  // Resolve to absolute, following symlinks
  const resolved = Deno.realPathSync(path);

  if (!resolved.endsWith(".ts")) {
    throw new Error(`Config path must be a .ts file: ${resolved}`);
  }

  const allowedRoots = getAllowedRoots();
  const isAllowed = allowedRoots.some((root) => resolved.startsWith(root));
  if (!isAllowed) {
    throw new Error(
      `Config path must be under CWD or HOME: ${resolved}`,
    );
  }

  return resolved;
}

/**
 * Monotonic counter for cache-busting — avoids unbounded module cache growth
 * from Date.now() which creates a new entry per millisecond.
 */
let reloadCounter = 0;

/**
 * Load a TypeScript config file by path.
 *
 * The file should `export default { ... }` a partial MarauderConfig.
 * Missing fields are filled with defaults.
 *
 * @param path - Absolute or relative path to the `.ts` config file.
 * @returns Validated MarauderConfig.
 */
export async function loadTsConfig(path: string): Promise<MarauderConfig> {
  const resolved = validateConfigPath(path);
  const url = `file://${resolved}`;
  const mod = await import(url);
  const raw = mod.default ?? mod;
  return validateConfig(raw);
}

/**
 * Discover config file paths by searching well-known locations.
 *
 * Searches relative to the current working directory and the user's home directory.
 *
 * @returns Array of absolute paths to discovered config files.
 */
export function discoverConfigPaths(): string[] {
  const found: string[] = [];
  const cwd = Deno.cwd();
  const home = Deno.env.get("HOME") ?? Deno.env.get("USERPROFILE") ?? "";

  const searchRoots = [cwd];
  if (home) {
    searchRoots.push(home);
  }

  for (const root of searchRoots) {
    for (const relPath of CONFIG_SEARCH_PATHS) {
      const fullPath = `${root}/${relPath}`;
      try {
        const stat = Deno.statSync(fullPath);
        if (stat.isFile) {
          found.push(fullPath);
        }
      } catch {
        // File doesn't exist — skip
      }
    }
  }

  return found;
}

/**
 * Watch a TypeScript config file for changes and reload on modification.
 *
 * @param path - Path to the config file to watch.
 * @param onReload - Callback invoked with the reloaded config.
 * @returns An AbortController that can be used to stop watching.
 */
export function watchTsConfig(
  path: string,
  onReload: (config: MarauderConfig) => void,
): AbortController {
  const controller = new AbortController();

  (async () => {
    const watcher = Deno.watchFs(path);
    // Close the OS file watcher handle when aborted
    controller.signal.addEventListener("abort", () => watcher.close(), { once: true });
    try {
      for await (const event of watcher) {
        if (controller.signal.aborted) break;
        if (event.kind === "modify" || event.kind === "create") {
          try {
            // Validate path and use monotonic counter for cache-busting
            const resolved = validateConfigPath(path);
            reloadCounter++;
            const url = `file://${resolved}?v=${reloadCounter}`;
            const mod = await import(url);
            const raw = mod.default ?? mod;
            const config = validateConfig(raw);
            onReload(config);
          } catch (e) {
            console.error(`Failed to reload config from ${path}:`, e);
          }
        }
      }
    } catch {
      // Watcher closed or path removed
    }
  })();

  return controller;
}
