/**
 * Status bar component — vanilla TS, manages #status-bar DOM element.
 *
 * Supports built-in segments (cwd, command, dimensions) and extension-registered
 * segments with position and priority control.
 */

/** A registered status bar segment. */
interface Segment {
  id: string;
  position: "left" | "center" | "right";
  priority: number;
  element: HTMLElement;
}

export class StatusBar {
  private leftEl: HTMLElement;
  private centerEl: HTMLElement;
  private rightEl: HTMLElement;
  private segments: Map<string, Segment> = new Map();

  // Built-in segment IDs
  private static readonly CWD_ID = "__builtin_cwd";
  private static readonly CMD_ID = "__builtin_cmd";
  private static readonly DIM_ID = "__builtin_dim";

  constructor(container: HTMLElement) {
    this.leftEl = document.createElement("div");
    this.leftEl.className = "status-left";
    container.appendChild(this.leftEl);

    this.centerEl = document.createElement("div");
    this.centerEl.className = "status-center";
    container.appendChild(this.centerEl);

    this.rightEl = document.createElement("div");
    this.rightEl.className = "status-right";
    container.appendChild(this.rightEl);

    // Register built-in segments
    this.registerSegment(StatusBar.CWD_ID, "left", 0);
    this.registerSegment(StatusBar.CMD_ID, "center", 0);
    this.registerSegment(StatusBar.DIM_ID, "right", 0);
  }

  setCwd(path: string): void {
    this.updateSegment(StatusBar.CWD_ID, path);
  }

  setCommand(cmd: string): void {
    this.updateSegment(StatusBar.CMD_ID, cmd);
  }

  clearCommand(): void {
    this.updateSegment(StatusBar.CMD_ID, "");
  }

  setDimensions(rows: number, cols: number): void {
    this.updateSegment(StatusBar.DIM_ID, `${cols}\u00d7${rows}`);
  }

  /**
   * Register an extension segment slot.
   * Segments are sorted by priority within each position (lower = leftmost).
   */
  registerSegment(
    id: string,
    position: "left" | "center" | "right",
    priority: number,
  ): void {
    // Remove existing segment with same ID if any
    if (this.segments.has(id)) {
      this.removeSegment(id);
    }

    const element = document.createElement("span");
    element.className = `status-segment status-segment-${id}`;
    element.dataset.segmentId = id;
    element.dataset.priority = priority.toString();

    const segment: Segment = { id, position, priority, element };
    this.segments.set(id, segment);

    this.#insertSorted(segment);
  }

  /**
   * Update the text content of a registered segment.
   */
  updateSegment(id: string, text: string): void {
    const segment = this.segments.get(id);
    if (segment) {
      segment.element.textContent = text;
    }
  }

  /**
   * Remove a registered segment.
   */
  removeSegment(id: string): void {
    const segment = this.segments.get(id);
    if (segment) {
      segment.element.remove();
      this.segments.delete(id);
    }
  }

  /** Insert a segment element in priority order within its position container. */
  #insertSorted(segment: Segment): void {
    const container = this.#containerFor(segment.position);

    // Find insertion point: before the first element with higher priority
    const children = container.querySelectorAll<HTMLElement>(".status-segment");
    let insertBefore: HTMLElement | null = null;
    for (const child of children) {
      const childPriority = parseInt(child.dataset.priority ?? "0", 10);
      if (childPriority > segment.priority) {
        insertBefore = child;
        break;
      }
    }

    if (insertBefore) {
      container.insertBefore(segment.element, insertBefore);
    } else {
      container.appendChild(segment.element);
    }
  }

  #containerFor(position: "left" | "center" | "right"): HTMLElement {
    switch (position) {
      case "left": return this.leftEl;
      case "center": return this.centerEl;
      case "right": return this.rightEl;
    }
  }
}
