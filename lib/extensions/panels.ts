// lib/extensions/panels.ts
// Panel registry for extension-contributed custom webview panels.

import type { PanelConfig } from "./types.ts";

/** Internal panel record. */
interface PanelEntry {
  extensionName: string;
  config: PanelConfig;
  visible: boolean;
  /** Callback to send messages to the webview panel. */
  messageHandler?: (type: string, data: unknown) => void;
}

/** Manages extension-registered panels. */
export class PanelRegistry {
  readonly #panels: Map<string, PanelEntry> = new Map();
  /** Callback invoked when panel state changes (for webview bridge). */
  #onChange?: (event: PanelEvent) => void;

  /** Set the change listener (typically the webview bridge). */
  setChangeListener(listener: (event: PanelEvent) => void): void {
    this.#onChange = listener;
  }

  /** Register a panel from an extension. */
  register(extensionName: string, config: PanelConfig): void {
    this.#panels.set(config.id, {
      extensionName,
      config,
      visible: false,
    });
    this.#onChange?.({
      kind: "registered",
      panelId: config.id,
      config,
      extensionName,
    });
  }

  /** Show a panel. */
  show(id: string): void {
    const entry = this.#panels.get(id);
    if (!entry) return;
    entry.visible = true;
    this.#onChange?.({
      kind: "show",
      panelId: id,
      config: entry.config,
      extensionName: entry.extensionName,
    });
  }

  /** Hide a panel. */
  hide(id: string): void {
    const entry = this.#panels.get(id);
    if (!entry) return;
    entry.visible = false;
    this.#onChange?.({
      kind: "hide",
      panelId: id,
      config: entry.config,
      extensionName: entry.extensionName,
    });
  }

  /** Destroy a panel. */
  destroy(id: string): void {
    const entry = this.#panels.get(id);
    if (!entry) return;
    this.#panels.delete(id);
    this.#onChange?.({
      kind: "destroyed",
      panelId: id,
      config: entry.config,
      extensionName: entry.extensionName,
    });
  }

  /** Send a message to a panel's webview. */
  postMessage(id: string, type: string, data: unknown): void {
    const entry = this.#panels.get(id);
    if (!entry) return;
    this.#onChange?.({
      kind: "message",
      panelId: id,
      config: entry.config,
      extensionName: entry.extensionName,
      messageType: type,
      messageData: data,
    });
  }

  /** Remove all panels registered by a given extension. */
  unregisterAll(extensionName: string): void {
    for (const [id, entry] of this.#panels) {
      if (entry.extensionName === extensionName) {
        this.#panels.delete(id);
        this.#onChange?.({
          kind: "destroyed",
          panelId: id,
          config: entry.config,
          extensionName,
        });
      }
    }
  }

  /** Get a panel's config. */
  get(id: string): (PanelConfig & { extensionName: string; visible: boolean }) | undefined {
    const entry = this.#panels.get(id);
    if (!entry) return undefined;
    return { ...entry.config, extensionName: entry.extensionName, visible: entry.visible };
  }

  /** List all registered panels. */
  list(): Array<PanelConfig & { extensionName: string; visible: boolean }> {
    return [...this.#panels.values()].map((e) => ({
      ...e.config,
      extensionName: e.extensionName,
      visible: e.visible,
    }));
  }
}

/** Events emitted by the panel registry. */
export type PanelEvent = {
  kind: "registered" | "show" | "hide" | "destroyed" | "message";
  panelId: string;
  config: PanelConfig;
  extensionName: string;
  messageType?: string;
  messageData?: unknown;
};
