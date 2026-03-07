/**
 * @marauder/ui/session — Session save/restore for tabs, panes, and layouts.
 *
 * Serializes the current window state (tabs + layout trees) to JSON and
 * restores it on next launch.
 */

import type { TabManager } from "./mod.ts";
import { LayoutEngine, type SerializedLayoutNode } from "./layout.ts";

/** Serialized tab state. */
export interface SerializedTab {
  id: number;
  title: string;
  layout: SerializedLayoutNode;
  activePaneId: string | null;
}

/** Full session state for save/restore. */
export interface SessionData {
  version: 1;
  tabs: SerializedTab[];
  activeTabId: number | null;
}

/**
 * Save the current session state from a TabManager.
 *
 * @param tabManager - The tab manager to serialize
 * @param layouts - Map of tab ID → LayoutEngine for each tab
 * @returns SessionData ready for JSON serialization
 */
export function saveSession(
  tabManager: TabManager,
  layouts: Map<number, LayoutEngine>,
): SessionData {
  const tabs = tabManager.listTabs();
  const activeTab = tabManager.getActiveTab();

  const serializedTabs: SerializedTab[] = tabs.map((tab) => {
    const layout = layouts.get(tab.id);
    return {
      id: tab.id,
      title: tab.title,
      layout: layout?.serialize() ?? { type: "leaf", paneId: "0" },
      activePaneId: tab.activePane?.toString() ?? null,
    };
  });

  return {
    version: 1,
    tabs: serializedTabs,
    activeTabId: activeTab?.id ?? null,
  };
}

/**
 * Restore a session from saved data.
 *
 * This recreates tabs and their layout trees. The caller is responsible for
 * creating actual PTY sessions for each pane in the restored layout.
 *
 * @param data - Previously saved SessionData
 * @param tabManager - The tab manager to populate
 * @param layouts - Map to populate with tab ID → LayoutEngine
 * @returns Map of old pane ID strings to the tabs they belong to (for PTY recreation)
 */
export function restoreSession(
  data: SessionData,
  tabManager: TabManager,
  layouts: Map<number, LayoutEngine>,
): Map<string, number> {
  const paneToTab = new Map<string, number>();

  for (const tabData of data.tabs) {
    const tab = tabManager.createTab(tabData.title);
    const layout = new LayoutEngine(tab.paneIds[0] ?? 0n);

    if (tabData.layout) {
      const restoredNode = LayoutEngine.deserialize(tabData.layout);
      layout.setLayout(restoredNode);

      // Collect all pane IDs from the layout for PTY recreation
      const paneIds = layout.getAllPaneIds();
      for (const paneId of paneIds) {
        paneToTab.set(paneId.toString(), tab.id);
      }
    }

    layouts.set(tab.id, layout);
  }

  // Focus the previously active tab
  if (data.activeTabId !== null) {
    const restoredTab = tabManager.listTabs().find(
      (t) => t.title === data.tabs.find((d) => d.id === data.activeTabId)?.title,
    );
    if (restoredTab) {
      tabManager.focusTab(restoredTab.id);
    }
  }

  return paneToTab;
}

/** Default path for auto-save session data. */
export function autoSavePath(): string {
  const home = typeof Deno !== "undefined"
    ? Deno.env.get("HOME") ?? Deno.env.get("USERPROFILE") ?? "."
    : ".";
  return `${home}/.config/marauder/sessions/last.json`;
}

/**
 * Auto-save session to disk.
 */
export async function autoSave(
  tabManager: TabManager,
  layouts: Map<number, LayoutEngine>,
): Promise<void> {
  const data = saveSession(tabManager, layouts);
  const json = JSON.stringify(data, null, 2);
  const path = autoSavePath();

  // Ensure directory exists
  const dir = path.substring(0, path.lastIndexOf("/"));
  try {
    await Deno.mkdir(dir, { recursive: true });
  } catch {
    // Directory may already exist
  }

  await Deno.writeTextFile(path, json);
}

/**
 * Auto-restore session from disk. Returns null if no saved session exists.
 */
export async function autoRestore(): Promise<SessionData | null> {
  const path = autoSavePath();
  try {
    const json = await Deno.readTextFile(path);
    const data = JSON.parse(json) as SessionData;
    if (data.version !== 1) return null;
    return data;
  } catch {
    return null;
  }
}
