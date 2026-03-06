/**
 * @marauder/config — Typed configuration accessor
 *
 * Wraps the FFI ConfigStore with a typed interface, caching parsed sections
 * and supporting change subscriptions.
 */

import type { ConfigStore } from "@marauder/ffi-config-store";
import type {
  MarauderConfig,
  TerminalConfig,
  FontConfig,
  CursorConfig,
  WindowConfig,
  ThemeConfig,
} from "./schema.ts";
import { validateConfig } from "./schema.ts";
import { DEFAULT_CONFIG } from "./defaults.ts";

export type { MarauderConfig, TerminalConfig, FontConfig, CursorConfig, WindowConfig, ThemeConfig };
export { DEFAULT_CONFIG } from "./defaults.ts";
export { validateConfig } from "./schema.ts";

/** Config section names for change subscriptions. */
export type ConfigSection = "terminal" | "font" | "cursor" | "window" | "keybindings" | "theme";

type ChangeCallback<T> = (value: T) => void;

/**
 * Typed configuration accessor wrapping the raw ConfigStore.
 * Caches parsed sections and invalidates on config changes.
 */
export class TypedConfig {
  readonly #store: ConfigStore;
  #cache: Partial<MarauderConfig> = {};
  readonly #listeners = new Map<ConfigSection, Set<ChangeCallback<unknown>>>();
  /** Additional layers from TypeScript config files. */
  #tsOverrides: Partial<MarauderConfig> = {};

  constructor(store: ConfigStore) {
    this.#store = store;
  }

  /** Get terminal configuration. */
  get terminal(): TerminalConfig {
    if (!this.#cache.terminal) {
      this.#cache.terminal = this.#readSection("terminal", DEFAULT_CONFIG.terminal);
    }
    return this.#cache.terminal;
  }

  /** Get font configuration. */
  get font(): FontConfig {
    if (!this.#cache.font) {
      this.#cache.font = this.#readSection("font", DEFAULT_CONFIG.font);
    }
    return this.#cache.font;
  }

  /** Get cursor configuration. */
  get cursor(): CursorConfig {
    if (!this.#cache.cursor) {
      this.#cache.cursor = this.#readSection("cursor", DEFAULT_CONFIG.cursor);
    }
    return this.#cache.cursor;
  }

  /** Get window configuration. */
  get window(): WindowConfig {
    if (!this.#cache.window) {
      this.#cache.window = this.#readSection("window", DEFAULT_CONFIG.window);
    }
    return this.#cache.window;
  }

  /** Get keybindings. */
  get keybindings(): Record<string, string> {
    if (!this.#cache.keybindings) {
      const fromStore = this.#store.get<Record<string, string>>("keybindings");
      const fromTs = this.#tsOverrides.keybindings;
      this.#cache.keybindings = {
        ...DEFAULT_CONFIG.keybindings,
        ...(fromStore ?? {}),
        ...(fromTs ?? {}),
      };
    }
    return this.#cache.keybindings;
  }

  /** Get theme configuration (optional). */
  get theme(): ThemeConfig | undefined {
    if (!this.#cache.theme) {
      const fromStore = this.#store.get<ThemeConfig>("theme");
      const fromTs = this.#tsOverrides.theme;
      this.#cache.theme = fromTs ?? fromStore ?? undefined;
    }
    return this.#cache.theme;
  }

  /** Get the full resolved config. */
  getAll(): MarauderConfig {
    return {
      terminal: this.terminal,
      font: this.font,
      cursor: this.cursor,
      window: this.window,
      keybindings: this.keybindings,
      theme: this.theme,
    };
  }

  /**
   * Subscribe to changes for a specific config section.
   * Returns an unsubscribe function for easy cleanup.
   */
  onChange<T>(section: ConfigSection, callback: ChangeCallback<T>): () => void {
    let set = this.#listeners.get(section);
    if (!set) {
      set = new Set();
      this.#listeners.set(section, set);
    }
    const wrapped = callback as ChangeCallback<unknown>;
    set.add(wrapped);

    return () => {
      const s = this.#listeners.get(section);
      if (s) s.delete(wrapped);
    };
  }

  /** Remove a change listener by reference. */
  offChange<T>(section: ConfigSection, callback: ChangeCallback<T>): void {
    const set = this.#listeners.get(section);
    if (set) {
      set.delete(callback as ChangeCallback<unknown>);
    }
  }

  /** Apply TypeScript config overrides (from config.ts files). */
  applyTsOverrides(overrides: Partial<MarauderConfig>): void {
    this.#tsOverrides = overrides;
    this.invalidateAll();
  }

  /** Invalidate all cached sections and notify listeners. */
  invalidateAll(): void {
    const oldConfig = this.#cache;
    this.#cache = {};

    // Notify listeners for sections that changed
    for (const section of this.#listeners.keys()) {
      const oldVal = oldConfig[section as keyof MarauderConfig];
      const newVal = this[section as keyof Pick<TypedConfig, ConfigSection>];
      if (JSON.stringify(oldVal) !== JSON.stringify(newVal)) {
        this.#notifyListeners(section, newVal);
      }
    }
  }

  /** Invalidate a specific section. */
  invalidate(section: ConfigSection): void {
    delete this.#cache[section as keyof MarauderConfig];
    const newVal = this[section as keyof Pick<TypedConfig, ConfigSection>];
    this.#notifyListeners(section, newVal);
  }

  /** Access the underlying raw ConfigStore. */
  get store(): ConfigStore {
    return this.#store;
  }

  #readSection<T extends Record<string, unknown>>(
    section: string,
    defaults: T,
  ): T {
    const result = { ...defaults };
    const tsOverride = this.#tsOverrides[section as keyof MarauderConfig];

    // Single FFI call to fetch the entire section object
    const sectionVal = this.#store.get<Record<string, unknown>>(section);
    if (sectionVal && typeof sectionVal === "object") {
      for (const key of Object.keys(defaults)) {
        if (key in sectionVal && sectionVal[key] !== undefined) {
          (result as Record<string, unknown>)[key] = sectionVal[key];
        }
      }
    }

    // Apply TS overrides on top
    if (tsOverride && typeof tsOverride === "object") {
      for (const [key, val] of Object.entries(tsOverride)) {
        if (key in defaults) {
          (result as Record<string, unknown>)[key] = val;
        }
      }
    }

    return result;
  }

  #notifyListeners(section: ConfigSection, value: unknown): void {
    const set = this.#listeners.get(section);
    if (set) {
      for (const cb of set) {
        try {
          cb(value);
        } catch {
          // Don't let listener errors propagate
        }
      }
    }
  }
}
