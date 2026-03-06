/**
 * @marauder/ui/user — Keybinding configuration loader
 *
 * Loads keybinding mappings from the config store and merges with defaults.
 */

import type { ConfigStore } from "@marauder/ffi-config-store";

/** Maps canonical key sequences (e.g. "Ctrl+T") to action names. */
export type KeybindingConfig = Record<string, string>;

/** Default keybindings shipped with Marauder. */
export const DEFAULT_KEYBINDINGS: KeybindingConfig = {
  "Ctrl+T": "new-tab",
  "Ctrl+W": "close-tab",
  "Ctrl+Tab": "next-tab",
  "Ctrl+Shift+Tab": "prev-tab",
  "Ctrl+Shift+N": "split-pane",
  "Ctrl+Shift+W": "close-pane",
  "Ctrl+Shift+Right": "focus-next",
  "Ctrl+Shift+Left": "focus-prev",
  "Ctrl+Shift+P": "command-palette",
  "Ctrl+Shift+F": "search",
  "Ctrl+Plus": "font-size-increase",
  "Ctrl+Minus": "font-size-decrease",
  "Ctrl+0": "font-size-reset",
};

/** Canonical modifier order: Ctrl → Alt → Shift → Meta. */
const MODIFIER_ORDER = ["Ctrl", "Alt", "Shift", "Meta"] as const;
const MODIFIER_SET = new Set<string>(MODIFIER_ORDER);

/**
 * Normalize a key sequence to canonical form.
 * Ensures modifier order is Ctrl+Alt+Shift+Meta and key is consistent.
 * e.g. "Shift+Ctrl+T" → "Ctrl+Shift+T"
 */
export function normalizeKeySequence(keySeq: string): string {
  const parts = keySeq.split("+");
  const modifiers: string[] = [];
  const keys: string[] = [];

  for (const part of parts) {
    if (MODIFIER_SET.has(part)) {
      modifiers.push(part);
    } else {
      keys.push(part);
    }
  }

  // Sort modifiers into canonical order
  modifiers.sort((a, b) =>
    MODIFIER_ORDER.indexOf(a as typeof MODIFIER_ORDER[number]) -
    MODIFIER_ORDER.indexOf(b as typeof MODIFIER_ORDER[number])
  );

  return [...modifiers, ...keys].join("+");
}

/**
 * Load keybindings from the config store, merged with defaults.
 * Config values override defaults for the same key sequence.
 * User keys are normalized to canonical modifier order so
 * "Shift+Ctrl+T" matches "Ctrl+Shift+T".
 */
export function loadKeybindings(configStore: ConfigStore): KeybindingConfig {
  const merged = { ...DEFAULT_KEYBINDINGS };

  const overrides = configStore.get<Record<string, string>>("keybindings");
  if (overrides && typeof overrides === "object") {
    for (const [keySeq, action] of Object.entries(overrides)) {
      if (typeof action === "string") {
        const normalized = normalizeKeySequence(keySeq);
        if (normalized !== keySeq) {
          console.warn(
            `Keybinding "${keySeq}" normalized to "${normalized}". ` +
            `Use canonical order: Ctrl+Alt+Shift+Meta+Key`,
          );
        }
        merged[normalized] = action;
      }
    }
  }

  return merged;
}
