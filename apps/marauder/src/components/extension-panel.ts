// apps/marauder/src/components/extension-panel.ts
// Webview component for rendering extension-contributed panels.

/** Panel state tracked on the frontend. */
interface PanelState {
  id: string;
  title: string;
  html: string;
  extensionName: string;
  visible: boolean;
  icon?: string;
  position: "sidebar" | "bottom" | "overlay";
  container?: HTMLElement;
}

/** Manages extension panel rendering in the webview. */
export class ExtensionPanelManager {
  readonly #panels: Map<string, PanelState> = new Map();
  readonly #root: HTMLElement;

  constructor(root: HTMLElement) {
    this.#root = root;
  }

  /** Register a new panel (called when panel-event "registered" arrives). */
  registerPanel(config: {
    panelId: string;
    title: string;
    html: string;
    extensionName: string;
    icon?: string;
    position?: string;
  }): void {
    const state: PanelState = {
      id: config.panelId,
      title: config.title,
      html: config.html,
      extensionName: config.extensionName,
      visible: false,
      icon: config.icon,
      position: (config.position as PanelState["position"]) ?? "sidebar",
    };
    this.#panels.set(config.panelId, state);
  }

  /** Show a panel — creates its DOM container if needed. */
  showPanel(panelId: string): void {
    const state = this.#panels.get(panelId);
    if (!state) return;

    state.visible = true;

    if (!state.container) {
      const container = document.createElement("div");
      container.className = `extension-panel extension-panel--${state.position}`;
      container.dataset.panelId = panelId;
      container.dataset.extension = state.extensionName;

      // Header
      const header = document.createElement("div");
      header.className = "extension-panel__header";
      header.textContent = state.title;
      container.appendChild(header);

      // Body (sandboxed via innerHTML — extensions are trusted first-party)
      const body = document.createElement("div");
      body.className = "extension-panel__body";
      body.innerHTML = state.html;
      container.appendChild(body);

      state.container = container;
      this.#root.appendChild(container);
    }

    state.container.style.display = "";
  }

  /** Hide a panel without destroying it. */
  hidePanel(panelId: string): void {
    const state = this.#panels.get(panelId);
    if (!state) return;
    state.visible = false;
    if (state.container) {
      state.container.style.display = "none";
    }
  }

  /** Destroy a panel and remove its DOM. */
  destroyPanel(panelId: string): void {
    const state = this.#panels.get(panelId);
    if (!state) return;
    if (state.container) {
      state.container.remove();
    }
    this.#panels.delete(panelId);
  }

  /** Handle a message directed at a panel. */
  handleMessage(panelId: string, type: string, data: unknown): void {
    const state = this.#panels.get(panelId);
    if (!state?.container) return;
    // Dispatch a custom event on the panel's container so panel HTML can listen
    state.container.dispatchEvent(
      new CustomEvent("extension-message", {
        detail: { type, data },
        bubbles: false,
      }),
    );
  }

  /** Process a panel event from the bridge. */
  handlePanelEvent(event: {
    kind: string;
    panelId: string;
    config?: { title?: string; html?: string; icon?: string; position?: string };
    extensionName?: string;
    messageType?: string;
    messageData?: unknown;
  }): void {
    switch (event.kind) {
      case "registered":
        if (event.config) {
          this.registerPanel({
            panelId: event.panelId,
            title: event.config.title ?? "",
            html: event.config.html ?? "",
            extensionName: event.extensionName ?? "",
            icon: event.config.icon,
            position: event.config.position,
          });
        }
        break;
      case "show":
        this.showPanel(event.panelId);
        break;
      case "hide":
        this.hidePanel(event.panelId);
        break;
      case "destroyed":
        this.destroyPanel(event.panelId);
        break;
      case "message":
        if (event.messageType) {
          this.handleMessage(event.panelId, event.messageType, event.messageData);
        }
        break;
    }
  }
}
