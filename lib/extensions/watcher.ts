// lib/extensions/watcher.ts
// Hot-reload file watcher for extensions (dev mode).

import type { ExtensionLoader } from "./loader.ts";

/** Debounce delay in milliseconds before triggering a reload. */
const DEBOUNCE_MS = 300;

/**
 * Watch extension directories for file changes and hot-reload affected
 * extensions. Uses Deno.watchFs() to detect modifications.
 */
export class ExtensionWatcher {
  readonly #loader: ExtensionLoader;
  readonly #dirs: string[];
  #watcher: Deno.FsWatcher | null = null;
  #running = false;
  /** Pending debounce timers keyed by extension name. */
  readonly #timers: Map<string, number> = new Map();

  constructor(loader: ExtensionLoader, dirs: string[]) {
    this.#loader = loader;
    this.#dirs = dirs;
  }

  /** Start watching. Does nothing if already watching. */
  start(): void {
    if (this.#running) return;
    this.#running = true;

    try {
      this.#watcher = Deno.watchFs(this.#dirs, { recursive: true });
    } catch (err) {
      console.error(
        `[marauder] Failed to start extension watcher:`,
        err,
      );
      this.#running = false;
      return;
    }

    this.#poll();
  }

  /** Stop watching and clean up. */
  stop(): void {
    this.#running = false;
    if (this.#watcher) {
      this.#watcher.close();
      this.#watcher = null;
    }
    for (const timer of this.#timers.values()) {
      clearTimeout(timer);
    }
    this.#timers.clear();
  }

  /** Internal: poll the watcher async iterator. */
  async #poll(): Promise<void> {
    if (!this.#watcher) return;
    try {
      for await (const event of this.#watcher) {
        if (!this.#running) break;
        if (event.kind === "access") continue; // Ignore access-only events

        for (const path of event.paths) {
          const extName = this.#resolveExtensionName(path);
          if (!extName) continue;
          this.#scheduleReload(extName);
        }
      }
    } catch {
      // Watcher closed or errored — expected on stop()
    }
  }

  /**
   * Resolve a changed file path to the extension name it belongs to.
   * Returns null if the path isn't inside a known extension directory.
   */
  #resolveExtensionName(path: string): string | null {
    for (const dir of this.#dirs) {
      if (path.startsWith(dir + "/")) {
        // Extract the extension directory name (first segment after the base)
        const relative = path.slice(dir.length + 1);
        const extDirName = relative.split("/")[0];
        if (extDirName) {
          // Check if this extension is actually loaded
          const info = this.#loader.registry.get(extDirName);
          if (info) return extDirName;
        }
      }
    }
    return null;
  }

  /** Schedule a debounced reload for the given extension. */
  #scheduleReload(name: string): void {
    const existing = this.#timers.get(name);
    if (existing !== undefined) {
      clearTimeout(existing);
    }

    const timer = setTimeout(() => {
      this.#timers.delete(name);
      console.info(`[marauder] Hot-reloading extension "${name}"...`);
      this.#loader.reload(name).catch((err) => {
        console.error(
          `[marauder] Hot-reload failed for "${name}":`,
          err,
        );
      });
    }, DEBOUNCE_MS);

    this.#timers.set(name, timer as unknown as number);
  }
}
