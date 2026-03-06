/**
 * TypedConfig — high-level config access wrapping the FFI ConfigStore.
 *
 * Provides typed section getters with caching, change callbacks,
 * and automatic invalidation on ConfigChanged events.
 */

export type {
  MarauderConfig,
  TerminalConfig,
  FontConfig,
  CursorConfig,
  WindowConfig,
  ThemeConfig,
} from "./schema.ts";
export { validateConfig } from "./schema.ts";
export { DEFAULT_CONFIG } from "./defaults.ts";

import type {
  MarauderConfig,
  TerminalConfig,
  FontConfig,
  CursorConfig,
  WindowConfig,
  ThemeConfig,
} from "./schema.ts";
import { DEFAULT_CONFIG } from "./defaults.ts";
import { ConfigStore, type ConfigPaths } from "../../ffi/config-store/mod.ts";

/** Callback invoked when a config section changes. */
export type ConfigChangeCallback<T> = (newValue: T) => void;

/** Section name → type mapping. */
type SectionMap = {
  terminal: TerminalConfig;
  font: FontConfig;
  cursor: CursorConfig;
  window: WindowConfig;
  theme: ThemeConfig;
};

type SectionName = keyof SectionMap;

/**
 * Typed configuration manager wrapping the native ConfigStore via FFI.
 *
 * Usage:
 * ```ts
 * using config = new TypedConfig();
 * config.load({ user: "~/.config/marauder/config.toml" });
 * const font = config.font();
 * config.onChange("font", (f) => console.log("font changed:", f));
 * ```
 */
export class TypedConfig {
  #store: ConfigStore;
  #cache: Partial<Record<SectionName, unknown>> = {};
  #listeners: Partial<Record<SectionName, ConfigChangeCallback<unknown>[]>> = {};

  constructor() {
    this.#store = new ConfigStore();
  }

  /** Load config from file paths (delegates to native ConfigStore). */
  load(paths: ConfigPaths): void {
    this.#store.load(paths);
    this.invalidateAll();
  }

  /** Get the terminal configuration section. */
  terminal(): TerminalConfig {
    return this.#getSection("terminal");
  }

  /** Get the font configuration section. */
  font(): FontConfig {
    return this.#getSection("font");
  }

  /** Get the cursor configuration section. */
  cursor(): CursorConfig {
    return this.#getSection("cursor");
  }

  /** Get the window configuration section. */
  window(): WindowConfig {
    return this.#getSection("window");
  }

  /** Get the theme configuration section. */
  theme(): ThemeConfig {
    return this.#getSection("theme");
  }

  /** Get the full resolved configuration. */
  all(): MarauderConfig {
    return {
      terminal: this.terminal(),
      font: this.font(),
      cursor: this.cursor(),
      window: this.window(),
      theme: this.theme(),
    };
  }

  /** Register a callback for when a section changes. Returns an unsubscribe function. */
  onChange<S extends SectionName>(
    section: S,
    callback: ConfigChangeCallback<SectionMap[S]>,
  ): () => void {
    const list = this.#listeners[section] ??= [];
    const wrapped = callback as ConfigChangeCallback<unknown>;
    list.push(wrapped);
    return () => {
      const arr = this.#listeners[section];
      if (arr) {
        const idx = arr.indexOf(wrapped);
        if (idx !== -1) arr.splice(idx, 1);
      }
    };
  }

  /** Invalidate cached values for a specific section and notify listeners. */
  invalidate(section: SectionName): void {
    delete this.#cache[section];
    const newValue = this.#getSection(section);
    const listeners = this.#listeners[section];
    if (listeners) {
      for (const cb of listeners) {
        cb(newValue);
      }
    }
  }

  /** Invalidate all cached sections. Called on reload. */
  invalidateAll(): void {
    const sections: SectionName[] = ["terminal", "font", "cursor", "window", "theme"];
    for (const s of sections) {
      this.invalidate(s);
    }
  }

  /** Set a config value and invalidate the relevant section cache. */
  set(key: string, value: unknown): void {
    this.#store.set(key, value);
    // Invalidate the section this key belongs to
    const sectionKey = key.split(".")[0];
    if (sectionKey && this.#isValidSection(sectionKey)) {
      this.invalidate(sectionKey);
    }
  }

  /** Save config to a file path. */
  save(path: string): void {
    this.#store.save(path);
  }

  /** Start watching config files for changes. */
  watch(): void {
    this.#store.watch();
  }

  /** Stop watching config files. */
  unwatch(): void {
    this.#store.unwatch();
  }

  /** Destroy the underlying native handle. */
  destroy(): void {
    this.#store.destroy();
  }

  [Symbol.dispose](): void {
    this.destroy();
  }

  static readonly #VALID_SECTIONS: ReadonlySet<string> = new Set<SectionName>([
    "terminal", "font", "cursor", "window", "theme",
  ]);

  #isValidSection(key: string): key is SectionName {
    return TypedConfig.#VALID_SECTIONS.has(key);
  }

  #getSection<S extends SectionName>(section: S): SectionMap[S] {
    if (section in this.#cache) {
      return this.#cache[section] as SectionMap[S];
    }

    // Try to read from native store, fall back to defaults
    const value = this.#store.get<SectionMap[S]>(section) ?? DEFAULT_CONFIG[section];
    this.#cache[section] = value;
    return value as SectionMap[S];
  }
}
