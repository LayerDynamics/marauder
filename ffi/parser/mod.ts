/**
 * Deno FFI bindings for marauder-parser.
 *
 * Wraps the C ABI exported by `libmarauder_parser` in an ergonomic TypeScript class.
 */

/** Resolve the path to the compiled parser shared library. */
function resolveLibPath(): string {
  const libName = (() => {
    switch (Deno.build.os) {
      case "darwin":
        return "libmarauder_parser.dylib";
      case "linux":
        return "libmarauder_parser.so";
      case "windows":
        return "marauder_parser.dll";
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
    parser_create: {
      parameters: [],
      result: "pointer",
    },
    parser_feed: {
      parameters: ["pointer", "pointer", "usize", "function", "pointer"],
      result: "void",
    },
    parser_reset: {
      parameters: ["pointer"],
      result: "void",
    },
    parser_destroy: {
      parameters: ["pointer"],
      result: "void",
    },
  } as const,
);

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** A parsed terminal action from the VT parser. */
export interface TerminalAction {
  type: string;
  [key: string]: unknown;
}

/** Callback type for receiving parsed actions. */
export type ActionCallback = (action: TerminalAction) => void;

/**
 * TypeScript wrapper around the marauder VT parser.
 *
 * Usage:
 * ```ts
 * using parser = new Parser();
 * const actions = parser.feed("\x1b[31mHello\x1b[0m");
 * console.log(actions);
 * ```
 */
export class Parser {
  #handle: Deno.PointerValue;
  #closed = false;

  constructor() {
    this.#handle = lib.symbols.parser_create();
    if (this.#handle === null) {
      throw new Error("Failed to create Parser native handle");
    }
  }

  /**
   * Feed input bytes and collect all parsed terminal actions.
   * Returns an array of parsed actions.
   */
  feed(input: string | Uint8Array): TerminalAction[] {
    this.#ensureOpen();

    const bytes = typeof input === "string" ? encoder.encode(input) : input;
    const inputPtr = Deno.UnsafePointer.of(bytes as unknown as ArrayBuffer);
    const actions: TerminalAction[] = [];

    const callback = new Deno.UnsafeCallback(
      {
        parameters: ["pointer", "usize", "pointer"],
        result: "void",
      } as const,
      (
        actionJsonPtr: Deno.PointerValue,
        actionJsonLen: number | bigint,
        _userData: Deno.PointerValue,
      ) => {
        const len = Number(actionJsonLen);
        if (len === 0 || actionJsonPtr === null) return;

        const view = new Deno.UnsafePointerView(actionJsonPtr);
        const jsonBytes = new Uint8Array(len);
        view.copyInto(jsonBytes);

        try {
          const parsed = JSON.parse(
            decoder.decode(jsonBytes),
          ) as TerminalAction;
          actions.push(parsed);
        } catch (err: unknown) {
          const msg = err instanceof Error ? err.message : String(err);
          console.warn(`[Parser] Failed to parse action JSON: ${msg}`);
        }
      },
    );

    try {
      lib.symbols.parser_feed(
        this.#handle,
        inputPtr,
        BigInt(bytes.byteLength),
        callback.pointer,
        null,
      );
    } finally {
      callback.close();
    }

    return actions;
  }

  /**
   * Feed input bytes and invoke a callback for each parsed action.
   * More efficient than `feed()` for streaming use cases.
   */
  feedWithCallback(input: string | Uint8Array, onAction: ActionCallback): void {
    this.#ensureOpen();

    const bytes = typeof input === "string" ? encoder.encode(input) : input;
    const inputPtr = Deno.UnsafePointer.of(bytes as unknown as ArrayBuffer);

    const callback = new Deno.UnsafeCallback(
      {
        parameters: ["pointer", "usize", "pointer"],
        result: "void",
      } as const,
      (
        actionJsonPtr: Deno.PointerValue,
        actionJsonLen: number | bigint,
        _userData: Deno.PointerValue,
      ) => {
        const len = Number(actionJsonLen);
        if (len === 0 || actionJsonPtr === null) return;

        const view = new Deno.UnsafePointerView(actionJsonPtr);
        const jsonBytes = new Uint8Array(len);
        view.copyInto(jsonBytes);

        try {
          const parsed = JSON.parse(
            decoder.decode(jsonBytes),
          ) as TerminalAction;
          onAction(parsed);
        } catch (err: unknown) {
          const msg = err instanceof Error ? err.message : String(err);
          console.warn(`[Parser] Failed to parse action JSON: ${msg}`);
        }
      },
    );

    try {
      lib.symbols.parser_feed(
        this.#handle,
        inputPtr,
        BigInt(bytes.byteLength),
        callback.pointer,
        null,
      );
    } finally {
      callback.close();
    }
  }

  /**
   * Reset the parser to initial state.
   */
  reset(): void {
    this.#ensureOpen();
    lib.symbols.parser_reset(this.#handle);
  }

  /**
   * Destroy the parser handle.
   */
  destroy(): void {
    if (this.#closed) return;
    this.#closed = true;
    lib.symbols.parser_destroy(this.#handle);
  }

  [Symbol.dispose](): void {
    this.destroy();
  }

  #ensureOpen(): void {
    if (this.#closed) {
      throw new Error("Parser has been destroyed");
    }
  }
}
