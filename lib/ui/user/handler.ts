/**
 * @marauder/ui/user — Keybinding handler + action dispatch
 *
 * Resolves key sequences to action names and dispatches them to the
 * appropriate managers (PaneManager, TabManager, EventBus).
 */

import type { PaneManager, TabManager } from "../mod.ts";
import type { EventBus } from "@marauder/ffi-event-bus";
import { EventType } from "@marauder/ffi-event-bus";
import type { KeybindingConfig } from "./config.ts";

/** Resolves canonical key sequences to action names. */
export class KeybindingHandler {
  #bindings: Map<string, string>;

  constructor(config: KeybindingConfig) {
    this.#bindings = new Map(Object.entries(config));
  }

  /** Look up the action for a key sequence. Returns null if no binding. */
  resolve(keySeq: string): string | null {
    return this.#bindings.get(keySeq) ?? null;
  }

  /** Hot-reload bindings from a new config (atomic swap). */
  reload(config: KeybindingConfig): void {
    this.#bindings = new Map(Object.entries(config));
  }

  /** Get all registered bindings as a plain object. */
  getBindings(): Record<string, string> {
    return Object.fromEntries(this.#bindings);
  }
}

/** Context passed to action dispatch. */
export interface ActionContext {
  paneId: bigint;
  tabId?: number;
}

/** Dispatches resolved actions to the appropriate managers. */
export class ActionDispatcher {
  readonly #paneManager: PaneManager;
  readonly #tabManager: TabManager;
  readonly #eventBus: EventBus;

  constructor(
    paneManager: PaneManager,
    tabManager: TabManager,
    eventBus: EventBus,
  ) {
    this.#paneManager = paneManager;
    this.#tabManager = tabManager;
    this.#eventBus = eventBus;
  }

  /**
   * Dispatch a named action. Returns true if handled, false for passthrough.
   * Async actions (close-tab, close-pane) are awaited and errors are logged.
   */
  async dispatch(action: string, context: ActionContext): Promise<boolean> {
    switch (action) {
      case "new-tab":
        this.#tabManager.createTab();
        return true;

      case "close-tab": {
        const activeTab = this.#tabManager.getActiveTab();
        if (activeTab) {
          await this.#tabManager.closeTab(activeTab.id);
        }
        return true;
      }

      case "next-tab": {
        const tabs = this.#tabManager.listTabs();
        const active = this.#tabManager.getActiveTab();
        if (active && tabs.length > 1) {
          const idx = tabs.findIndex((t) => t.id === active.id);
          const nextIdx = (idx + 1) % tabs.length;
          this.#tabManager.focusTab(tabs[nextIdx]!.id);
        }
        return true;
      }

      case "prev-tab": {
        const tabs = this.#tabManager.listTabs();
        const active = this.#tabManager.getActiveTab();
        if (active && tabs.length > 1) {
          const idx = tabs.findIndex((t) => t.id === active.id);
          const prevIdx = (idx - 1 + tabs.length) % tabs.length;
          this.#tabManager.focusTab(tabs[prevIdx]!.id);
        }
        return true;
      }

      case "split-pane": {
        this.#paneManager.createPane({ title: "Pane (split)" });
        return true;
      }

      case "close-pane": {
        if (context.paneId !== undefined) {
          await this.#paneManager.closePane(context.paneId);
        }
        return true;
      }

      case "focus-next": {
        const panes = this.#paneManager.listPanes();
        const current = this.#paneManager.getActivePane();
        if (current && panes.length > 1) {
          const idx = panes.findIndex((p) => p.id === current.id);
          const nextIdx = (idx + 1) % panes.length;
          this.#paneManager.focusPane(panes[nextIdx]!.id);
        }
        return true;
      }

      case "focus-prev": {
        const panes = this.#paneManager.listPanes();
        const current = this.#paneManager.getActivePane();
        if (current && panes.length > 1) {
          const idx = panes.findIndex((p) => p.id === current.id);
          const prevIdx = (idx - 1 + panes.length) % panes.length;
          this.#paneManager.focusPane(panes[prevIdx]!.id);
        }
        return true;
      }

      default:
        // Publish unhandled action as an event for extensions
        this.#eventBus.publish(EventType.ExtensionMessage, {
          type: "action",
          action,
          paneId: Number(context.paneId),
        });
        return false;
    }
  }
}
