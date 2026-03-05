/**
 * Deno FFI bindings for marauder-compute.
 *
 * Wraps the C ABI exported by `libmarauder_compute` in an ergonomic TypeScript class.
 */

import { resolve } from "jsr:@std/path@^1.0.0";

/** A search match result. */
export interface SearchMatch {
  row: number;
  col: number;
  length: number;
}

/** A detected URL in the grid. */
export interface UrlMatch {
  row: number;
  startCol: number;
  endCol: number;
}

/** A cell position. */
export interface CellPos {
  row: number;
  col: number;
}

/** Highlight rule for semantic highlighting. */
export interface HighlightRule {
  pattern: string;
  category: string;
  color: string;
}

/** Highlight categories from the GPU classifier. */
export type HighlightCategory = "None" | "Number" | "FilePath" | "Flag" | "Operator";

/** A highlight result for a cell. */
export interface HighlightResult {
  row: number;
  col: number;
  category: HighlightCategory;
}

/** GPU cell data for upload. */
export interface GpuCell {
  codepoint: number;
  fg_packed: number;
  bg_packed: number;
  flags: number;
  row: number;
  col: number;
}

function findLibPath(): string {
  const envDir = Deno.env.get("MARAUDER_LIB_DIR");
  const ext = Deno.build.os === "darwin" ? "dylib" : Deno.build.os === "windows" ? "dll" : "so";
  const name = `libmarauder_compute.${ext}`;

  if (envDir) {
    return resolve(envDir, name);
  }
  // Try release first, then debug
  for (const profile of ["release", "debug"]) {
    const path = resolve("target", profile, name);
    try {
      Deno.statSync(path);
      return path;
    } catch {
      // continue
    }
  }
  return resolve("target", "debug", name);
}

const lib = Deno.dlopen(findLibPath(), {
  compute_create: {
    parameters: [],
    result: "pointer",
  },
  compute_create_shared: {
    parameters: ["pointer", "pointer"],
    result: "pointer",
  },
  compute_upload_cells: {
    parameters: ["pointer", "buffer", "usize", "u32", "u32"],
    result: "i32",
  },
  compute_upload_from_grid: {
    parameters: ["pointer", "pointer"],
    result: "i32",
  },
  compute_search: {
    parameters: ["pointer", "buffer", "usize", "buffer", "usize"],
    result: "usize",
  },
  compute_detect_urls: {
    parameters: ["pointer", "u32", "u32", "buffer", "usize"],
    result: "usize",
  },
  compute_highlight_cells: {
    parameters: ["pointer", "buffer", "usize"],
    result: "usize",
  },
  compute_extract_selection: {
    parameters: ["pointer", "u32", "u32", "u32", "u32", "buffer", "usize"],
    result: "usize",
  },
  compute_destroy: {
    parameters: ["pointer"],
    result: "void",
  },
});

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Default output buffer size (1MB). */
const DEFAULT_BUF_SIZE = 1024 * 1024;

/** Maximum buffer growth (64MB). */
const MAX_BUF_SIZE = 64 * 1024 * 1024;

/**
 * Sentinel value returned by Rust FFI when the output buffer is too small.
 * Rust returns `usize::MAX`; Deno FFI surfaces large usize as BigInt on 64-bit.
 */
function isBufferTooSmall(written: number | bigint): boolean {
  // Handle both number and BigInt representations
  if (typeof written === "bigint") {
    return written === 0xFFFFFFFFFFFFFFFFn;
  }
  // On 32-bit (unlikely), usize::MAX = 0xFFFFFFFF
  return written === 0xFFFFFFFF;
}

/**
 * TypeScript wrapper around the marauder GPU compute engine.
 */
export class ComputeEngine {
  #handle: Deno.PointerValue;
  #closed = false;
  #outBuf = new Uint8Array(DEFAULT_BUF_SIZE);

  /** Create a standalone compute engine (allocates its own GPU device). */
  constructor();
  /** Create a compute engine sharing the renderer's device and queue. */
  constructor(devicePtr: Deno.PointerValue, queuePtr: Deno.PointerValue);
  constructor(devicePtr?: Deno.PointerValue, queuePtr?: Deno.PointerValue) {
    if (devicePtr && queuePtr) {
      this.#handle = lib.symbols.compute_create_shared(devicePtr, queuePtr);
    } else {
      this.#handle = lib.symbols.compute_create();
    }
    if (this.#handle === null) {
      throw new Error("Failed to create ComputeEngine — no GPU adapter available");
    }
  }

