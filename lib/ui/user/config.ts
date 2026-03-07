/**
 * @marauder/ui/user — Keybinding configuration loader
 *
 * Loads keybinding mappings from the config store and merges with defaults.
 */

import type { ConfigStore } from "@marauder/ffi-config-store";

/** Maps canonical key sequences (e.g. "Ctrl+T") to action names. */
export type KeybindingConfig = Record<string, string>;

/** Detect if running on macOS. */
function isMacOS(): boolean {
  // Works in both Deno and browser contexts
  if (typeof globalThis.navigator !== "undefined") {
    return globalThis.navigator.platform?.startsWith("Mac") ??
      globalThis.navigator.userAgent?.includes("Mac") ?? false;
  }
  // Deno runtime
  if (typeof Deno !== "undefined") {
    return Deno.build.os === "darwin";
  }
  return false;
}

/** Base keybinding actions — platform modifier applied dynamically. */
const KEYBINDING_ACTIONS: { mod: string; key: string; action: string }[] = [
  { mod: "Mod", key: "T", action: "new-tab" },
  { mod: "Mod", key: "W", action: "close-tab" },
  { mod: "Ctrl", key: "Tab", action: "next-tab" },
  { mod: "Ctrl+Shift", key: "Tab", action: "prev-tab" },
  { mod: "Mod+Shift", key: "N", action: "split-pane" },
  { mod: "Mod+Shift", key: "W", action: "close-pane" },
  { mod: "Mod+Shift", key: "Right", action: "focus-next" },
  { mod: "Mod+Shift", key: "Left", action: "focus-prev" },
  { mod: "Mod+Shift", key: "P", action: "command-palette" },
  { mod: "Mod+Shift", key: "F", action: "search" },
  { mod: "Mod", key: "Plus", action: "font-size-increase" },
  { mod: "Mod", key: "Minus", action: "font-size-decrease" },
  { mod: "Mod", key: "0", action: "font-size-reset" },
  { mod: "Ctrl", key: "Up", action: "jump-prev-prompt" },
  { mod: "Ctrl", key: "Down", action: "jump-next-prompt" },
  { mod: "Ctrl", key: "R", action: "history-search" },
];

/** Build platform-appropriate default keybindings. "Mod" becomes Meta on macOS, Ctrl elsewhere. */
function buildDefaults(): KeybindingConfig {
  const mac = isMacOS();
  const config: KeybindingConfig = {};
  for (const { mod, key, action } of KEYBINDING_ACTIONS) {
    const resolvedMod = mac ? mod.replace("Mod", "Meta") : mod.replace("Mod", "Ctrl");
    config[`${resolvedMod}+${key}`] = action;
  }
  return config;
}

/** Default keybindings shipped with Marauder (platform-aware). */
export const DEFAULT_KEYBINDINGS: KeybindingConfig = buildDefaults();

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
