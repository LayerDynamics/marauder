/**
 * Shared FFI utilities for all marauder FFI bindings.
 */

const encoder = new TextEncoder();

/**
 * Resolve the path to a compiled marauder shared library.
 *
 * Resolution order:
 * 1. `MARAUDER_LIB_DIR` environment variable
 * 2. `target/release/`
 * 3. `target/debug/` (fallback)
 */
export function resolveLibPath(crateName: string): string {
  const libName = (() => {
    switch (Deno.build.os) {
      case "darwin":
        return `lib${crateName}.dylib`;
      case "linux":
        return `lib${crateName}.so`;
      case "windows":
        return `${crateName}.dll`;
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

  // Fall back to debug path (will error on dlopen if missing)
  return `target/debug/${libName}`;
}

/**
 * Get a pointer suitable for FFI from a typed array.
 * Uses the underlying ArrayBuffer to avoid unsafe casts.
 */
export function bufferPtr(
  buf: Uint8Array | Uint32Array,
): Deno.PointerValue {
  return Deno.UnsafePointer.of(buf.buffer as ArrayBuffer);
}

/** Encode a string as a null-terminated C string in a Uint8Array. */
export function toCString(s: string): Uint8Array {
  const bytes = encoder.encode(s);
  const buf = new Uint8Array(bytes.length + 1);
  buf.set(bytes);
  buf[bytes.length] = 0;
  return buf;
}
