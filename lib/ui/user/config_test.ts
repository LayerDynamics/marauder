/**
 * Unit tests for keybinding configuration.
 * Run with: deno test lib/ui/user/config_test.ts
 */

import {
  assertEquals,
  assert,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import { normalizeKeySequence, DEFAULT_KEYBINDINGS } from "./config.ts";

Deno.test("normalizeKeySequence sorts modifiers canonically", () => {
  assertEquals(normalizeKeySequence("Shift+Ctrl+T"), "Ctrl+Shift+T");
  assertEquals(normalizeKeySequence("Meta+Alt+Ctrl+X"), "Ctrl+Alt+Meta+X");
  assertEquals(normalizeKeySequence("Ctrl+T"), "Ctrl+T");
});

Deno.test("normalizeKeySequence preserves non-modifier keys", () => {
  assertEquals(normalizeKeySequence("Tab"), "Tab");
  assertEquals(normalizeKeySequence("Ctrl+Shift+Tab"), "Ctrl+Shift+Tab");
});

Deno.test("DEFAULT_KEYBINDINGS includes platform-appropriate modifier", () => {
  // On macOS (darwin), should use Meta; elsewhere Ctrl
  const isMac = typeof Deno !== "undefined" && Deno.build.os === "darwin";
  const tabAction = isMac
    ? DEFAULT_KEYBINDINGS["Meta+T"]
    : DEFAULT_KEYBINDINGS["Ctrl+T"];
  assertEquals(tabAction, "new-tab");
});

Deno.test("DEFAULT_KEYBINDINGS has all expected actions", () => {
  const actions = Object.values(DEFAULT_KEYBINDINGS);
  assert(actions.includes("new-tab"));
  assert(actions.includes("close-tab"));
  assert(actions.includes("command-palette"));
  assert(actions.includes("search"));
  assert(actions.includes("history-search"));
});

Deno.test("Ctrl bindings remain for non-Mod actions", () => {
  // jump-prev-prompt and history-search always use Ctrl, not Mod
  assertEquals(DEFAULT_KEYBINDINGS["Ctrl+Up"], "jump-prev-prompt");
  assertEquals(DEFAULT_KEYBINDINGS["Ctrl+Down"], "jump-next-prompt");
  assertEquals(DEFAULT_KEYBINDINGS["Ctrl+R"], "history-search");
});
