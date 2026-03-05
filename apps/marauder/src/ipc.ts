/**
 * IPC wrappers for Tauri commands — event bus and PTY management.
 */

import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  BusEvent,
  CreatePtyRequest,
  PtyInfo,
  EventTypeValue,
} from "./types";

/** Client for the event bus bridge. */
export class EventBusClient {
  private subscriberIds: Map<EventTypeValue, number[]> = new Map();
  private channels: Channel<string>[] = [];

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
    this.channels.push(channel);

    const ids: number[] = await invoke("event_bus_subscribe_channel", {
      eventTypes,
      channel,
    });

    for (let i = 0; i < eventTypes.length; i++) {
      const et = eventTypes[i];
      const existing = this.subscriberIds.get(et) ?? [];
      existing.push(ids[i]);
      this.subscriberIds.set(et, existing);
    }

    return ids;
  }

  /** Unsubscribe a specific subscriber from an event type. */
  async unsubscribe(
    eventType: EventTypeValue,
    subscriberId: number
  ): Promise<void> {
    await invoke("event_bus_unsubscribe_channel", {
      eventType,
      subscriberId,
    });
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
    await invoke("event_bus_emit", { eventType, payload });
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
    this.channels = [];
  }
}

/** Client for PTY management commands. */
export class PtyClient {
  async create(config: CreatePtyRequest): Promise<PtyInfo> {
    return invoke("pty_cmd_create", { request: config });
  }

  async write(paneId: number, data: number[]): Promise<void> {
    await invoke("pty_cmd_write", { paneId, data });
  }

  async read(paneId: number): Promise<number[]> {
    return invoke("pty_cmd_read", { paneId });
  }

  async resize(paneId: number, rows: number, cols: number): Promise<void> {
    await invoke("pty_cmd_resize", { paneId, rows, cols });
  }

  async close(paneId: number): Promise<void> {
    await invoke("pty_cmd_close", { paneId });
  }

  async getPid(paneId: number): Promise<number | null> {
    return invoke("pty_cmd_get_pid", { paneId });
  }

  async wait(paneId: number): Promise<number | null> {
    return invoke("pty_cmd_wait", { paneId });
  }

  async list(): Promise<PtyInfo[]> {
    return invoke("pty_cmd_list", {});
  }
}