  /** Upload cell data as GpuCell JSON for GPU processing. */
  uploadCells(cells: GpuCell[], rows: number, cols: number): void {
    this.#ensureOpen();
    const json = encoder.encode(JSON.stringify(cells));
    const result = lib.symbols.compute_upload_cells(
      this.#handle,
      json,
      json.byteLength,
      rows,
      cols,
    );
    if (result === 0) {
      throw new Error("Failed to upload cells");
    }
  }

  /** Upload cells directly from a Grid FFI handle. */
  uploadFromGrid(gridHandle: Deno.PointerValue): void {
    this.#ensureOpen();
    const result = lib.symbols.compute_upload_from_grid(this.#handle, gridHandle);
    if (result === 0) {
      throw new Error("Failed to upload from grid");
    }
  }

  /** Search for a pattern across the grid buffer. */
  search(pattern: string): SearchMatch[] {
    this.#ensureOpen();
    const patternBytes = encoder.encode(pattern);
    for (;;) {
      const outBuf = this.#outBuf;
      const written = lib.symbols.compute_search(
        this.#handle,
        patternBytes,
        patternBytes.byteLength,
        outBuf,
        outBuf.byteLength,
      );
      if (isBufferTooSmall(written)) {
        this.#growBuf();
        continue;
      }
      if (written === 0) return [];
      return JSON.parse(decoder.decode(outBuf.subarray(0, written as number)));
    }
  }

  /** Detect URLs in a range of rows. */
  detectUrls(startRow: number, endRow: number): UrlMatch[] {
    this.#ensureOpen();
    for (;;) {
      const outBuf = this.#outBuf;
      const written = lib.symbols.compute_detect_urls(
        this.#handle,
        startRow,
        endRow,
        outBuf,
        outBuf.byteLength,
      );
      if (isBufferTooSmall(written)) {
        this.#growBuf();
        continue;
      }
      if (written === 0) return [];
      const raw: Array<{ row: number; start_col: number; end_col: number }> =
        JSON.parse(decoder.decode(outBuf.subarray(0, written as number)));
      return raw.map((m) => ({
        row: m.row,
        startCol: m.start_col,
        endCol: m.end_col,
      }));
    }
  }

  /** Classify cells for semantic highlighting. */
  highlightCells(): HighlightResult[] {
    this.#ensureOpen();
    for (;;) {
      const outBuf = this.#outBuf;
      const written = lib.symbols.compute_highlight_cells(
        this.#handle,
        outBuf,
        outBuf.byteLength,
      );
      if (isBufferTooSmall(written)) {
        this.#growBuf();
        continue;
      }
      if (written === 0) return [];
      return JSON.parse(decoder.decode(outBuf.subarray(0, written as number)));
    }
  }

  /** Extract text from a selection range. */
  extractSelection(start: CellPos, end: CellPos): string {
    this.#ensureOpen();
    for (;;) {
      const outBuf = this.#outBuf;
      const written = lib.symbols.compute_extract_selection(
        this.#handle,
        start.row,
        start.col,
        end.row,
        end.col,
        outBuf,
        outBuf.byteLength,
      );
      if (isBufferTooSmall(written)) {
        this.#growBuf();
        continue;
      }
      if (written === 0) return "";
      return decoder.decode(outBuf.subarray(0, written as number));
    }
  }

  /** Destroy the compute engine, freeing GPU resources. */
  destroy(): void {
    if (this.#closed) return;
    this.#closed = true;
    lib.symbols.compute_destroy(this.#handle);
    this.#handle = null as unknown as Deno.PointerValue;
  }

  [Symbol.dispose](): void {
    this.destroy();
  }

  #ensureOpen(): void {
    if (this.#closed) {
      throw new Error("ComputeEngine has been destroyed");
    }
  }

  /** Double the output buffer size for retry, up to MAX_BUF_SIZE. */
  #growBuf(): void {
    const newSize = Math.min(this.#outBuf.byteLength * 2, MAX_BUF_SIZE);
    if (newSize === this.#outBuf.byteLength) {
      throw new Error(
        `ComputeEngine: result exceeds maximum buffer size (${MAX_BUF_SIZE} bytes)`
      );
    }
    this.#outBuf = new Uint8Array(newSize);
  }
}
