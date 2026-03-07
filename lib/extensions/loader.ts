// lib/extensions/loader.ts
// Extension discovery, validation, loading, and unloading.

import type { ExtensionManifest, ExtensionModule } from "./types.ts";
import { EXTENSION_API_VERSION } from "./types.ts";
import { ExtensionRegistry } from "./registry.ts";
import { CommandRegistry } from "./commands.ts";
import { KeybindingRegistry } from "./keybindings.ts";
import { PanelRegistry } from "./panels.ts";
import { createExtensionContext, type RuntimeServices } from "./context.ts";
import { safeActivate, safeDeactivate, clearErrors } from "./isolation.ts";

/** Paths to scan for extensions, in priority order. */
const DEFAULT_DISCOVERY_PATHS = [
  "extensions", // bundled
];

/** User-level extension directory (resolved at runtime). */
function userExtensionDir(): string | null {
  const home =
    (typeof Deno !== "undefined" && Deno.env?.get("HOME")) || null;
  if (!home) return null;
  return `${home}/.config/marauder/extensions`;
}

/** Validate a parsed JSON object as an ExtensionManifest. */
export function validateManifest(
  json: Record<string, unknown>,
): ExtensionManifest | null {
  const { name, version, description, entry } = json;
  if (
    typeof name !== "string" || name.length === 0 ||
    typeof version !== "string" || version.length === 0 ||
    typeof description !== "string" ||
    typeof entry !== "string" || entry.length === 0
  ) {
    return null;
  }

  // Reject names with path separators or traversal patterns
  if (/[/\\]/.test(name) || name.includes("..") || name.startsWith(".")) {
    return null;
  }

  // Reject entry points that escape the extension directory
  if (entry.includes("..") || entry.startsWith("/") || entry.startsWith("\\")) {
    return null;
  }
  return {
    name,
    version,
    description,
    entry,
    permissions: Array.isArray(json.permissions)
      ? (json.permissions as string[])
      : undefined,
    dependencies:
      json.dependencies && typeof json.dependencies === "object"
        ? (json.dependencies as Record<string, string>)
        : undefined,
    engines:
      json.engines && typeof json.engines === "object"
        ? (json.engines as Record<string, string>)
        : undefined,
    repository:
      typeof json.repository === "string" ? json.repository : undefined,
    activationEvents: Array.isArray(json.activationEvents)
      ? (json.activationEvents as string[])
      : undefined,
  };
}

/** Discover extension directories and return validated manifests + paths. */
export async function discover(
  dirs?: string[],
): Promise<Array<{ manifest: ExtensionManifest; dir: string }>> {
  const searchDirs = dirs ?? [...DEFAULT_DISCOVERY_PATHS];

  const userDir = userExtensionDir();
  if (userDir && !searchDirs.includes(userDir)) {
    searchDirs.push(userDir);
  }

  const results: Array<{ manifest: ExtensionManifest; dir: string }> = [];

  for (const base of searchDirs) {
    let entries: Iterable<Deno.DirEntry>;
    try {
      entries = Deno.readDirSync(base);
    } catch {
      // Directory doesn't exist — skip silently.
      continue;
    }

    for (const entry of entries) {
      if (!entry.isDirectory) continue;
      const extDir = `${base}/${entry.name}`;
      const manifestPath = `${extDir}/extension.json`;

      try {
        const raw = Deno.readTextFileSync(manifestPath);
        const json = JSON.parse(raw) as Record<string, unknown>;
        const manifest = validateManifest(json);
        if (manifest) {
          results.push({ manifest, dir: extDir });
        } else {
          console.warn(
            `[marauder] Invalid manifest at ${manifestPath} — skipping`,
          );
        }
      } catch {
        // No manifest or invalid JSON — skip.
      }
    }
  }

  return results;
}

/**
 * The main ExtensionLoader: discovers, loads, activates, and unloads extensions.
 */
export class ExtensionLoader {
  readonly registry: ExtensionRegistry;
  readonly commandRegistry: CommandRegistry;
  readonly keybindingRegistry: KeybindingRegistry;
  readonly panelRegistry: PanelRegistry;
  readonly #services: RuntimeServices;

  constructor(
    registry: ExtensionRegistry,
    commandRegistry: CommandRegistry,
    keybindingRegistry: KeybindingRegistry,
    panelRegistry: PanelRegistry,
    services: RuntimeServices,
  ) {
    this.registry = registry;
    this.commandRegistry = commandRegistry;
    this.keybindingRegistry = keybindingRegistry;
    this.panelRegistry = panelRegistry;
    this.#services = services;
  }

