/**
 * Deno FFI bindings for marauder-event-bus.
 *
 * Wraps the C ABI exported by `libmarauder_event_bus` in an ergonomic TypeScript class.
 */

/** Event type discriminants matching `EventType` in `pkg/event-bus/src/events.rs`. */
export enum EventType {
  // Input layer
  KeyInput = 0,
  MouseInput = 1,
  PasteInput = 2,
  // PTY layer
  PtyOutput = 3,
  PtyExit = 4,
  PtyError = 5,
  // Parser layer
  ParserAction = 6,
  // Grid layer
  GridUpdated = 7,
  GridResized = 8,
  GridScrolled = 9,
  SelectionChanged = 10,
  // Shell layer
  ShellPromptDetected = 11,
  ShellCommandStarted = 12,
  ShellCommandFinished = 13,
  ShellCwdChanged = 14,
  // Render layer
  RenderFrameRequested = 15,
  RenderFrameCompleted = 16,
  OverlayChanged = 17,
  // Config layer
  ConfigChanged = 18,
  ConfigError = 19,
  // Lifecycle
  SessionCreated = 20,
  SessionClosed = 21,
  PaneCreated = 22,
  PaneClosed = 23,
  PaneFocused = 24,
  TabCreated = 25,
  TabClosed = 26,
  TabFocused = 27,
  // Extension layer
  ExtensionLoaded = 28,
  ExtensionUnloaded = 29,
  ExtensionMessage = 30,
}

/** Deserialized event received from the bus. */
export interface BusEvent {
  event_type: string;
  /** JSON-serialized payload bytes (arrives as number[] from serde Vec<u8>). */
  payload: number[];
  timestamp_us: number;
  source: string | null;
}

/** Resolve the path to the compiled event-bus shared library. */
function resolveLibPath(): string {
  const libName = (() => {
    switch (Deno.build.os) {
      case "darwin":
        return "libmarauder_event_bus.dylib";
      case "linux":
        return "libmarauder_event_bus.so";
      case "windows":
        return "marauder_event_bus.dll";
      default:
        throw new Error(`Unsupported platform: ${Deno.build.os}`);
    }
  })();

  const envDir = Deno.env.get("MARAUDER_LIB_DIR");
  if (envDir) {
    return `${envDir}/${libName}`;
  }

  // Try release first, then debug
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

const lib = Deno.dlopen(resolveLibPath(), {
  event_bus_create: {
    parameters: [],
    result: "pointer",
  },
  event_bus_subscribe: {
    parameters: ["pointer", "u32", "function", "pointer"],
    result: "u64",
  },
  event_bus_unsubscribe: {
    parameters: ["pointer", "u32", "u64"],
    result: "i32",
  },
  event_bus_publish: {
    parameters: ["pointer", "u32", "pointer", "usize"],
    result: "i32",
  },
  event_bus_destroy: {
    parameters: ["pointer"],
    result: "void",
  },
} as const);

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Callback type for event subscribers. */
export type EventCallback = (event: BusEvent) => void;

/** Tracked subscription for cleanup. */
interface Subscription {
  eventType: EventType;
  subscriberId: number | bigint;
  ffiCallback: Deno.UnsafeCallback;
}

/**
 * TypeScript wrapper around the marauder event bus.
 *
 * Usage:
 * ```ts
 * using bus = new EventBus();
 * bus.subscribe(EventType.KeyInput, (event) => { console.log(event); });
 * bus.publish(EventType.KeyInput, { key: "a" });
 * ```
 */
export class EventBus {
  #handle: Deno.PointerValue;
  #subscriptions: Map<number | bigint, Subscription> = new Map();
  #closed = false;

  constructor() {
    this.#handle = lib.symbols.event_bus_create();
    if (this.#handle === null) {
      throw new Error("Failed to create EventBus native handle");
    }
  }

  /**
   * Subscribe to events of a given type.
   * Returns a subscriber ID that can be used to unsubscribe.
   */
  subscribe(eventType: EventType, callback: EventCallback): number | bigint {
    this.#ensureOpen();

    // Create a C callback that receives (event_json_ptr, event_json_len, user_data)
    const ffiCallback = new Deno.UnsafeCallback(
      {
        parameters: ["pointer", "usize", "pointer"],
        result: "void",
      } as const,
      (eventJsonPtr: Deno.PointerValue, eventJsonLen: number | bigint, _userData: Deno.PointerValue) => {
        const len = Number(eventJsonLen);
        if (len === 0 || eventJsonPtr === null) return;

        const view = new Deno.UnsafePointerView(eventJsonPtr);
        const jsonBytes = new Uint8Array(len);
        view.copyInto(jsonBytes);

        try {
          const parsed = JSON.parse(decoder.decode(jsonBytes)) as BusEvent;
          callback(parsed);
        } catch (err: unknown) {
          const msg = err instanceof Error ? err.message : String(err);
          console.warn(`[EventBus] Failed to parse event JSON: ${msg}`);
        }
      },
    );

    const subscriberId = lib.symbols.event_bus_subscribe(
      this.#handle,
      eventType as number,
      ffiCallback.pointer,
      null, // user_data — not needed, closure captures state
    );

    if (subscriberId === 0 || subscriberId === 0n) {
      ffiCallback.close();
      throw new Error(`Failed to subscribe to event type ${eventType}`);
    }

    this.#subscriptions.set(subscriberId, {
      eventType,
      subscriberId,
      ffiCallback,
    });

    return subscriberId;
  }

  /**
   * Unsubscribe a previously registered callback.
   */
  unsubscribe(eventType: EventType, subscriberId: number | bigint): void {
    this.#ensureOpen();

    const normalizedId = typeof subscriberId === "number" ? BigInt(subscriberId) : subscriberId;

    lib.symbols.event_bus_unsubscribe(
      this.#handle,
      eventType as number,
      normalizedId,
    );

    const sub = this.#subscriptions.get(subscriberId);
    if (sub) {
      sub.ffiCallback.close();
      this.#subscriptions.delete(subscriberId);
    }
  }

  /**
   * Publish an event with a JSON-serializable payload.
   */
  publish(eventType: EventType, payload: unknown): void {
    this.#ensureOpen();

    const jsonBytes = encoder.encode(JSON.stringify(payload));
    const buf = Deno.UnsafePointer.of(jsonBytes);

    const result = lib.symbols.event_bus_publish(
      this.#handle,
      eventType as number,
      buf,
      jsonBytes.byteLength,
    );

    if (result === 0) {
      throw new Error(`Failed to publish event type ${eventType}`);
    }
  }

  /**
   * Destroy the native event bus handle and clean up all subscriptions.
   * Unsubscribes from Rust first, then closes FFI callbacks, then destroys the handle.
   */
  close(): void {
    if (this.#closed) return;
    this.#closed = true;

    // First unsubscribe from Rust side to prevent callbacks from firing
    for (const sub of this.#subscriptions.values()) {
      const normalizedId = typeof sub.subscriberId === "number"
        ? BigInt(sub.subscriberId)
        : sub.subscriberId;
      lib.symbols.event_bus_unsubscribe(
        this.#handle,
        sub.eventType as number,
        normalizedId,
      );
    }

    // Now safe to close FFI callbacks — Rust no longer references them
    for (const sub of this.#subscriptions.values()) {
      sub.ffiCallback.close();
    }
    this.#subscriptions.clear();

    lib.symbols.event_bus_destroy(this.#handle);
  }

  [Symbol.dispose](): void {
    this.close();
  }

  #ensureOpen(): void {
    if (this.#closed) {
      throw new Error("EventBus has been closed");
    }
  }
}
