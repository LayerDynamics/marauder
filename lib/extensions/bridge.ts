// lib/extensions/bridge.ts
// Tauri Channel-based bridge for extension <-> webview communication.

import type { PanelEvent, PanelRegistry } from "./panels.ts";

/** Message sent from extension runtime to the webview. */
export interface ExtensionBridgeMessage {
  kind: "extension-message" | "panel-event";
  extensionName?: string;
  type: string;
  data: unknown;
}

/**
 * Server-side bridge (Deno/runtime side) that collects messages from
 * extensions and forwards them to the webview via Tauri Channel.
 */
export class ExtensionBridgeServer {
  /** Queue of messages waiting to be sent. */
  readonly #queue: ExtensionBridgeMessage[] = [];
  /** Callback to send a message to the webview (set by Tauri integration). */
  #sender: ((msg: ExtensionBridgeMessage) => void) | null = null;

  /** Set the sender function (called by Tauri when a Channel is established). */
  setSender(sender: (msg: ExtensionBridgeMessage) => void): void {
    this.#sender = sender;
    // Flush any queued messages
    while (this.#queue.length > 0) {
      const msg = this.#queue.shift()!;
      sender(msg);
    }
  }

  /** Post a message from an extension to the webview. */
  postMessage(extensionName: string, type: string, data: unknown): void {
    const msg: ExtensionBridgeMessage = {
      kind: "extension-message",
      extensionName,
      type,
      data,
    };
    if (this.#sender) {
      this.#sender(msg);
    } else {
      this.#queue.push(msg);
    }
  }

  /**
   * Wire up the panel registry to forward panel events to the webview.
   * Call once during initialization.
   */
  wirePanelRegistry(panelRegistry: PanelRegistry): void {
    panelRegistry.setChangeListener((event: PanelEvent) => {
      const msg: ExtensionBridgeMessage = {
        kind: "panel-event",
        extensionName: event.extensionName,
        type: event.kind,
        data: {
          panelId: event.panelId,
          config: event.config,
          messageType: event.messageType,
          messageData: event.messageData,
        },
      };
      if (this.#sender) {
        this.#sender(msg);
      } else {
        this.#queue.push(msg);
      }
    });
  }
}
