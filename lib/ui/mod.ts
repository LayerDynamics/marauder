/**
 * @marauder/ui — PaneManager + TabManager
 */

import { type EventBus, EventType } from "@marauder/ffi-event-bus";
import type { BusEvent } from "@marauder/ffi-event-bus";
import {
  type PipelineConfig,
  TerminalPipeline,
} from "@marauder/io/pipeline.ts";
import { decodeBusPayload, Logger } from "@marauder/dev";
import { LayoutEngine } from "./layout.ts";
export { LayoutEngine } from "./layout.ts";
export type { LayoutNode, SerializedLayoutNode, Rect, Direction } from "./layout.ts";
export { saveSession, restoreSession, autoSave, autoRestore } from "./session.ts";
export type { SessionData, SerializedTab } from "./session.ts";

const MAX_PANES = 256;
const MAX_TABS = 64;

export interface Pane {
  readonly id: bigint;
  readonly title: string;
  readonly rows: number;
  readonly cols: number;
  readonly active: boolean;
  readonly pipeline: TerminalPipeline;
}

/** Internal mutable pane state */
interface MutablePane {
  id: bigint;
  title: string;
  rows: number;
  cols: number;
  active: boolean;
  pipeline: TerminalPipeline;
}

export interface Tab {
  readonly id: number;
  readonly title: string;
  readonly paneIds: bigint[];
  readonly activePane: bigint | null;
}

/** Internal mutable tab state */
interface MutableTab {
  id: number;
  title: string;
  paneIds: bigint[];
  activePane: bigint | null;
}

export class PaneManager {
  readonly #panes = new Map<bigint, MutablePane>();
  readonly #eventBus: EventBus;
  readonly #log: Logger;
  #activePane: bigint | null = null;

  constructor(eventBus: EventBus) {
    this.#eventBus = eventBus;
    this.#log = new Logger("pane-manager");
  }

