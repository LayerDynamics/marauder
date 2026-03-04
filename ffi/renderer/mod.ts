/**
 * Deno FFI bindings for marauder-renderer.
 *
 * Wraps the C ABI exported by `libmarauder_renderer` in an ergonomic TypeScript class.
 * NOTE: pkg/renderer C ABI is not yet implemented. This module provides the type
 * interfaces and will be wired up once `pkg/renderer/src/ffi.rs` is complete.
 */

/** Theme color map for the renderer. */
export interface ThemeColors {
  background: string;
  foreground: string;
  cursor: string;
  selection: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

/** Cursor style. */
export type CursorStyle = "block" | "underline" | "bar";

/** Font configuration. */
export interface FontConfig {
  family: string;
  size: number;
  lineHeight: number;
}

/** Cell size in pixels. */
export interface CellSize {
  width: number;
  height: number;
}

/**
 * TypeScript wrapper around the marauder GPU renderer.
 *
 * This class will wrap the wgpu-based renderer once the C ABI is implemented.
 * Currently provides the interface contract for downstream consumers.
 */
export class Renderer {
  #handle: Deno.PointerValue | null = null;
  #closed = false;

  constructor(_windowHandle: Deno.PointerValue) {
    // TODO: Wire to renderer_create once C ABI is available
    throw new Error(
      "Renderer FFI not yet implemented — pkg/renderer C ABI pending",
    );
  }

  setFont(_config: FontConfig): void {
    this.#ensureOpen();
  }

  setTheme(_theme: ThemeColors): void {
    this.#ensureOpen();
  }

  updateCells(_gridHandle: Deno.PointerValue): void {
    this.#ensureOpen();
  }

  renderFrame(): void {
    this.#ensureOpen();
  }

  resizeSurface(_width: number, _height: number, _scaleFactor: number): void {
    this.#ensureOpen();
  }

  setCursorStyle(_style: CursorStyle, _blink: boolean): void {
    this.#ensureOpen();
  }

  getCellSize(): CellSize {
    this.#ensureOpen();
    return { width: 0, height: 0 };
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
      throw new Error("Renderer has been destroyed");
    }
  }
}
