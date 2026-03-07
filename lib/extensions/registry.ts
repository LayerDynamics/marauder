// lib/extensions/registry.ts
// Central extension registry: tracks loaded extensions by name.

import type {
  ExtensionInfo,
  ExtensionManifest,
  ExtensionModule,
  ExtensionState,
} from "./types.ts";

/** Internal record for a registered extension. */
interface RegistryEntry {
  manifest: ExtensionManifest;
  module: ExtensionModule;
  state: ExtensionState;
  error?: string;
  dir: string;
  /** Cleanup functions registered during activation (event unsubs, etc.). */
  cleanups: Array<() => void>;
}

/** Tracks all loaded extensions and their state. */
export class ExtensionRegistry {
  readonly #entries: Map<string, RegistryEntry> = new Map();

  /** Register a loaded extension. */
  register(
    manifest: ExtensionManifest,
    mod: ExtensionModule,
    dir: string,
  ): void {
    this.#entries.set(manifest.name, {
      manifest,
      module: mod,
      state: "loaded",
      dir,
      cleanups: [],
    });
  }

  /** Remove an extension from the registry. */
  unregister(name: string): boolean {
    return this.#entries.delete(name);
  }

  /** Get registry entry (internal). */
  getEntry(name: string): RegistryEntry | undefined {
    return this.#entries.get(name);
  }

  /** Get public info for an extension. */
  get(name: string): ExtensionInfo | undefined {
    const entry = this.#entries.get(name);
    if (!entry) return undefined;
    return {
      manifest: entry.manifest,
      state: entry.state,
      error: entry.error,
      dir: entry.dir,
    };
  }

  /** Get the module for an extension. */
  getModule(name: string): ExtensionModule | undefined {
    return this.#entries.get(name)?.module;
  }

  /** Update the state of a registered extension. */
  setState(name: string, state: ExtensionState, error?: string): void {
    const entry = this.#entries.get(name);
    if (entry) {
      entry.state = state;
      entry.error = error;
    }
  }

  /** Add a cleanup function for an extension (called on unload). */
  addCleanup(name: string, cleanup: () => void): void {
    const entry = this.#entries.get(name);
    if (entry) {
      entry.cleanups.push(cleanup);
    }
  }

  /** Run and clear all cleanup functions for an extension. */
  runCleanups(name: string): void {
    const entry = this.#entries.get(name);
    if (!entry) return;
    for (const fn of entry.cleanups) {
      try {
        fn();
      } catch (err) {
        console.error(
          `[marauder] Cleanup error for extension "${name}":`,
          err,
        );
      }
    }
    entry.cleanups.length = 0;
  }

  /** Check if an extension is registered. */
  has(name: string): boolean {
    return this.#entries.has(name);
  }

  /** List info for all registered extensions. */
  list(): ExtensionInfo[] {
    return [...this.#entries.values()].map((e) => ({
      manifest: e.manifest,
      state: e.state,
      error: e.error,
      dir: e.dir,
    }));
  }

  /** Get names of all registered extensions. */
  names(): string[] {
    return [...this.#entries.keys()];
  }
}
