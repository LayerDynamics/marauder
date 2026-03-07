/**
 * @marauder/ui/layout — Tree-based split pane layout engine.
 *
 * Each tab owns a LayoutEngine that manages a binary tree of split panes.
 * Leaf nodes map to pane IDs; split nodes divide space horizontally or vertically.
 */

/** A leaf node representing a single pane. */
export interface LeafNode {
  readonly type: "leaf";
  readonly paneId: bigint;
}

/** A split node dividing space between two children. */
export interface SplitNode {
  readonly type: "split";
  readonly direction: "horizontal" | "vertical";
  ratio: number;
  children: [LayoutNode, LayoutNode];
}

/** A node in the layout tree. */
export type LayoutNode = LeafNode | SplitNode;

/** Serializable version of LayoutNode (bigint → string for JSON). */
export type SerializedLeafNode = { type: "leaf"; paneId: string };
export type SerializedSplitNode = {
  type: "split";
  direction: "horizontal" | "vertical";
  ratio: number;
  children: [SerializedLayoutNode, SerializedLayoutNode];
};
export type SerializedLayoutNode = SerializedLeafNode | SerializedSplitNode;

/** A rectangle in pixel coordinates. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Direction for adjacency lookups. */
export type Direction = "left" | "right" | "up" | "down";

/**
 * Tree-based split pane layout engine.
 *
 * Manages a binary tree where leaves are panes and internal nodes are
 * horizontal/vertical splits with adjustable ratios.
 */
export class LayoutEngine {
  #root: LayoutNode;

  constructor(initialPaneId: bigint) {
    this.#root = { type: "leaf", paneId: initialPaneId };
  }

  /** Get the current layout tree. */
  getLayout(): LayoutNode {
    return this.#root;
  }

  /** Replace the entire layout tree (for restore). */
  setLayout(node: LayoutNode): void {
    this.#root = node;
  }

  /**
   * Split the pane identified by `paneId` in the given direction.
   * The existing pane becomes the first child; the new pane becomes the second.
   */
  split(
    paneId: bigint,
    direction: "horizontal" | "vertical",
    newPaneId: bigint,
  ): boolean {
    const result = this.#splitNode(this.#root, paneId, direction, newPaneId);
    if (result) {
      this.#root = result;
      return true;
    }
    return false;
  }