  createPane(config: Partial<PipelineConfig> & { title?: string } = {}): Pane {
    if (this.#panes.size >= MAX_PANES) {
      throw new Error(
        `Maximum pane limit reached (${MAX_PANES}). Close existing panes first.`,
      );
    }

    const pipeline = TerminalPipeline.create(config);
    pipeline.start();

    const id = BigInt(pipeline.paneId);
    const pane: MutablePane = {
      id,
      title: config.title ?? `Pane ${id}`,
      rows: config.rows ?? 24,
      cols: config.cols ?? 80,
      active: this.#panes.size === 0,
      pipeline,
    };

    this.#panes.set(id, pane);
    if (pane.active) this.#activePane = id;

    this.#eventBus.publish(EventType.PaneCreated, {
      paneId: Number(id),
      title: pane.title,
    });
    this.#log.info(`Created pane ${id}: ${pane.title}`);
    return pane as Pane;
  }

  async closePane(id: bigint): Promise<void> {
    const pane = this.#panes.get(id);
    if (!pane) {
      this.#log.warn(`closePane: pane ${id} not found`);
      return;
    }

    // Remove from map first to prevent re-entry
    this.#panes.delete(id);

    // Reassign active pane before destroying pipeline
    if (this.#activePane === id) {
      const first = this.#panes.keys().next();
      this.#activePane = first.done ? null : first.value;
      if (this.#activePane !== null) {
        const next = this.#panes.get(this.#activePane);
        if (next) next.active = true;
      }
    }

    try {
      await pane.pipeline.destroy();
    } catch (err) {
      this.#log.error(`Error destroying pipeline for pane ${id}`, err);
    }

    this.#eventBus.publish(EventType.PaneClosed, { paneId: Number(id) });
    this.#log.info(`Closed pane ${id}`);
  }

  focusPane(id: bigint): void {
    const pane = this.#panes.get(id);
    if (!pane) {
      this.#log.warn(`focusPane: pane ${id} not found`);
      return;
    }

    if (this.#activePane !== null) {
      const prev = this.#panes.get(this.#activePane);
      if (prev) prev.active = false;
    }

    pane.active = true;
    this.#activePane = id;
    this.#eventBus.publish(EventType.PaneFocused, { paneId: Number(id) });
  }

  getPane(id: bigint): Pane | undefined {
    return this.#panes.get(id) as Pane | undefined;
  }

  listPanes(): Pane[] {
    return [...this.#panes.values()] as Pane[];
  }

  getActivePane(): Pane | undefined {
    return this.#activePane !== null
      ? this.#panes.get(this.#activePane) as Pane | undefined
      : undefined;
  }

  resizePane(id: bigint, rows: number, cols: number): void {
    const pane = this.#panes.get(id);
    if (!pane) {
      this.#log.warn(`resizePane: pane ${id} not found`);
      return;
    }
    pane.pipeline.resize(rows, cols);
    pane.rows = rows;
    pane.cols = cols;
  }

  async destroyAll(): Promise<void> {
    const results = await Promise.allSettled(
      [...this.#panes.values()].map((pane) => pane.pipeline.destroy()),
    );
    for (const result of results) {
      if (result.status === "rejected") {
        this.#log.error("Error destroying pane pipeline", result.reason);
      }
    }
    this.#panes.clear();
    this.#activePane = null;
  }

  [Symbol.dispose](): void {
    for (const pane of this.#panes.values()) {
      try {
        pane.pipeline[Symbol.dispose]();
      } catch (err) {
        this.#log.error("Error disposing pane pipeline", err);
      }
    }
    this.#panes.clear();
    this.#activePane = null;
  }
}

export class TabManager {
  readonly #tabs = new Map<number, MutableTab>();
  readonly #paneManager: PaneManager;
  readonly #eventBus: EventBus;
  readonly #log: Logger;
  readonly #layouts = new Map<number, LayoutEngine>();
  #nextId = 1;
  #activeTab: number | null = null;
  #panClosedSubId: bigint | null = null;

  constructor(paneManager: PaneManager, eventBus: EventBus) {
    this.#paneManager = paneManager;
    this.#eventBus = eventBus;
    this.#log = new Logger("tab-manager");

    // Listen for pane closures to update tab state
    this.#panClosedSubId = this.#eventBus.subscribe(
      EventType.PaneClosed,
      (event: BusEvent) => {
        this.#handlePaneClosed(event);
      },
    );
  }

  #handlePaneClosed(event: BusEvent): void {
    try {
      const payload = decodeBusPayload<{ paneId?: number }>(event.payload);
      if (!payload?.paneId) return;
      const closedId = BigInt(payload.paneId);

      for (const tab of this.#tabs.values()) {
        const idx = tab.paneIds.indexOf(closedId);
        if (idx !== -1) {
          tab.paneIds.splice(idx, 1);
          // Reassign active pane if it was the closed one
          if (tab.activePane === closedId) {
            tab.activePane = tab.paneIds.length > 0 ? tab.paneIds[0]! : null;
          }
        }
      }
    } catch {
      // Ignore decode errors
    }
  }

  createTab(title?: string): Tab {
    if (this.#tabs.size >= MAX_TABS) {
      throw new Error(
        `Maximum tab limit reached (${MAX_TABS}). Close existing tabs first.`,
      );
    }

    const id = this.#nextId++;
    const pane = this.#paneManager.createPane({
      title: title ?? `Tab ${id}`,
    });

    const tab: MutableTab = {
      id,
      title: title ?? `Tab ${id}`,
      paneIds: [pane.id],
      activePane: pane.id,
    };

    this.#tabs.set(id, tab);
    this.#layouts.set(id, new LayoutEngine(pane.id));
    if (this.#activeTab === null) this.#activeTab = id;

    this.#eventBus.publish(EventType.TabCreated, {
      tabId: id,
      title: tab.title,
    });
    this.#log.info(`Created tab ${id}: ${tab.title}`);
    return tab as Tab;
  }

  async closeTab(id: number): Promise<void> {
    const tab = this.#tabs.get(id);
    if (!tab) {
      this.#log.warn(`closeTab: tab ${id} not found`);
      return;
    }

    for (const paneId of tab.paneIds) {
      await this.#paneManager.closePane(paneId);
    }
    this.#tabs.delete(id);
    this.#layouts.delete(id);

    if (this.#activeTab === id) {
      const first = this.#tabs.keys().next();
      this.#activeTab = first.done ? null : first.value;
    }

    this.#eventBus.publish(EventType.TabClosed, { tabId: id });
    this.#log.info(`Closed tab ${id}`);
  }

  focusTab(id: number): void {
    const tab = this.#tabs.get(id);
    if (!tab) {
      this.#log.warn(`focusTab: tab ${id} not found`);
      return;
    }

    this.#activeTab = id;
    // Verify the active pane still exists before focusing
    if (tab.activePane !== null) {
      const pane = this.#paneManager.getPane(tab.activePane);
      if (pane) {
        this.#paneManager.focusPane(tab.activePane);
      } else if (tab.paneIds.length > 0) {
        // Active pane was stale, fallback to first available
        tab.activePane = tab.paneIds[0]!;
        this.#paneManager.focusPane(tab.activePane);
      } else {
        tab.activePane = null;
        this.#log.warn(`focusTab: tab ${id} has no valid panes`);
      }
    } else if (tab.paneIds.length > 0) {
      // No active pane set, derive from paneIds
      tab.activePane = tab.paneIds[0]!;
      this.#paneManager.focusPane(tab.activePane);
    } else {
      this.#log.warn(`focusTab: tab ${id} has no panes`);
    }
    this.#eventBus.publish(EventType.TabFocused, { tabId: id });
  }

  getActiveTab(): Tab | undefined {
    return this.#activeTab !== null
      ? this.#tabs.get(this.#activeTab) as Tab | undefined
      : undefined;
  }

  getTab(id: number): Tab | undefined {
    return this.#tabs.get(id) as Tab | undefined;
  }

  listTabs(): Tab[] {
    return [...this.#tabs.values()] as Tab[];
  }

  /** Rename a tab. Publishes a TabRenamed event. */
  renameTab(id: number, title: string): void {
    const tab = this.#tabs.get(id);
    if (!tab) {
      this.#log.warn(`renameTab: tab ${id} not found`);
      return;
    }
    tab.title = title;
    this.#eventBus.publish(EventType.ExtensionMessage, {
      type: "tab-renamed",
      tabId: id,
      title,
    });
    this.#log.info(`Renamed tab ${id}: ${title}`);
  }

  /**
   * Split the active pane in a tab, creating a new pane alongside it.
   * Returns the new pane, or null if the tab or layout was not found.
   */
  splitPane(
    tabId: number,
    direction: "horizontal" | "vertical",
  ): Pane | null {
    const tab = this.#tabs.get(tabId);
    const layout = this.#layouts.get(tabId);
    if (!tab || !layout) {
      this.#log.warn(`splitPane: tab ${tabId} not found`);
      return null;
    }

    const activePaneId = tab.activePane;
    if (activePaneId === null) {
      this.#log.warn(`splitPane: tab ${tabId} has no active pane`);
      return null;
    }

    const newPane = this.#paneManager.createPane({ title: `Pane (split)` });
    tab.paneIds.push(newPane.id);
    layout.split(activePaneId, direction, newPane.id);

    this.#log.info(
      `Split pane ${activePaneId} ${direction} in tab ${tabId}, new pane ${newPane.id}`,
    );
    return newPane as Pane;
  }

  /** Get the layout engine for a tab. */
  getLayout(tabId: number): LayoutEngine | undefined {
    return this.#layouts.get(tabId);
  }

  /** Get the layouts map (for session save/restore). */
  getLayouts(): Map<number, LayoutEngine> {
    return this.#layouts;
  }

  [Symbol.dispose](): void {
    if (this.#panClosedSubId !== null) {
      try {
        this.#eventBus.unsubscribe(EventType.PaneClosed, this.#panClosedSubId);
      } catch {
        // Bus may already be closed
      }
      this.#panClosedSubId = null;
    }
    this.#paneManager[Symbol.dispose]();
  }
}
