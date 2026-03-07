// lib/extensions/isolation.ts
// Error isolation wrappers for extension lifecycle and event handlers.

import type { ExtensionModule } from "./types.ts";

/** Tracks error counts per extension for circuit-breaker logic. */
interface ErrorRecord {
  timestamps: number[];
}

const errorRecords: Map<string, ErrorRecord> = new Map();

/** Circuit breaker threshold: 3 errors in 60 seconds triggers auto-disable. */
const MAX_ERRORS = 3;
const ERROR_WINDOW_MS = 60_000;

/** Default timeout for activate() in milliseconds. */
const ACTIVATE_TIMEOUT_MS = 5_000;

/** Record an error for the named extension. Returns true if the circuit breaker trips. */
function recordError(extensionName: string): boolean {
  let record = errorRecords.get(extensionName);
  if (!record) {
    record = { timestamps: [] };
    errorRecords.set(extensionName, record);
  }
  const now = Date.now();
  record.timestamps.push(now);
  // Prune old entries outside the window
  record.timestamps = record.timestamps.filter(
    (t) => now - t < ERROR_WINDOW_MS,
  );
  return record.timestamps.length >= MAX_ERRORS;
}

/** Clear error records for an extension (e.g., on successful reload). */
export function clearErrors(extensionName: string): void {
  errorRecords.delete(extensionName);
}

/**
 * Safely call activate() with a timeout guard.
 * Returns an error string if activation fails, or null on success.
 */
export async function safeActivate(
  mod: ExtensionModule,
  ctx: Parameters<ExtensionModule["activate"]>[0],
  extensionName: string,
  timeoutMs: number = ACTIVATE_TIMEOUT_MS,
): Promise<string | null> {
  try {
    const result = mod.activate(ctx);
    if (result instanceof Promise) {
      await Promise.race([
        result,
        new Promise<never>((_, reject) =>
          setTimeout(
            () => reject(new Error(`activate() timed out after ${timeoutMs}ms`)),
            timeoutMs,
          )
        ),
      ]);
    }
    return null;
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    recordError(extensionName);
    return msg;
  }
}

/**
 * Safely call deactivate().
 * Returns an error string if deactivation fails, or null on success.
 */
export async function safeDeactivate(
  mod: ExtensionModule,
  extensionName: string,
): Promise<string | null> {
  try {
    const result = mod.deactivate();
    if (result instanceof Promise) {
      await result;
    }
    return null;
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    recordError(extensionName);
    return msg;
  }
}

/**
 * Wrap an event handler callback in a try/catch so extension errors
 * never crash the core runtime. Returns a wrapped handler.
 */
export function safeHandler(
  extensionName: string,
  handler: (payload: unknown) => void,
  onCircuitBreak?: (extensionName: string) => void,
): (payload: unknown) => void {
  return (payload: unknown) => {
    try {
      handler(payload);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(
        `[marauder] Extension "${extensionName}" event handler error: ${msg}`,
      );
      const tripped = recordError(extensionName);
      if (tripped && onCircuitBreak) {
        onCircuitBreak(extensionName);
      }
    }
  };
}

/** Check whether the circuit breaker has tripped for an extension. */
export function isCircuitBroken(extensionName: string): boolean {
  const record = errorRecords.get(extensionName);
  if (!record) return false;
  const now = Date.now();
  const recent = record.timestamps.filter((t) => now - t < ERROR_WINDOW_MS);
  return recent.length >= MAX_ERRORS;
}
