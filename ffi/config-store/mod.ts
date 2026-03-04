/**
 * Deno FFI bindings for marauder-config-store.
 *
 * Wraps the C ABI exported by `libmarauder_config_store` in an ergonomic TypeScript class.
 */

import { bufferPtr, resolveLibPath, toCString } from "../_lib.ts";

const lib = Deno.dlopen(
  resolveLibPath("marauder_config_store"),
  {
    config_store_create: {
      parameters: [],
      result: "pointer",
    },
    config_store_load: {
      parameters: ["pointer", "pointer", "pointer", "pointer"],
      result: "i32",
    },
    config_store_get: {
      parameters: ["pointer", "pointer", "pointer", "usize"],
      result: "i32",
    },
    config_store_set: {
      parameters: ["pointer", "pointer", "pointer"],
      result: "i32",
    },
    config_store_save: {
      parameters: ["pointer", "pointer"],
      result: "i32",
    },
    config_store_watch: {
      parameters: ["pointer"],
      result: "i32",
    },
    config_store_unwatch: {
      parameters: ["pointer"],
      result: "i32",
    },
    config_store_destroy: {
      parameters: ["pointer"],
      result: "void",
    },
  } as const,
);

const decoder = new TextDecoder();

/** Maximum buffer size for config value reads (1 MB). */
const MAX_CONFIG_VALUE_SIZE = 1024 * 1024;

/** Config file paths for loading layered configuration. */
export interface ConfigPaths {
  /** System-level config (e.g. /etc/marauder/config.toml). */
  system?: string;
  /** User-level config (e.g. ~/.config/marauder/config.toml). */
  user?: string;
  /** Project-level config (e.g. .marauder/config.toml). */
  project?: string;
}

/**
 * TypeScript wrapper around the marauder config store.
 *
 * Usage:
 * ```ts
 * using config = new ConfigStore();
 * config.load({ user: "~/.config/marauder/config.toml" });
 * const fontSize = config.get<number>("font.size");
 * config.set("font.size", 16);
 * config.save("~/.config/marauder/config.toml");
 * ```
 */
export class ConfigStore {
  #handle: Deno.PointerValue;
  #closed = false;

  constructor() {
    this.#handle = lib.symbols.config_store_create();
    if (this.#handle === null) {
      throw new Error("Failed to create ConfigStore native handle");
    }
  }

  /**
   * Load configuration from file paths. Each layer is optional.
   * Later layers override earlier ones (system < user < project).
   */
  load(paths: ConfigPaths): void {
    this.#ensureOpen();

    const systemBuf = paths.system ? toCString(paths.system) : null;
    const userBuf = paths.user ? toCString(paths.user) : null;
    const projectBuf = paths.project ? toCString(paths.project) : null;

    const systemPtr = systemBuf ? bufferPtr(systemBuf) : null;
    const userPtr = userBuf ? bufferPtr(userBuf) : null;
    const projectPtr = projectBuf ? bufferPtr(projectBuf) : null;

    const result = lib.symbols.config_store_load(
      this.#handle,
      systemPtr,
      userPtr,
      projectPtr,
    );

    if (result !== 0) {
      throw new Error("Failed to load config");
    }
  }

  /**
   * Get a config value by dot-notation key (e.g. "font.size").
   * Returns the parsed JSON value, or undefined if not found.
   *
   * Automatically retries with a larger buffer if the value was truncated.
   */
  get<T = unknown>(key: string): T | undefined {
    this.#ensureOpen();

    const keyBuf = toCString(key);
    const keyPtr = bufferPtr(keyBuf);
    let bufSize = 4096;

    // Retry loop: if the value is larger than our buffer, grow and retry
    while (bufSize <= MAX_CONFIG_VALUE_SIZE) {
      const outBuf = new Uint8Array(bufSize);

      const totalLen = lib.symbols.config_store_get(
        this.#handle,
        keyPtr,
        bufferPtr(outBuf),
        BigInt(bufSize),
      );

      if (totalLen < 0) return undefined;

      if (totalLen <= bufSize) {
        const json = decoder.decode(outBuf.subarray(0, totalLen));
        return JSON.parse(json) as T;
      }

      // Value was truncated — retry with the exact needed size
      bufSize = totalLen;
    }

    throw new Error(
      `Config value for "${key}" exceeds maximum size (${MAX_CONFIG_VALUE_SIZE} bytes)`,
    );
  }

  /**
   * Set a config value in the CLI override layer.
   * The value will be JSON-serialized.
   */
  set(key: string, value: unknown): void {
    this.#ensureOpen();

    const keyBuf = toCString(key);
    const valueBuf = toCString(JSON.stringify(value));

    const result = lib.symbols.config_store_set(
      this.#handle,
      bufferPtr(keyBuf),
      bufferPtr(valueBuf),
    );

    if (result !== 0) {
      throw new Error(`Failed to set config key "${key}"`);
    }
  }

  /**
   * Save the user config layer to a TOML file.
   */
  save(path: string): void {
    this.#ensureOpen();

    const pathBuf = toCString(path);

    const result = lib.symbols.config_store_save(
      this.#handle,
      bufferPtr(pathBuf),
    );

    if (result !== 0) {
      throw new Error(`Failed to save config to "${path}"`);
    }
  }

  /**
   * Start watching config file paths for changes.
   * On change, the store reloads automatically.
   */
  watch(): void {
    this.#ensureOpen();

    const result = lib.symbols.config_store_watch(this.#handle);
    if (result !== 0) {
      throw new Error("Failed to start config file watching");
    }
  }

  /**
   * Stop watching config files.
   */
  unwatch(): void {
    this.#ensureOpen();
    lib.symbols.config_store_unwatch(this.#handle);
  }

  /**
   * Destroy the config store handle.
   */
  destroy(): void {
    if (this.#closed) return;
    this.#closed = true;
    lib.symbols.config_store_destroy(this.#handle);
  }

  [Symbol.dispose](): void {
    this.destroy();
  }

  #ensureOpen(): void {
    if (this.#closed) {
      throw new Error("ConfigStore has been destroyed");
    }
  }
}