  #splitNode(
    node: LayoutNode,
    targetId: bigint,
    direction: "horizontal" | "vertical",
    newPaneId: bigint,
  ): LayoutNode | null {
    if (node.type === "leaf") {
      if (node.paneId === targetId) {
        return {
          type: "split",
          direction,
          ratio: 0.5,
          children: [
            { type: "leaf", paneId: targetId },
            { type: "leaf", paneId: newPaneId },
          ],
        };
      }
      return null;
    }

    // Recurse into children
    const leftResult = this.#splitNode(node.children[0], targetId, direction, newPaneId);
    if (leftResult) {
      return { ...node, children: [leftResult, node.children[1]] };
    }
    const rightResult = this.#splitNode(node.children[1], targetId, direction, newPaneId);
    if (rightResult) {
      return { ...node, children: [node.children[0], rightResult] };
    }
    return null;
  }

  /**
   * Remove a pane from the layout. The parent split collapses to the sibling.
   * Returns true if removed, false if not found.
   */
  remove(paneId: bigint): boolean {
    if (this.#root.type === "leaf") {
      // Can't remove the last pane
      return this.#root.paneId === paneId ? false : false;
    }
    const result = this.#removeNode(this.#root, paneId);
    if (result) {
      this.#root = result;
      return true;
    }
    return false;
  }

  #removeNode(node: LayoutNode, targetId: bigint): LayoutNode | null {
    if (node.type === "leaf") {
      return null;
    }

    const [left, right] = node.children;

    // Check if either direct child is the target leaf
    if (left.type === "leaf" && left.paneId === targetId) {
      return right; // Collapse to sibling
    }
    if (right.type === "leaf" && right.paneId === targetId) {
      return left; // Collapse to sibling
    }

    // Recurse
    const leftResult = this.#removeNode(left, targetId);
    if (leftResult) {
      return { ...node, children: [leftResult, right] };
    }
    const rightResult = this.#removeNode(right, targetId);
    if (rightResult) {
      return { ...node, children: [left, rightResult] };
    }
    return null;
  }

  /**
   * Adjust the split ratio of the parent containing `paneId`.
   * Ratio is clamped to [0.1, 0.9].
   */
  resize(paneId: bigint, ratio: number): boolean {
    const clamped = Math.max(0.1, Math.min(0.9, ratio));
    return this.#resizeNode(this.#root, paneId, clamped);
  }

  #resizeNode(node: LayoutNode, targetId: bigint, ratio: number): boolean {
    if (node.type === "leaf") return false;

    const [left, right] = node.children;
    // If either child is the target, adjust this node's ratio
    if (
      (left.type === "leaf" && left.paneId === targetId) ||
      (right.type === "leaf" && right.paneId === targetId)
    ) {
      node.ratio = ratio;
      return true;
    }

    return this.#resizeNode(left, targetId, ratio) ||
      this.#resizeNode(right, targetId, ratio);
  }

  /**
   * Compute pixel rectangles for all panes given the total available area.
   */
  computeRects(width: number, height: number): Map<bigint, Rect> {
    const result = new Map<bigint, Rect>();
    this.#computeRectsImpl(this.#root, 0, 0, width, height, result);
    return result;
  }

  #computeRectsImpl(
    node: LayoutNode,
    x: number,
    y: number,
    w: number,
    h: number,
    out: Map<bigint, Rect>,
  ): void {
    if (node.type === "leaf") {
      out.set(node.paneId, { x, y, w, h });
      return;
    }

    if (node.direction === "horizontal") {
      const leftW = Math.round(w * node.ratio);
      const rightW = w - leftW;
      this.#computeRectsImpl(node.children[0], x, y, leftW, h, out);
      this.#computeRectsImpl(node.children[1], x + leftW, y, rightW, h, out);
    } else {
      const topH = Math.round(h * node.ratio);
      const bottomH = h - topH;
      this.#computeRectsImpl(node.children[0], x, y, w, topH, out);
      this.#computeRectsImpl(node.children[1], x, y + topH, w, bottomH, out);
    }
  }

  /**
   * Hit test: which pane is at the given pixel coordinates?
   */
  getPaneAt(x: number, y: number, width: number, height: number): bigint | null {
    return this.#hitTest(this.#root, x, y, 0, 0, width, height);
  }

  #hitTest(
    node: LayoutNode,
    px: number,
    py: number,
    rx: number,
    ry: number,
    rw: number,
    rh: number,
  ): bigint | null {
    if (node.type === "leaf") {
      if (px >= rx && px < rx + rw && py >= ry && py < ry + rh) {
        return node.paneId;
      }
      return null;
    }

    if (node.direction === "horizontal") {
      const leftW = Math.round(rw * node.ratio);
      const result = this.#hitTest(node.children[0], px, py, rx, ry, leftW, rh);
      if (result !== null) return result;
      return this.#hitTest(node.children[1], px, py, rx + leftW, ry, rw - leftW, rh);
    } else {
      const topH = Math.round(rh * node.ratio);
      const result = this.#hitTest(node.children[0], px, py, rx, ry, rw, topH);
      if (result !== null) return result;
      return this.#hitTest(node.children[1], px, py, rx, ry + topH, rw, rh - topH);
    }
  }

  /**
   * Find the adjacent pane in the given direction from the specified pane.
   * Uses the center point of the source pane to find the nearest neighbor.
   */
  getAdjacentPane(
    paneId: bigint,
    direction: Direction,
    width: number,
    height: number,
  ): bigint | null {
    const rects = this.computeRects(width, height);
    const sourceRect = rects.get(paneId);
    if (!sourceRect) return null;

    const cx = sourceRect.x + sourceRect.w / 2;
    const cy = sourceRect.y + sourceRect.h / 2;

    let bestId: bigint | null = null;
    let bestDist = Infinity;

    for (const [id, rect] of rects) {
      if (id === paneId) continue;

      const targetCx = rect.x + rect.w / 2;
      const targetCy = rect.y + rect.h / 2;

      let isAdjacent = false;
      switch (direction) {
        case "left":
          isAdjacent = targetCx < cx;
          break;
        case "right":
          isAdjacent = targetCx > cx;
          break;
        case "up":
          isAdjacent = targetCy < cy;
          break;
        case "down":
          isAdjacent = targetCy > cy;
          break;
      }

      if (isAdjacent) {
        const dist = Math.abs(targetCx - cx) + Math.abs(targetCy - cy);
        if (dist < bestDist) {
          bestDist = dist;
          bestId = id;
        }
      }
    }

    return bestId;
  }

  /** Collect all pane IDs in the layout tree. */
  getAllPaneIds(): bigint[] {
    const ids: bigint[] = [];
    this.#collectIds(this.#root, ids);
    return ids;
  }

  #collectIds(node: LayoutNode, out: bigint[]): void {
    if (node.type === "leaf") {
      out.push(node.paneId);
    } else {
      this.#collectIds(node.children[0], out);
      this.#collectIds(node.children[1], out);
    }
  }

  /** Serialize the layout tree for JSON storage. */
  serialize(): SerializedLayoutNode {
    return serializeNode(this.#root);
  }

  /** Deserialize a layout tree from JSON storage. */
  static deserialize(data: SerializedLayoutNode): LayoutNode {
    return deserializeNode(data);
  }
}

/** Serialize a LayoutNode to a JSON-safe representation. */
function serializeNode(node: LayoutNode): SerializedLayoutNode {
  if (node.type === "leaf") {
    return { type: "leaf", paneId: node.paneId.toString() };
  }
  return {
    type: "split",
    direction: node.direction,
    ratio: node.ratio,
    children: [
      serializeNode(node.children[0]),
      serializeNode(node.children[1]),
    ],
  };
}

/** Deserialize a SerializedLayoutNode back to a LayoutNode. */
function deserializeNode(data: SerializedLayoutNode): LayoutNode {
  if (data.type === "leaf") {
    return { type: "leaf", paneId: BigInt(data.paneId) };
  }
  return {
    type: "split",
    direction: data.direction,
    ratio: data.ratio,
    children: [
      deserializeNode(data.children[0]),
      deserializeNode(data.children[1]),
    ],
  };
}
