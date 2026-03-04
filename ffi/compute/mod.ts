/**
 * Deno FFI bindings for marauder-compute.
 *
 * Wraps the C ABI exported by `libmarauder_compute` in an ergonomic TypeScript class.
 * NOTE: pkg/compute C ABI is not yet implemented. This module provides the type
 * interfaces and will be wired up once `pkg/compute/src/ffi.rs` is complete.
 */

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
  url: string;
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

/**
 * TypeScript wrapper around the marauder GPU compute engine.
 *
 * This class will wrap the wgpu compute shaders once the C ABI is implemented.
 * Currently provides the interface contract for downstream consumers.
 */
export class ComputeEngine {
  #handle: Deno.PointerValue | null = null;
  #closed = false;

  constructor(_deviceShared: Deno.PointerValue) {
    // TODO: Wire to compute_create once C ABI is available
    throw new Error(
      "ComputeEngine FFI not yet implemented — pkg/compute C ABI pending",
    );
  }

  /**
   * Search for a pattern across the grid buffer.
   * Returns matching positions.
   */
  search(_pattern: string): SearchMatch[] {
    this.#ensureOpen();
    return [];
  }

  /**
   * Detect URLs in a range of rows.
   */
  detectUrls(_startRow: number, _endRow: number): UrlMatch[] {
    this.#ensureOpen();
    return [];
  }

  /**
   * Apply semantic highlighting rules to the grid.
   */
  highlightCells(_rules: HighlightRule[]): void {
    this.#ensureOpen();
  }

  /**
   * Extract text from a selection range.
   */
  extractSelection(_start: CellPos, _end: CellPos): string {
    this.#ensureOpen();
    return "";
  }

  destroy(): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#handle = null;
  }

  [Symbol.dispose](): void {
    this.destroy();
  }

  #ensureOpen(): void {
    if (this.#closed) {
      throw new Error("ComputeEngine has been destroyed");
    }
  }
}
