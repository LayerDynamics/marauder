/**
 * Tab bar component — vanilla TS, manages #tab-bar DOM element.
 */

export interface Tab {
  id: number;
  title: string;
}

export class TabBar {
  private container: HTMLElement;
  private tabsEl: HTMLElement;
  private tabs: Map<number, Tab> = new Map();
  private activeTabId: number | null = null;

  constructor(container: HTMLElement) {
    this.container = container;
    this.container.setAttribute("data-tauri-drag-region", "");

    this.tabsEl = document.createElement("div");
    this.tabsEl.className = "tab-list";
    this.container.appendChild(this.tabsEl);

    const newBtn = document.createElement("button");
    newBtn.className = "tab-new-btn";
    newBtn.textContent = "+";
    newBtn.title = "New tab";
    newBtn.addEventListener("click", () => {
      this.container.dispatchEvent(
        new CustomEvent("tab-new", { bubbles: true })
      );
    });
    this.container.appendChild(newBtn);
  }

  addTab(id: number, title: string): void {
    const tab: Tab = { id, title };
    this.tabs.set(id, tab);
    this.render();
    this.setActiveTab(id);
  }

  removeTab(id: number): void {
    this.tabs.delete(id);
    if (this.activeTabId === id) {
      const remaining = Array.from(this.tabs.keys());
      this.activeTabId = remaining.length > 0 ? remaining[remaining.length - 1] : null;
    }
    this.render();
  }

  setActiveTab(id: number): void {
    this.activeTabId = id;
    this.render();
  }

  renameTab(id: number, title: string): void {
    const tab = this.tabs.get(id);
    if (tab) {
      tab.title = title;
      this.render();
    }
  }

  private render(): void {
    this.tabsEl.innerHTML = "";

    for (const [id, tab] of this.tabs) {
      const tabEl = document.createElement("div");
      tabEl.className = "tab" + (id === this.activeTabId ? " tab-active" : "");

      const titleEl = document.createElement("span");
      titleEl.className = "tab-title";
      titleEl.textContent = tab.title;
      titleEl.addEventListener("click", () => {
        this.container.dispatchEvent(
          new CustomEvent("tab-select", { bubbles: true, detail: { id } })
        );
      });
      tabEl.appendChild(titleEl);

      const closeBtn = document.createElement("button");
      closeBtn.className = "tab-close-btn";
      closeBtn.textContent = "\u00d7";
      closeBtn.title = "Close tab";
      closeBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        this.container.dispatchEvent(
          new CustomEvent("tab-close", { bubbles: true, detail: { id } })
        );
      });
      tabEl.appendChild(closeBtn);

      this.tabsEl.appendChild(tabEl);
    }
  }
}
