/**
 * Deno FFI bindings for marauder-grid.
 *
 * Wraps the C ABI exported by `libmarauder_grid` in an ergonomic TypeScript class.
 */

import { bufferPtr, resolveLibPath } from "../_lib.ts";

const lib = Deno.dlopen(
  resolveLibPath("marauder_grid"),
  {
    grid_create: {
      parameters: ["u16", "u16"],
      result: "pointer",
    },
    grid_apply_action: {
      parameters: ["pointer", "pointer", "usize"],
      result: "i32",
    },
    grid_get_cell: {
      parameters: ["pointer", "usize", "usize", "pointer", "usize"],
      result: "usize",
    },
    grid_get_cursor: {
      parameters: ["pointer"],
      result: "u64",
    },
    grid_resize: {
      parameters: ["pointer", "u16", "u16"],
      result: "i32",
    },
    grid_scroll_viewport: {
      parameters: ["pointer", "u32"],
      result: "void",
    },
    grid_select: {
      parameters: ["pointer", "u32", "u32", "u32", "u32"],
      result: "void",
    },
    grid_get_selection_text: {
      parameters: ["pointer", "pointer", "usize"],
      result: "usize",
    },
    grid_get_dirty_rows: {
      parameters: ["pointer", "pointer", "usize"],
      result: "usize",
    },
    grid_clear_dirty: {
      parameters: ["pointer"],
      result: "void",
    },
    grid_destroy: {
      parameters: ["pointer"],
      result: "void",
    },
  } as const,
);

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Cursor position. */
export interface CursorPosition {
  row: number;
  col: number;
}

/** A terminal cell's data (deserialized from JSON). */
export interface Cell {
  char: string;
  fg: unknown;
  bg: unknown;
  attrs: unknown;
  width: number;
}

/**
 * TypeScript wrapper around the marauder terminal grid.
 *
 * Usage:
 * ```ts
 * using grid = new Grid(24, 80);
 * grid.applyAction({ type: "Print", char: "A" });
 * const cursor = grid.getCursor();
 * ```
 */
export class Grid {
  #handle: Deno.PointerValue;
  #closed = false;
  #rows: number;
  #cols: number;

  constructor(rows: number, cols: number) {
    this.#handle = lib.symbols.grid_create(rows, cols);
    if (this.#handle === null) {
      throw new Error("Failed to create Grid native handle");
    }
    this.#rows = rows;
    this.#cols = cols;
  }

  /** Current number of rows. */
  get rows(): number {
    return this.#rows;
  }

  /** Current number of columns. */
  get cols(): number {
    return this.#cols;
  }

  /**
   * Apply a terminal action to the grid (from parser output).
   * The action should be a JSON-serializable TerminalAction object.
   */
  applyAction(action: unknown): boolean {
    this.#ensureOpen();

    const jsonBytes = encoder.encode(JSON.stringify(action));

    const result = lib.symbols.grid_apply_action(
      this.#handle,
      bufferPtr(jsonBytes),
      BigInt(jsonBytes.byteLength),
    );

    return result === 1;
  }

  /**
   * Get a cell's data at the given position.
   * Returns null if the position is out of bounds.
   */
  getCell(row: number, col: number): Cell | null {
    this.#ensureOpen();

    const bufSize = 1024;
    const buf = new Uint8Array(bufSize);

    const written = Number(
      lib.symbols.grid_get_cell(
        this.#handle,
        BigInt(row),
        BigInt(col),
        bufferPtr(buf),
        BigInt(bufSize),
      ),
    );

    if (written === 0) return null;

    const json = decoder.decode(buf.subarray(0, written));
    return JSON.parse(json) as Cell;
  }

  /**
   * Get the current cursor position.
   */
  getCursor(): CursorPosition {
    this.#ensureOpen();

    const packed = lib.symbols.grid_get_cursor(this.#handle);
    const value = typeof packed === "bigint" ? packed : BigInt(packed);

    return {
      row: Number(value >> 32n),
      col: Number(value & 0xFFFFFFFFn),
    };
  }

  /**
   * Resize the grid.
   */
  resize(rows: number, cols: number): void {
    this.#ensureOpen();

    const result = lib.symbols.grid_resize(this.#handle, rows, cols);
    if (result === 0) {
      throw new Error(`Failed to resize grid to ${rows}x${cols}`);
    }
    this.#rows = rows;
    this.#cols = cols;
  }

  /**
   * Set the viewport scroll offset.
   * 0 = bottom (live), N = scrolled N lines up into scrollback.
   */
  scrollViewport(offset: number): void {
    this.#ensureOpen();
    lib.symbols.grid_scroll_viewport(this.#handle, offset);
  }

  /**
   * Set a text selection range.
   */
  setSelection(
    startRow: number,
    startCol: number,
    endRow: number,
    endCol: number,
  ): void {
    this.#ensureOpen();
    lib.symbols.grid_select(this.#handle, startRow, startCol, endRow, endCol);
  }

  /**
   * Clear the current text selection.
   */
  clearSelection(): void {
    this.#ensureOpen();
    // u32::MAX sentinel clears selection
    lib.symbols.grid_select(this.#handle, 0xFFFFFFFF, 0, 0xFFFFFFFF, 0);
  }

  /**
   * Get the selected text. Returns null if no selection.
   */
  getSelectionText(): string | null {
    this.#ensureOpen();

    const bufSize = 65536;
    const buf = new Uint8Array(bufSize);

    const written = Number(
      lib.symbols.grid_get_selection_text(
        this.#handle,
        bufferPtr(buf),
        BigInt(bufSize),
      ),
    );

    if (written === 0) return null;
    return decoder.decode(buf.subarray(0, written));
  }

  /**
   * Get indices of dirty rows (rows modified since last clear).
   */
  getDirtyRows(): number[] {
    this.#ensureOpen();

    const maxRows = 1024;
    const buf = new Uint32Array(maxRows);

    const count = Number(
      lib.symbols.grid_get_dirty_rows(
        this.#handle,
        bufferPtr(buf),
        BigInt(maxRows),
      ),
    );

    const rows: number[] = [];
    for (let i = 0; i < count; i++) {
      rows.push(buf[i]!);
    }
    return rows;
  }

  /**
   * Clear dirty tracking flags.
   */
  clearDirty(): void {
    this.#ensureOpen();
    lib.symbols.grid_clear_dirty(this.#handle);
  }

  /**
   * Destroy the grid handle.
   */
  destroy(): void {
    if (this.#closed) return;
    this.#closed = true;
    lib.symbols.grid_destroy(this.#handle);
  }

  [Symbol.dispose](): void {
    this.destroy();
  }

  #ensureOpen(): void {
    if (this.#closed) {
      throw new Error("Grid has been destroyed");
    }
  }
}
