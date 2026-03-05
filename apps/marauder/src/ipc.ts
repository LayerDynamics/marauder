/**
 * IPC wrappers for Tauri commands — event bus and PTY management.
 */

import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  BusEvent,
  CreatePtyRequest,
  PtyInfo,
  EventTypeValue,
  CellInfo,
  CursorPosition,
  GridDimensions,
  ScreenSnapshot,
} from "./types";

/** Client for the event bus bridge. */
export class EventBusClient {
  private subscriberIds: Map<EventTypeValue, number[]> = new Map();
  /** Map subscriber ID → Channel so channels are pruned on unsubscribe. */
  private channelBySubscriber: Map<number, Channel<string>> = new Map();
  /** Retained reference to the bridge channel to prevent GC. */
  private bridgeChannel?: Channel<string>;

  /**
   * Start the server-push event bridge (call once at startup).
   * The channel is retained on the instance for the app lifetime.
   */
  async startBridge(callback: (event: BusEvent) => void): Promise<void> {
    const channel = new Channel<string>();
    channel.onmessage = (json: string) => {
      try {
        const event: BusEvent = JSON.parse(json);
        callback(event);
      } catch (e) {
        console.error("EventBusClient: failed to parse bridge event", e);
      }
    };
    this.bridgeChannel = channel;
    await invoke("event_bus_start_bridge", { channel });
  }

  /**
   * Subscribe to event types via a Tauri Channel.
   * The callback receives parsed BusEvent objects.
   */
  async subscribe(
    eventTypes: EventTypeValue[],
    callback: (event: BusEvent) => void
  ): Promise<number[]> {
    const channel = new Channel<string>();
    channel.onmessage = (json: string) => {
      try {
        const event: BusEvent = JSON.parse(json);
        callback(event);
      } catch (e) {
        console.error("EventBusClient: failed to parse event", e);
      }
    };

    const ids: number[] = await invoke("event_bus_subscribe_channel", {
      event_types: eventTypes,
      channel,
    });

    for (let i = 0; i < eventTypes.length; i++) {
      const et = eventTypes[i];
      const existing = this.subscriberIds.get(et) ?? [];
      existing.push(ids[i]);
      this.subscriberIds.set(et, existing);
      this.channelBySubscriber.set(ids[i], channel);
    }

    return ids;
  }

  /** Unsubscribe a specific subscriber from an event type. */
  async unsubscribe(
    eventType: EventTypeValue,
    subscriberId: number
  ): Promise<void> {
    await invoke("event_bus_unsubscribe_channel", {
      event_type: eventType,
      subscriber_id: subscriberId,
    });
    this.channelBySubscriber.delete(subscriberId);
    const existing = this.subscriberIds.get(eventType);
    if (existing) {
      const filtered = existing.filter((id) => id !== subscriberId);
      if (filtered.length > 0) {
        this.subscriberIds.set(eventType, filtered);
      } else {
        this.subscriberIds.delete(eventType);
      }
    }
  }

  /** Emit an event from the webview (subject to allowlist). */
  async emit(eventType: EventTypeValue, payload: string): Promise<void> {
    await invoke("event_bus_emit", { event_type: eventType, payload });
  }

  /** Clean up all active subscriptions. */
  async destroy(): Promise<void> {
    for (const [eventType, ids] of this.subscriberIds) {
      for (const id of ids) {
        try {
          await this.unsubscribe(eventType, id);
        } catch {
          // Best effort cleanup
        }
      }
    }
    this.subscriberIds.clear();
    this.channelBySubscriber.clear();
    this.bridgeChannel = undefined;
  }

  [Symbol.dispose](): void {
    this.destroy().catch(() => {});
  }
}

/** Client for PTY management commands. */
export class PtyClient {
  async create(config: CreatePtyRequest): Promise<PtyInfo> {
    return invoke("pty_cmd_create", { request: config });
  }

  async write(paneId: number, data: number[]): Promise<void> {
    await invoke("pty_cmd_write", { pane_id: paneId, data });
  }

  async read(paneId: number, maxBytes: number = 65536): Promise<number[]> {
    return invoke("pty_cmd_read", { pane_id: paneId, max_bytes: maxBytes });
  }

  async resize(paneId: number, rows: number, cols: number): Promise<void> {
    await invoke("pty_cmd_resize", { pane_id: paneId, rows, cols });
  }

  async close(paneId: number): Promise<void> {
    await invoke("pty_cmd_close", { pane_id: paneId });
  }

  async getPid(paneId: number): Promise<number | null> {
    return invoke("pty_cmd_get_pid", { pane_id: paneId });
  }

  async wait(paneId: number): Promise<number | null> {
    return invoke("pty_cmd_wait", { pane_id: paneId });
  }

