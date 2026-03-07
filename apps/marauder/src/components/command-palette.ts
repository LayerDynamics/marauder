/**
 * Command palette component — floating overlay with fuzzy-filtered command list.
 * Toggled by Ctrl+Shift+P (dispatched as "command-palette" action).
 */

export interface PaletteCommand {
  id: string;
  label: string;
  category?: string;
}

export type PaletteCallback = (commandId: string) => void;

const MAX_VISIBLE_ITEMS = 100;
const FILTER_DEBOUNCE_MS = 16; // ~1 frame

export class CommandPalette {
  private overlay: HTMLElement;
  private input: HTMLInputElement;
  private list: HTMLElement;
  private commands: PaletteCommand[] = [];
  private filtered: PaletteCommand[] = [];
  private selectedIndex = 0;
  private onSelect: PaletteCallback | null = null;
  private visible = false;
  private filterTimer: ReturnType<typeof setTimeout> | null = null;
  private renderedItems: HTMLElement[] = [];

  constructor(parent: HTMLElement) {
    this.overlay = document.createElement("div");
    this.overlay.className = "command-palette-overlay";
    this.overlay.style.cssText =
      "display:none;position:fixed;top:0;left:0;right:0;bottom:0;z-index:9999;" +
      "background:rgba(0,0,0,0.4);align-items:flex-start;justify-content:center;padding-top:80px;";

    const panel = document.createElement("div");
    panel.className = "command-palette";
    panel.style.cssText =
      "width:500px;max-height:400px;background:var(--bg-primary,#1e1e2e);" +
      "border:1px solid var(--border-color,#45475a);border-radius:8px;" +
      "box-shadow:0 8px 32px rgba(0,0,0,0.5);overflow:hidden;display:flex;flex-direction:column;";

    this.input = document.createElement("input");
    this.input.type = "text";
    this.input.placeholder = "Type a command...";
    this.input.style.cssText =
      "width:100%;padding:12px 16px;border:none;border-bottom:1px solid var(--border-color,#45475a);" +
      "background:transparent;color:var(--text-primary,#cdd6f4);font-size:14px;outline:none;box-sizing:border-box;";

    this.list = document.createElement("div");
    this.list.className = "command-palette-list";
    this.list.style.cssText = "overflow-y:auto;flex:1;";

    panel.appendChild(this.input);
    panel.appendChild(this.list);
    this.overlay.appendChild(panel);
    parent.appendChild(this.overlay);

    this.input.addEventListener("input", () => this.debouncedFilter());
    this.input.addEventListener("keydown", (e) => this.handleKey(e));
    this.overlay.addEventListener("click", (e) => {
      if (e.target === this.overlay) this.hide();
    });
  }

  /** Register commands and a selection callback. */
  setCommands(commands: PaletteCommand[], onSelect: PaletteCallback): void {
    this.commands = commands;
    this.onSelect = onSelect;
  }

  /** Add commands from extensions at runtime. */
  addCommands(commands: PaletteCommand[]): void {
    for (const cmd of commands) {
      if (!this.commands.some((c) => c.id === cmd.id)) {
        this.commands.push(cmd);
      }
    }
  }

  /** Remove commands by ID (e.g., when an extension is unloaded). */
  removeCommands(commandIds: string[]): void {
    const idSet = new Set(commandIds);
    this.commands = this.commands.filter((c) => !idSet.has(c.id));
    if (this.visible) this.filter();
  }

  show(): void {
    if (this.visible) return;
    this.visible = true;
    this.overlay.style.display = "flex";
    this.input.value = "";
    this.selectedIndex = 0;
    this.filter();
    this.input.focus();
  }

  hide(): void {
    if (!this.visible) return;
    this.visible = false;
    this.overlay.style.display = "none";
  }

  toggle(): void {
    if (this.visible) this.hide();
    else this.show();
  }

  isVisible(): boolean {
    return this.visible;
  }

  private debouncedFilter(): void {
    if (this.filterTimer !== null) clearTimeout(this.filterTimer);
    this.filterTimer = setTimeout(() => this.filter(), FILTER_DEBOUNCE_MS);
  }

  private filter(): void {
    const query = this.input.value.toLowerCase();
    this.filtered = query
      ? this.commands.filter((c) => fuzzyMatch(query, c.label.toLowerCase()))
      : [...this.commands];
    this.selectedIndex = Math.min(this.selectedIndex, Math.max(0, this.filtered.length - 1));
    this.render();
  }

  private render(): void {
    const visibleCount = Math.min(this.filtered.length, MAX_VISIBLE_ITEMS);

    // Reuse existing DOM nodes where possible, add/remove as needed
    while (this.renderedItems.length > visibleCount) {
      const removed = this.renderedItems.pop()!;
      removed.remove();
    }

    for (let i = 0; i < visibleCount; i++) {
      const cmd = this.filtered[i]!;
      let item = this.renderedItems[i];

      if (!item) {
        item = document.createElement("div");
        item.style.cssText = "padding:8px 16px;cursor:pointer;color:var(--text-primary,#cdd6f4);";
        const idx = i;
        item.addEventListener("click", () => this.select(idx));
        item.addEventListener("mouseenter", () => {
          this.selectedIndex = idx;
          this.updateSelection();
        });
        this.renderedItems.push(item);
        this.list.appendChild(item);
      }

      item.textContent = cmd.label;
      item.className = "command-palette-item" + (i === this.selectedIndex ? " selected" : "");
      item.style.background = i === this.selectedIndex ? "var(--bg-selected,#45475a)" : "";

      if (cmd.category) {
        const cat = document.createElement("span");
        cat.style.cssText = "float:right;opacity:0.5;font-size:12px;";
        cat.textContent = cmd.category;
        item.appendChild(cat);
      }
    }
  }

  private updateSelection(): void {
    for (let i = 0; i < this.renderedItems.length; i++) {
      const item = this.renderedItems[i]!;
      const selected = i === this.selectedIndex;
      item.className = "command-palette-item" + (selected ? " selected" : "");
      item.style.background = selected ? "var(--bg-selected,#45475a)" : "";
    }
  }

  private handleKey(e: KeyboardEvent): void {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        this.selectedIndex = Math.min(this.selectedIndex + 1, this.filtered.length - 1);
        this.updateSelection();
        this.scrollToSelected();
        break;
      case "ArrowUp":
        e.preventDefault();
        this.selectedIndex = Math.max(this.selectedIndex - 1, 0);
        this.updateSelection();
        this.scrollToSelected();
        break;
      case "Enter":
        e.preventDefault();
        this.select(this.selectedIndex);
        break;
      case "Escape":
        e.preventDefault();
        this.hide();
        break;
    }
  }

  private select(index: number): void {
    const cmd = this.filtered[index];
    if (cmd && this.onSelect) {
      this.hide();
      this.onSelect(cmd.id);
    }
  }

  private scrollToSelected(): void {
    const items = this.list.children;
    if (items[this.selectedIndex]) {
      (items[this.selectedIndex] as HTMLElement).scrollIntoView({ block: "nearest" });
    }
  }
}

/** Simple fuzzy match — all query chars must appear in order in the target. */
function fuzzyMatch(query: string, target: string): boolean {
  let qi = 0;
  for (let ti = 0; ti < target.length && qi < query.length; ti++) {
    if (target[ti] === query[qi]) qi++;
  }
  return qi === query.length;
}
