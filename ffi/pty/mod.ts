/**
 * Deno FFI bindings for marauder-pty.
 *
 * Wraps the C ABI exported by `libmarauder_pty` in an ergonomic TypeScript class.
 */

import { bufferPtr, resolveLibPath, toCString } from "../_lib.ts";

const lib = Deno.dlopen(
  resolveLibPath("marauder_pty"),
  {
    pty_manager_create: {
      parameters: [],
      result: "pointer",
    },
    pty_create: {
      parameters: ["pointer", "pointer", "pointer", "pointer", "u16", "u16"],
      result: "u64",
    },
    pty_read: {
      parameters: ["pointer", "u64", "pointer", "usize"],
      result: "i64",
    },
    pty_write: {
      parameters: ["pointer", "u64", "pointer", "usize"],
      result: "i64",
    },
    pty_resize: {
      parameters: ["pointer", "u64", "u16", "u16"],
      result: "i32",
    },
    pty_close: {
      parameters: ["pointer", "u64"],
      result: "i32",
    },
    pty_get_pid: {
      parameters: ["pointer", "u64"],
      result: "u32",
    },
    pty_wait: {
      parameters: ["pointer", "u64"],
      result: "i32",
    },
    pty_count: {
      parameters: ["pointer"],
      result: "u64",
    },
    pty_manager_destroy: {
      parameters: ["pointer"],
      result: "void",
    },
  } as const,
);

const encoder = new TextEncoder();

/** Configuration for creating a PTY session. */
export interface PtyConfig {
  /** Shell executable path. Empty or undefined for platform default. */
  shell?: string;
  /** Working directory. Empty or undefined for current directory. */
  cwd?: string;
  /** Environment variables to set. */
  env?: Record<string, string>;
  /** Terminal rows. */
  rows: number;
  /** Terminal columns. */
  cols: number;
}

/**
 * TypeScript wrapper around the marauder PTY manager.
 *
 * Usage:
 * ```ts
 * using pty = new PtyManager();
 * const paneId = pty.create({ rows: 24, cols: 80 });
 * pty.write(paneId, "ls\n");
 * const output = pty.read(paneId, 4096);
 * pty.close(paneId);
 * ```
 */
export class PtyManager {
  #handle: Deno.PointerValue;
  #closed = false;

  constructor() {
    this.#handle = lib.symbols.pty_manager_create();
    if (this.#handle === null) {
      throw new Error("Failed to create PtyManager native handle");
    }
  }

  /**
   * Create a new PTY session. Returns the pane ID (>0).
   * Throws on error.
   */
  create(config: PtyConfig): number | bigint {
    this.#ensureOpen();

    const shellBuf = config.shell ? toCString(config.shell) : null;
    const cwdBuf = config.cwd ? toCString(config.cwd) : null;
    const envBuf = config.env ? toCString(JSON.stringify(config.env)) : null;

    const shellPtr = shellBuf ? bufferPtr(shellBuf) : null;
    const cwdPtr = cwdBuf ? bufferPtr(cwdBuf) : null;
    const envPtr = envBuf ? bufferPtr(envBuf) : null;

    const paneId = lib.symbols.pty_create(
      this.#handle,
      shellPtr,
      cwdPtr,
      envPtr,
      config.rows,
      config.cols,
    );

    if (paneId === 0n) {
      throw new Error("Failed to create PTY session");
    }

    return paneId;
  }

  /**
   * Read data from a PTY session. Returns the bytes read.
   * WARNING: This blocks until data is available.
   */
  read(paneId: number | bigint, maxBytes: number): Uint8Array {
    this.#ensureOpen();

    const buf = new Uint8Array(maxBytes);
    const normalizedId = typeof paneId === "number" ? BigInt(paneId) : paneId;

    const bytesRead = lib.symbols.pty_read(
      this.#handle,
      normalizedId,
      bufferPtr(buf),
      BigInt(maxBytes),
    );

    if (bytesRead < 0n) {
      throw new Error(`Failed to read from PTY pane ${paneId}`);
    }

    return buf.subarray(0, Number(bytesRead));
  }

  /**
   * Write data to a PTY session. Returns bytes written.
   */
  write(paneId: number | bigint, data: string | Uint8Array): number {
    this.#ensureOpen();

    const bytes = typeof data === "string" ? encoder.encode(data) : data;
    const normalizedId = typeof paneId === "number" ? BigInt(paneId) : paneId;

    const written = lib.symbols.pty_write(
      this.#handle,
      normalizedId,
      bufferPtr(bytes),
      BigInt(bytes.byteLength),
    );

    if (written < 0n) {
      throw new Error(`Failed to write to PTY pane ${paneId}`);
    }

    return Number(written);
  }

  /**
   * Resize a PTY session.
   */
  resize(paneId: number | bigint, rows: number, cols: number): void {
    this.#ensureOpen();
    const normalizedId = typeof paneId === "number" ? BigInt(paneId) : paneId;

    const result = lib.symbols.pty_resize(
      this.#handle,
      normalizedId,
      rows,
      cols,
    );

    if (result === 0) {
      throw new Error(`Failed to resize PTY pane ${paneId}`);
    }
  }

  /**
   * Close a PTY session.
   */
  close(paneId: number | bigint): void {
    this.#ensureOpen();
    const normalizedId = typeof paneId === "number" ? BigInt(paneId) : paneId;

    const result = lib.symbols.pty_close(this.#handle, normalizedId);

    if (result === 0) {
      throw new Error(`Failed to close PTY pane ${paneId}`);
    }
  }

  /**
   * Get the child process PID for a PTY session. Returns 0 if not available.
   */
  getPid(paneId: number | bigint): number {
    this.#ensureOpen();
    const normalizedId = typeof paneId === "number" ? BigInt(paneId) : paneId;
    return lib.symbols.pty_get_pid(this.#handle, normalizedId);
  }

  /**
   * Check if a child process has exited.
   * Returns true if exited, false if still running.
   */
  hasExited(paneId: number | bigint): boolean {
    this.#ensureOpen();
    const normalizedId = typeof paneId === "number" ? BigInt(paneId) : paneId;
    const result = lib.symbols.pty_wait(this.#handle, normalizedId);
    if (result === -1) {
      throw new Error(`Failed to check exit status for PTY pane ${paneId}`);
    }
    return result === 1;
  }

  /**
   * Get the number of active PTY sessions.
   */
  count(): number {
    this.#ensureOpen();
    return Number(lib.symbols.pty_count(this.#handle));
  }

  /**
   * Destroy the PTY manager handle and kill all child processes.
   */
  destroy(): void {
    if (this.#closed) return;
    this.#closed = true;
    lib.symbols.pty_manager_destroy(this.#handle);
  }

  [Symbol.dispose](): void {
    this.destroy();
  }

  #ensureOpen(): void {
    if (this.#closed) {
      throw new Error("PtyManager has been destroyed");
    }
  }
}
