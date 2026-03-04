/**
 * Deno FFI bindings for marauder-config-store.
 *
 * Wraps the C ABI exported by `libmarauder_config_store` in an ergonomic TypeScript class.
 */

/** Resolve the path to the compiled config-store shared library. */
function resolveLibPath(): string {
  const libName = (() => {
    switch (Deno.build.os) {
      case "darwin":
        return "libmarauder_config_store.dylib";
      case "linux":
        return "libmarauder_config_store.so";
      case "windows":
        return "marauder_config_store.dll";
      default:
        throw new Error(`Unsupported platform: ${Deno.build.os}`);
    }
  })();

  const envDir = Deno.env.get("MARAUDER_LIB_DIR");
  if (envDir) {
    return `${envDir}/${libName}`;
  }

  const candidates = [
    `target/release/${libName}`,
    `target/debug/${libName}`,
  ];

  for (const candidate of candidates) {
    try {
      Deno.statSync(candidate);
      return candidate;
    } catch {
      // continue
    }
  }

  return `target/debug/${libName}`;
}

const lib = Deno.dlopen(
  resolveLibPath(),
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

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Encode a string as a null-terminated C string in a Uint8Array. */
function toCString(s: string): Uint8Array {
  const bytes = encoder.encode(s);
  const buf = new Uint8Array(bytes.length + 1);
  buf.set(bytes);
  buf[bytes.length] = 0;
  return buf;
}

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

    const systemPtr = systemBuf
      ? Deno.UnsafePointer.of(systemBuf as unknown as ArrayBuffer)
      : null;
    const userPtr = userBuf
      ? Deno.UnsafePointer.of(userBuf as unknown as ArrayBuffer)
      : null;
    const projectPtr = projectBuf
      ? Deno.UnsafePointer.of(projectBuf as unknown as ArrayBuffer)
      : null;

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
   */
  get<T = unknown>(key: string): T | undefined {
    this.#ensureOpen();

    const keyBuf = toCString(key);
    const keyPtr = Deno.UnsafePointer.of(keyBuf as unknown as ArrayBuffer);
    const bufSize = 4096;
    const outBuf = new Uint8Array(bufSize);
    const outPtr = Deno.UnsafePointer.of(outBuf as unknown as ArrayBuffer);

    const totalLen = lib.symbols.config_store_get(
      this.#handle,
      keyPtr,
      outPtr,
      BigInt(bufSize),
    );

    if (totalLen < 0) return undefined;

    // If truncated, we'd need a larger buffer — for now return what we have
    const readLen = Math.min(totalLen, bufSize);
    const json = decoder.decode(outBuf.subarray(0, readLen));
    return JSON.parse(json) as T;
  }

  /**
   * Set a config value in the CLI override layer.
   * The value will be JSON-serialized.
   */
  set(key: string, value: unknown): void {
    this.#ensureOpen();

    const keyBuf = toCString(key);
    const valueBuf = toCString(JSON.stringify(value));
    const keyPtr = Deno.UnsafePointer.of(keyBuf as unknown as ArrayBuffer);
    const valuePtr = Deno.UnsafePointer.of(valueBuf as unknown as ArrayBuffer);

    const result = lib.symbols.config_store_set(
      this.#handle,
      keyPtr,
      valuePtr,
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
    const pathPtr = Deno.UnsafePointer.of(pathBuf as unknown as ArrayBuffer);

    const result = lib.symbols.config_store_save(this.#handle, pathPtr);

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
