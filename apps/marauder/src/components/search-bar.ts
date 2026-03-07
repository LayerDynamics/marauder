/**
 * SearchBar — floating search input overlay for in-terminal search.
 *
 * Positioned top-right of the terminal grid. Emits search events via
 * the event bus and displays match counts from overlay highlights.
 */

import { invoke } from "@tauri-apps/api/core";
import { EventType } from "../types";

const DEBOUNCE_MS = 150;

export class SearchBar {
  readonly #container: HTMLElement;
  readonly #input: HTMLInputElement;
  readonly #matchCount: HTMLSpanElement;
  readonly #prevBtn: HTMLButtonElement;
  readonly #nextBtn: HTMLButtonElement;
  readonly #closeBtn: HTMLButtonElement;
  #visible = false;
  #totalMatches = 0;
  #currentMatch = 0;
  #debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // Store bound handlers for cleanup
  readonly #onInputHandler: () => void;
  readonly #onKeydownHandler: (e: KeyboardEvent) => void;
  readonly #onPrevClick: () => void;
  readonly #onNextClick: () => void;
  readonly #onCloseClick: () => void;

  constructor(container: HTMLElement) {
    this.#container = container;
    this.#container.className = "search-bar";
    this.#container.style.display = "none";

    this.#input = document.createElement("input");
    this.#input.type = "text";
    this.#input.placeholder = "Search…";
    this.#input.className = "search-bar-input";
    this.#input.setAttribute("aria-label", "Search terminal");
    this.#input.maxLength = 1024;

    this.#matchCount = document.createElement("span");
    this.#matchCount.className = "search-bar-count";
    this.#matchCount.textContent = "";

    this.#prevBtn = document.createElement("button");
    this.#prevBtn.className = "search-bar-btn";
    this.#prevBtn.textContent = "▲";
    this.#prevBtn.title = "Previous match";
    this.#prevBtn.setAttribute("aria-label", "Previous match");

    this.#nextBtn = document.createElement("button");
    this.#nextBtn.className = "search-bar-btn";
    this.#nextBtn.textContent = "▼";
    this.#nextBtn.title = "Next match";
    this.#nextBtn.setAttribute("aria-label", "Next match");

    this.#closeBtn = document.createElement("button");
    this.#closeBtn.className = "search-bar-btn search-bar-close";
    this.#closeBtn.textContent = "✕";
    this.#closeBtn.title = "Close search";
    this.#closeBtn.setAttribute("aria-label", "Close search");

    this.#container.appendChild(this.#input);
    this.#container.appendChild(this.#matchCount);
    this.#container.appendChild(this.#prevBtn);
    this.#container.appendChild(this.#nextBtn);
    this.#container.appendChild(this.#closeBtn);

    // Bind handlers for later removal
    this.#onInputHandler = () => this.#onInput();
    this.#onKeydownHandler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        this.hide();
      } else if (e.key === "Enter") {
        e.shiftKey ? this.#navigatePrev() : this.#navigateNext();
      }
    };
    this.#onPrevClick = () => this.#navigatePrev();
    this.#onNextClick = () => this.#navigateNext();
    this.#onCloseClick = () => this.hide();

    // Event handlers
    this.#input.addEventListener("input", this.#onInputHandler);
    this.#input.addEventListener("keydown", this.#onKeydownHandler);
    this.#prevBtn.addEventListener("click", this.#onPrevClick);
    this.#nextBtn.addEventListener("click", this.#onNextClick);
    this.#closeBtn.addEventListener("click", this.#onCloseClick);
  }

  show(): void {
    if (this.#visible) return;
    this.#visible = true;
    this.#container.style.display = "flex";
    this.#input.value = "";
    this.#matchCount.textContent = "";
    this.#totalMatches = 0;
    this.#currentMatch = 0;
    this.#input.focus();
  }

  hide(): void {
    if (!this.#visible) return;
    this.#visible = false;
    this.#container.style.display = "none";
    this.#input.value = "";
    this.#clearDebounce();
    // Emit clear search
    this.#emitEvent("SearchQuery", { query: "", clear: true });
  }

  isVisible(): boolean {
    return this.#visible;
  }

  /** Update match count from OverlayHighlights event data. */
  setMatchInfo(total: number, current: number): void {
    this.#totalMatches = total;
    this.#currentMatch = current;
    this.#matchCount.textContent = total > 0
      ? `${current}/${total}`
      : this.#input.value.length > 0 ? "0 results" : "";
  }

  /** Remove all event listeners and clear timers. */
  destroy(): void {
    this.#clearDebounce();
    this.#input.removeEventListener("input", this.#onInputHandler);
    this.#input.removeEventListener("keydown", this.#onKeydownHandler);
    this.#prevBtn.removeEventListener("click", this.#onPrevClick);
    this.#nextBtn.removeEventListener("click", this.#onNextClick);
    this.#closeBtn.removeEventListener("click", this.#onCloseClick);
  }

  #clearDebounce(): void {
    if (this.#debounceTimer !== null) {
      clearTimeout(this.#debounceTimer);
      this.#debounceTimer = null;
    }
  }

  #onInput(): void {
    this.#clearDebounce();
    this.#debounceTimer = setTimeout(() => {
      const query = this.#input.value;
      this.#emitEvent("SearchQuery", { query });
    }, DEBOUNCE_MS);
  }

  #navigateNext(): void {
    this.#emitEvent("SearchNavigate", { direction: "next" });
  }

  #navigatePrev(): void {
    this.#emitEvent("SearchNavigate", { direction: "prev" });
  }

  #emitEvent(type: string, payload: Record<string, unknown>): void {
    invoke("event_bus_emit", {
      event_type: EventType.ExtensionMessage,
      payload: JSON.stringify({
        source: "search-bar",
        type,
        payload,
      }),
    }).catch(console.error);
  }
}
