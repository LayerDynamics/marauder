// lib/extensions/loader.ts
// Extension discovery, validation, loading, and unloading.

import type { ExtensionManifest, ExtensionModule } from "./types.ts";
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
    typeof name !== "string" ||
    typeof version !== "string" ||
    typeof description !== "string" ||
    typeof entry !== "string"
  ) {
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
      console.error(
        `[marauder] Extension "${manifest.name}" has no activate() export`,
      );
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

  /**
   * Discover and load all extensions from default + custom directories.
   * Returns the number of extensions loaded.
   */
  async loadAll(dirs?: string[]): Promise<number> {
    const discovered = await discover(dirs);
    let count = 0;
    for (const { manifest, dir } of discovered) {
      await this.load(manifest, dir);
      count++;
    }
    return count;
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
