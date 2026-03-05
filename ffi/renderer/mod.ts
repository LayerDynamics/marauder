/**
 * Deno FFI bindings for marauder-renderer.
 *
 * Wraps the C ABI exported by `libmarauder_renderer` in an ergonomic TypeScript class.
 */

import { resolve } from "jsr:@std/path@^1.0.0";

/** Theme color map for the renderer. */
export interface ThemeColors {
  background: [number, number, number, number];
  foreground: [number, number, number, number];
  cursor: [number, number, number, number];
  selection: [number, number, number, number];
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

/** Grid dimensions. */
export interface GridDimensions {
  rows: number;
  cols: number;
}

/** Renderer configuration (mirrors Rust RendererConfig). */
export interface RendererConfig {
  font_family?: string;
  font_size?: number;
  line_height?: number;
  cursor_style?: "Block" | "Underline" | "Bar";
  cursor_blink?: boolean;
  theme?: ThemeColors;
}

/** Overlay layer configuration. */
export interface OverlayConfig {
  layer_id: number;
  visible: boolean;
  data?: Record<string, unknown>;
}

const CURSOR_STYLE_MAP: Record<CursorStyle, number> = {
  block: 0,
  underline: 1,
  bar: 2,
};

function findLibPath(): string {
  const envDir = Deno.env.get("MARAUDER_LIB_DIR");
  const ext = Deno.build.os === "darwin"
    ? "dylib"
    : Deno.build.os === "windows"
    ? "dll"
    : "so";
  const name = `libmarauder_renderer.${ext}`;

  if (envDir) {
    return resolve(envDir, name);
  }
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
  renderer_create: {
    parameters: ["u32", "u32", "f32", "buffer", "usize"],
    result: "pointer",
  },
  renderer_destroy: {
    parameters: ["pointer"],
    result: "void",
  },
  renderer_set_font: {
    parameters: ["pointer", "buffer", "usize", "f32", "f32"],
    result: "i32",
  },
  renderer_set_theme: {
    parameters: ["pointer", "buffer", "usize"],
    result: "i32",
  },
  renderer_set_cursor_style: {
    parameters: ["pointer", "u32", "u32"],
    result: "i32",
  },
  renderer_update_cells: {
    parameters: ["pointer", "pointer"],
    result: "i32",
  },
  renderer_render_frame: {
    parameters: ["pointer"],
    result: "i32",
  },
  renderer_resize_surface: {
    parameters: ["pointer", "u32", "u32", "f32"],
    result: "i32",
  },
  renderer_get_cell_size: {
    parameters: ["pointer", "buffer", "buffer"],
    result: "i32",
  },
  renderer_get_grid_dimensions: {
    parameters: ["pointer", "buffer", "buffer"],
    result: "i32",
  },
  renderer_get_device_ptr: {
    parameters: ["pointer"],
    result: "pointer",
  },
  renderer_get_queue_ptr: {
    parameters: ["pointer"],
    result: "pointer",
  },
  renderer_free_device_ptr: {
    parameters: ["pointer"],
    result: "void",
  },
  renderer_free_queue_ptr: {
    parameters: ["pointer"],
    result: "void",
  },
  renderer_add_overlay: {
    parameters: ["pointer", "buffer", "usize"],
    result: "i32",
  },
  renderer_remove_overlay: {
    parameters: ["pointer", "u32"],
    result: "i32",
  },
});

const encoder = new TextEncoder();

/**
 * TypeScript wrapper around the marauder GPU renderer.
 *
 * Creates a headless renderer suitable for cell size queries, font/theme
 * configuration, and device sharing with ComputeEngine. The primary
 * windowed renderer is created on the Tauri/Rust side.
 */
/** Error code returned by Rust when the renderer mutex is poisoned. */
const ERR_POISONED = -99;