  /** Load and activate a single extension from its manifest and directory. */
  async load(manifest: ExtensionManifest, dir: string): Promise<void> {
    if (this.registry.has(manifest.name)) {
      console.warn(
        `[marauder] Extension "${manifest.name}" is already loaded — skipping`,
      );
      return;
    }

    // API version compatibility check
    if (manifest.engines?.["marauder"]) {
      const required = manifest.engines["marauder"];
      if (!isCompatibleVersion(required, EXTENSION_API_VERSION)) {
        const msg = `Requires marauder engine "${required}" but current API is "${EXTENSION_API_VERSION}"`;
        console.error(`[marauder] Extension "${manifest.name}": ${msg}`);
        this.registry.register(manifest, { activate() {}, deactivate() {} } as ExtensionModule, dir);
        this.registry.setState(manifest.name, "error", msg);
        return;
      }
    }

    // Dynamic import of the extension entry point
    const entryPath = `${dir}/${manifest.entry}`;
    let mod: ExtensionModule;
    try {
      mod = (await import(entryPath)) as ExtensionModule;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(
        `[marauder] Failed to import extension "${manifest.name}": ${msg}`,
      );
      return;
    }

    if (typeof mod.activate !== "function") {
      const msg = `Extension "${manifest.name}" has no activate() export`;
      console.error(`[marauder] ${msg}`);
      this.registry.register(manifest, mod, dir);
      this.registry.setState(manifest.name, "error", msg);
      return;
    }

    // Register in the registry
    this.registry.register(manifest, mod, dir);

    // Create the context wired to runtime services
    const ctx = createExtensionContext(
      manifest,
      this.#services,
      this.commandRegistry,
      this.keybindingRegistry,
      this.panelRegistry,
    );

    // Activate with error isolation + timeout
    const error = await safeActivate(mod, ctx, manifest.name);
    if (error) {
      this.registry.setState(manifest.name, "error", error);
      console.error(
        `[marauder] Extension "${manifest.name}" activation failed: ${error}`,
      );
    } else {
      this.registry.setState(manifest.name, "active");
    }

    // Emit ExtensionLoaded event
    this.#services.eventEmit("ExtensionLoaded", {
      name: manifest.name,
      version: manifest.version,
    });
  }

  /** Unload an extension by name: deactivate, clean up registrations, remove. */
  async unload(name: string): Promise<void> {
    const mod = this.registry.getModule(name);
    if (!mod) {
      console.warn(`[marauder] Extension "${name}" is not loaded`);
      return;
    }

    // Deactivate
    if (typeof mod.deactivate === "function") {
      const error = await safeDeactivate(mod, name);
      if (error) {
        console.error(
          `[marauder] Extension "${name}" deactivation error: ${error}`,
        );
      }
    }

    // Clean up commands, keybindings, panels, and registry cleanups
    this.commandRegistry.unregisterAll(name);
    this.keybindingRegistry.unregisterAll(name);
    this.panelRegistry.unregisterAll(name);
    this.registry.runCleanups(name);

    // Emit ExtensionUnloaded event
    this.#services.eventEmit("ExtensionUnloaded", { name });

    // Remove from registry
    this.registry.unregister(name);
    clearErrors(name);
  }

  /** Extensions deferred for lazy activation, keyed by activation event. */
  readonly #deferred = new Map<string, Array<{ manifest: ExtensionManifest; dir: string }>>();

  /**
   * Discover and load all extensions from default + custom directories.
   * Extensions with activationEvents are deferred until the event fires.
   * Returns the number of extensions eagerly loaded.
   */
  async loadAll(dirs?: string[]): Promise<number> {
    const discovered = await discover(dirs);
    const allNames = new Set(discovered.map(({ manifest }) => manifest.name));
    let count = 0;

    for (const { manifest, dir } of discovered) {
      // Check declared dependencies are present
      if (manifest.dependencies) {
        const missing = Object.keys(manifest.dependencies).filter((dep) => !allNames.has(dep));
        if (missing.length > 0) {
          console.warn(
            `[marauder] Extension "${manifest.name}" has missing dependencies: ${missing.join(", ")} — loading anyway`,
          );
        }
      }

      // Defer extensions that declare activation events
      if (manifest.activationEvents && manifest.activationEvents.length > 0) {
        for (const event of manifest.activationEvents) {
          let list = this.#deferred.get(event);
          if (!list) {
            list = [];
            this.#deferred.set(event, list);
            // Subscribe to the event so deferred extensions auto-load
            this.#services.eventOn(event, () => this.#activateDeferred(event));
          }
          list.push({ manifest, dir });
        }
        continue;
      }

      await this.load(manifest, dir);
      count++;
    }
    return count;
  }

  /** Load deferred extensions when their activation event fires. */
  async #activateDeferred(event: string): Promise<void> {
    const pending = this.#deferred.get(event);
    if (!pending) return;
    this.#deferred.delete(event);
    for (const { manifest, dir } of pending) {
      if (!this.registry.has(manifest.name)) {
        await this.load(manifest, dir);
      }
    }
  }

  /** Reload a specific extension (unload then load). */
  async reload(name: string): Promise<void> {
    const info = this.registry.get(name);
    if (!info) {
      console.warn(`[marauder] Cannot reload "${name}" — not loaded`);
      return;
    }

    const { manifest, dir } = info;
    await this.unload(name);
    await this.load(manifest, dir);
  }
}

/**
 * Check semver compatibility. Supports `^major.minor.patch` (caret range)
 * and exact match. Returns true if `actual` satisfies `required`.
 */
export function isCompatibleVersion(required: string, actual: string): boolean {
  const clean = required.replace(/^[~^]/, "");
  const prefix = required.startsWith("^") ? "^" : required.startsWith("~") ? "~" : "";

  const parse = (v: string): { major: number; minor: number; patch: number } | null => {
    const parts = v.split(".");
    if (parts.length < 1 || parts.length > 3) return null;
    const nums = parts.map(Number);
    if (nums.some((n) => !Number.isInteger(n) || n < 0)) return null;
    return { major: nums[0] ?? 0, minor: nums[1] ?? 0, patch: nums[2] ?? 0 };
  };

  const req = parse(clean);
  const act = parse(actual);

  if (!req || !act) return false;

  if (prefix === "^") {
    // ^1.2.3 means >=1.2.3 <2.0.0 (for major > 0)
    if (req.major > 0) {
      return act.major === req.major &&
        (act.minor > req.minor || (act.minor === req.minor && act.patch >= req.patch));
    }
    // ^0.x — minor must match
    return act.major === 0 && act.minor === req.minor && act.patch >= req.patch;
  }

  if (prefix === "~") {
    // ~1.2.3 means >=1.2.3 <1.3.0
    return act.major === req.major && act.minor === req.minor && act.patch >= req.patch;
  }

  // Exact match
  return act.major === req.major && act.minor === req.minor && act.patch === req.patch;
}
