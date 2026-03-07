// extensions/search/mod.ts
// GPU-accelerated in-terminal search with overlay highlights and next/prev navigation.

import type { ExtensionContext } from "@marauder/extensions";

interface SearchMatch {
  row: number;
  col: number;
  length: number;
}

interface SearchResultPayload {
  matches: SearchMatch[];
  pattern: string;
}

interface SearchQueryPayload {
  pattern: string;
}

interface SearchNavigatePayload {
  direction: "next" | "prev";
}

// ---------------------------------------------------------------------------
// Module-level state
// ---------------------------------------------------------------------------

let _isOpen = false;
let _currentMatches: SearchMatch[] = [];
let _currentMatchIndex = -1;
const _unsubscribers: Array<() => void> = [];

function updateStatusBar(ctx: ExtensionContext): void {
  if (!_isOpen) {
    ctx.statusBar.set("center", "");
    return;
  }
  if (_currentMatches.length === 0) {
    ctx.statusBar.set("center", "search: no results");
    return;
  }
  const displayIndex = _currentMatchIndex >= 0 ? _currentMatchIndex + 1 : 1;
  ctx.statusBar.set(
    "center",
    `search: ${displayIndex}/${_currentMatches.length}`,
  );
}

function emitOverlayHighlights(ctx: ExtensionContext): void {
  ctx.events.emit("ExtensionMessage", {
    source: "search",
    type: "OverlayHighlights",
    payload: {
      matches: _currentMatches,
      currentIndex: _currentMatchIndex,
    },
  });
}

function navigateTo(ctx: ExtensionContext, index: number): void {
  if (_currentMatches.length === 0) return;
  _currentMatchIndex =
    ((index % _currentMatches.length) + _currentMatches.length) %
    _currentMatches.length;
  updateStatusBar(ctx);
  // Emit a scroll-to event so the renderer can bring the match into view.
  ctx.events.emit("ExtensionMessage", {
    source: "search",
    type: "ScrollToMatch",
    payload: _currentMatches[_currentMatchIndex],
  });
  emitOverlayHighlights(ctx);
}

function openSearch(ctx: ExtensionContext): void {
  if (_isOpen) return;
  _isOpen = true;
  ctx.events.emit("ExtensionMessage", {
    source: "search",
    type: "OverlayShow",
    payload: { kind: "search-input" },
  });
  updateStatusBar(ctx);
}

function closeSearch(ctx: ExtensionContext): void {
  if (!_isOpen) return;
  _isOpen = false;
  _currentMatches = [];
  _currentMatchIndex = -1;
  ctx.events.emit("ExtensionMessage", {
    source: "search",
    type: "OverlayHide",
    payload: { kind: "search-input" },
  });
  // Clear any active highlights.
  ctx.events.emit("ExtensionMessage", {
    source: "search",
    type: "OverlayHighlights",
    payload: { matches: [], currentIndex: -1 },
  });
  updateStatusBar(ctx);
}

export function activate(ctx: ExtensionContext): void {
  // Register the toggle command.
  ctx.commands.register("marauder.search.toggle", () => {
    if (_isOpen) {
      closeSearch(ctx);
    } else {
      openSearch(ctx);
    }
  });

  // Register next/prev commands for keyboard navigation within results.
  ctx.commands.register("marauder.search.next", () => {
    navigateTo(ctx, _currentMatchIndex + 1);
  });

  ctx.commands.register("marauder.search.prev", () => {
    navigateTo(ctx, _currentMatchIndex - 1);
  });

  // Bind Ctrl+Shift+F to the toggle command.
  ctx.keybindings.register("Ctrl+Shift+F", "marauder.search.toggle");
  ctx.keybindings.register("F3", "marauder.search.next");
  ctx.keybindings.register("Shift+F3", "marauder.search.prev");

  // Listen for the compute engine returning search results.
  const unsubResults = ctx.events.on(
    "ExtensionMessage",
    (raw: unknown) => {
      const msg = raw as {
        source?: string;
        type?: string;
        payload?: unknown;
      };
      if (msg.source !== "search" && msg.type === "SearchResults") {
        const result = msg.payload as SearchResultPayload;
        _currentMatches = result.matches;
        _currentMatchIndex = _currentMatches.length > 0 ? 0 : -1;
        updateStatusBar(ctx);
        emitOverlayHighlights(ctx);
      }
    },
  );
  _unsubscribers.push(unsubResults);

  // Listen for the UI sending a new search query (typed by the user).
  const unsubQuery = ctx.events.on(
    "ExtensionMessage",
    (raw: unknown) => {
      const msg = raw as {
        source?: string;
        type?: string;
        payload?: unknown;
      };
      if (msg.source !== "search" && msg.type === "SearchQuery") {
        const query = msg.payload as SearchQueryPayload;
        if (query.pattern.length === 0) {
          _currentMatches = [];
          _currentMatchIndex = -1;
          updateStatusBar(ctx);
          emitOverlayHighlights(ctx);
          return;
        }
        // Delegate the actual search to the compute engine via the event bus.
        ctx.events.emit("ExtensionMessage", {
          source: "search",
          type: "ComputeSearch",
          payload: { pattern: query.pattern },
        });
      }
    },
  );
  _unsubscribers.push(unsubQuery);

  // Listen for UI navigation (next/prev) events emitted by the webview.
  const unsubNav = ctx.events.on(
    "ExtensionMessage",
    (raw: unknown) => {
      const msg = raw as {
        source?: string;
        type?: string;
        payload?: unknown;
      };
      if (msg.source !== "search" && msg.type === "SearchNavigate") {
        const nav = msg.payload as SearchNavigatePayload;
        if (nav.direction === "next") {
          navigateTo(ctx, _currentMatchIndex + 1);
        } else {
          navigateTo(ctx, _currentMatchIndex - 1);
        }
      }
    },
  );
  _unsubscribers.push(unsubNav);
}

export function deactivate(): void {
  for (const unsub of _unsubscribers) {
    unsub();
  }
  _unsubscribers.length = 0;
  _isOpen = false;
  _currentMatches = [];
  _currentMatchIndex = -1;
}