  async list(): Promise<PtyInfo[]> {
    return invoke("pty_cmd_list", {});
  }

  [Symbol.dispose](): void {
    // No cleanup needed — PTY lifecycle managed by runtime
  }
}

/** Client for config store commands. */
export class ConfigClient {
  async get(key: string): Promise<unknown | null> {
    return invoke("config_cmd_get", { key });
  }

  async set(key: string, value: unknown): Promise<void> {
    await invoke("config_cmd_set", { key, value });
  }

  async keys(): Promise<string[]> {
    return invoke("config_cmd_keys", {});
  }

  async save(path: string): Promise<void> {
    await invoke("config_cmd_save", { path });
  }

  async reload(): Promise<void> {
    await invoke("config_cmd_reload", {});
  }

  [Symbol.dispose](): void {
    // No cleanup needed — config store is app-lifetime
  }
}

/** Client for grid commands. */
export class GridClient {
  async getCursor(paneId: number): Promise<CursorPosition> {
    return invoke("grid_cmd_get_cursor", { pane_id: paneId });
  }

  async getCell(paneId: number, row: number, col: number): Promise<CellInfo> {
    return invoke("grid_cmd_get_cell", { pane_id: paneId, row, col });
  }

  async getSelectionText(paneId: number): Promise<string | null> {
    return invoke("grid_cmd_get_selection_text", { pane_id: paneId });
  }

  async setSelection(
    paneId: number,
    startRow: number,
    startCol: number,
    endRow: number,
    endCol: number
  ): Promise<void> {
    await invoke("grid_cmd_set_selection", {
      pane_id: paneId,
      start_row: startRow,
      start_col: startCol,
      end_row: endRow,
      end_col: endCol,
    });
  }

  async clearSelection(paneId: number): Promise<void> {
    await invoke("grid_cmd_clear_selection", { pane_id: paneId });
  }

  async scrollViewport(paneId: number, offset: number): Promise<void> {
    await invoke("grid_cmd_scroll_viewport", { pane_id: paneId, offset });
  }

  async scrollViewportBy(paneId: number, delta: number): Promise<void> {
    await invoke("grid_cmd_scroll_viewport_by", { pane_id: paneId, delta });
  }

  async getScreenSnapshot(paneId: number): Promise<ScreenSnapshot> {
    return invoke("grid_cmd_get_screen_snapshot", { pane_id: paneId });
  }

  async getDimensions(paneId: number): Promise<GridDimensions> {
    return invoke("grid_cmd_get_dimensions", { pane_id: paneId });
  }

  [Symbol.dispose](): void {
    // No cleanup needed — grid lifecycle managed by runtime
  }
}

/** Client for evaluating JS and calling ops in the embedded deno_core JsRuntime. */
export class DenoClient {
  /**
   * Evaluate arbitrary JS code in the JsRuntime and return the result.
   * Only available in debug builds — returns an error in release builds.
   */
  async eval(code: string): Promise<string> {
    return invoke("deno_eval", { code });
  }

  /** Call a registered #[op2] by name with positional args. */
  async callOp(opName: string, args: unknown[] = []): Promise<unknown> {
    // Validate all args are JSON-serializable before sending
    const sanitized = args.map((a, i) => {
      const s = JSON.stringify(a);
      if (s === undefined) {
        throw new Error(
          `Argument at index ${i} is not JSON-serializable (undefined, function, or symbol)`,
        );
      }
      return JSON.parse(s);
    });

    const result: string = await invoke("deno_call_op", {
      op_name: opName,
      args: sanitized,
    });
    try {
      return JSON.parse(result);
    } catch {
      return result;
    }
  }

  [Symbol.dispose](): void {
    // No cleanup needed — Deno runtime is app-lifetime
  }
}

/** Client for renderer commands. */
export class RendererClient {
  async getCellSize(): Promise<[number, number]> {
    return invoke("renderer_get_cell_size", {});
  }

  async resize(width: number, height: number, scaleFactor: number): Promise<void> {
    await invoke("renderer_resize", { width, height, scale_factor: scaleFactor });
  }

  [Symbol.dispose](): void {
    // No cleanup needed — renderer is app-lifetime
  }
}

/** Client for runtime commands. */
export class RuntimeClient {
  async state(): Promise<string> {
    return invoke("runtime_cmd_state", {});
  }

  async paneIds(): Promise<number[]> {
    return invoke("runtime_cmd_pane_ids", {});
  }

  async createPane(): Promise<number> {
    return invoke("runtime_cmd_create_pane", {});
  }

  async closePane(paneId: number): Promise<void> {
    await invoke("runtime_cmd_close_pane", { pane_id: paneId });
  }

  [Symbol.dispose](): void {
    // No cleanup needed — runtime is app-lifetime
  }
}