/** Thrown when the renderer's internal mutex is poisoned (prior panic). */
export class RendererPoisonedError extends Error {
  constructor(fn_name: string) {
    super(
      `${fn_name}: renderer mutex poisoned — GPU state is corrupt. ` +
        `Destroy this instance and create a new one.`,
    );
    this.name = "RendererPoisonedError";
  }
}

/** Check an FFI result code; throw on poison or generic failure. */
function checkResult(result: number, fn_name: string): void {
  if (result === ERR_POISONED) {
    throw new RendererPoisonedError(fn_name);
  }
  if (result !== 0) {
    throw new Error(`${fn_name} failed (code ${result})`);
  }
}

export class Renderer {
  #handle: Deno.PointerValue;
  #closed = false;
  #devicePtr: Deno.PointerValue | null = null;
  #queuePtr: Deno.PointerValue | null = null;

  /**
   * Create a headless renderer.
   *
   * @param width  Surface width in pixels (used for grid dimension calculations).
   * @param height Surface height in pixels.
   * @param scaleFactor DPI scale factor (default 1.0).
   * @param config Optional renderer configuration.
   */
  constructor(
    width: number,
    height: number,
    scaleFactor = 1.0,
    config?: RendererConfig,
  ) {
    let configBuf: Uint8Array | null = null;
    let configLen = 0;
    if (config) {
      configBuf = encoder.encode(JSON.stringify(config));
      configLen = configBuf.byteLength;
    }
    this.#handle = lib.symbols.renderer_create(
      width,
      height,
      scaleFactor,
      configBuf ?? new Uint8Array(0),
      configLen,
    );
    if (this.#handle === null) {
      throw new Error("Failed to create Renderer — no GPU adapter available");
    }
  }

  /** Set font family, size, and line height. Rebuilds the glyph atlas. */
  setFont(config: FontConfig): void {
    this.#ensureOpen();
    const familyBuf = config.family ? encoder.encode(config.family) : new Uint8Array(0);
    const result = lib.symbols.renderer_set_font(
      this.#handle,
      familyBuf,
      familyBuf.byteLength,
      config.size,
      config.lineHeight,
    );
    checkResult(result, "renderer_set_font");
  }

  /** Set theme colors. */
  setTheme(theme: ThemeColors): void {
    this.#ensureOpen();
    const json = encoder.encode(JSON.stringify(theme));
    const result = lib.symbols.renderer_set_theme(
      this.#handle,
      json,
      json.byteLength,
    );
    checkResult(result, "renderer_set_theme");
  }

  /** Update instance buffers from a Grid FFI handle. */
  updateCells(gridHandle: Deno.PointerValue): void {
    this.#ensureOpen();
    const result = lib.symbols.renderer_update_cells(this.#handle, gridHandle);
    checkResult(result, "renderer_update_cells");
  }

  /** Render a frame (no-op for headless renderer). */
  renderFrame(): void {
    this.#ensureOpen();
    const result = lib.symbols.renderer_render_frame(this.#handle);
    checkResult(result, "renderer_render_frame");
  }

  /** Resize the rendering surface. */
  resizeSurface(width: number, height: number, scaleFactor: number): void {
    this.#ensureOpen();
    const result = lib.symbols.renderer_resize_surface(
      this.#handle,
      width,
      height,
      scaleFactor,
    );
    checkResult(result, "renderer_resize_surface");
  }

  /** Set cursor style and blink. */
  setCursorStyle(style: CursorStyle, blink: boolean): void {
    this.#ensureOpen();
    const styleNum = CURSOR_STYLE_MAP[style];
    const result = lib.symbols.renderer_set_cursor_style(
      this.#handle,
      styleNum,
      blink ? 1 : 0,
    );
    checkResult(result, "renderer_set_cursor_style");
  }

  /** Get cell dimensions in pixels. */
  getCellSize(): CellSize {
    this.#ensureOpen();
    const buf = new ArrayBuffer(8); // 2 × f32
    const bytes = new Uint8Array(buf);
    const view = new DataView(buf);
    const result = lib.symbols.renderer_get_cell_size(
      this.#handle,
      bytes.subarray(0, 4),
      bytes.subarray(4, 8),
    );
    checkResult(result, "renderer_get_cell_size");
    return {
      width: view.getFloat32(0, true),
      height: view.getFloat32(4, true),
    };
  }

  /** Get grid dimensions (rows, cols) for current surface size. */
  getGridDimensions(): GridDimensions {
    this.#ensureOpen();
    const buf = new ArrayBuffer(4); // 2 × u16
    const bytes = new Uint8Array(buf);
    const view = new DataView(buf);
    const result = lib.symbols.renderer_get_grid_dimensions(
      this.#handle,
      bytes.subarray(0, 2),
      bytes.subarray(2, 4),
    );
    checkResult(result, "renderer_get_grid_dimensions");
    return {
      rows: view.getUint16(0, true),
      cols: view.getUint16(2, true),
    };
  }

  /**
   * Get a heap-allocated Arc<wgpu::Device> pointer for sharing with ComputeEngine.
   * Each call allocates a new Arc clone — cache the result and free with freeDevicePtr().
   */
  getDevicePtr(): Deno.PointerValue {
    this.#ensureOpen();
    const ptr = lib.symbols.renderer_get_device_ptr(this.#handle);
    if (ptr === null) {
      throw new Error("renderer_get_device_ptr returned null");
    }
    this.#devicePtr = ptr;
    return ptr;
  }

  /**
   * Get a heap-allocated Arc<wgpu::Queue> pointer for sharing with ComputeEngine.
   * Each call allocates a new Arc clone — cache the result and free with freeQueuePtr().
   */
  getQueuePtr(): Deno.PointerValue {
    this.#ensureOpen();
    const ptr = lib.symbols.renderer_get_queue_ptr(this.#handle);
    if (ptr === null) {
      throw new Error("renderer_get_queue_ptr returned null");
    }
    this.#queuePtr = ptr;
    return ptr;
  }

  /** Free a previously obtained device Arc pointer. */
  freeDevicePtr(ptr: Deno.PointerValue): void {
    lib.symbols.renderer_free_device_ptr(ptr);
    if (this.#devicePtr === ptr) this.#devicePtr = null;
  }

  /** Free a previously obtained queue Arc pointer. */
  freeQueuePtr(ptr: Deno.PointerValue): void {
    lib.symbols.renderer_free_queue_ptr(ptr);
    if (this.#queuePtr === ptr) this.#queuePtr = null;
  }

  /** Add an overlay layer. Replaces any existing overlay with the same layer_id. */
  addOverlay(config: OverlayConfig): void {
    this.#ensureOpen();
    const json = encoder.encode(JSON.stringify(config));
    const result = lib.symbols.renderer_add_overlay(this.#handle, json, json.byteLength);
    checkResult(result, "renderer_add_overlay");
  }

  /** Remove an overlay layer by ID. Returns true if it existed. */
  removeOverlay(layerId: number): boolean {
    this.#ensureOpen();
    const result = lib.symbols.renderer_remove_overlay(this.#handle, layerId);
    if (result === -1) {
      throw new Error("renderer_remove_overlay failed");
    }
    return result === 0;
  }

  /** Destroy the renderer, freeing GPU resources. */
  destroy(): void {
    if (this.#closed) return;
    this.#closed = true;
    // Free any outstanding Arc pointers before destroying the handle
    if (this.#devicePtr) {
      lib.symbols.renderer_free_device_ptr(this.#devicePtr);
      this.#devicePtr = null;
    }
    if (this.#queuePtr) {
      lib.symbols.renderer_free_queue_ptr(this.#queuePtr);
      this.#queuePtr = null;
    }
    lib.symbols.renderer_destroy(this.#handle);
    this.#handle = null as unknown as Deno.PointerValue;
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
