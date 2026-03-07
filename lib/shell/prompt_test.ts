/**
 * Unit tests for PromptTracker.
 * Run with: deno test lib/shell/prompt_test.ts
 */

import {
  assertEquals,
  assertNotEquals,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import { PromptTracker } from "./prompt.ts";

Deno.test("recordPrompt stores prompt info", () => {
  const t = new PromptTracker();
  t.recordPrompt(0, "/home");
  t.recordPrompt(10, "/tmp");
  assertEquals(t.size, 2);
  assertEquals(t.getAll()[0]!.row, 0);
  assertEquals(t.getAll()[1]!.cwd, "/tmp");
});

Deno.test("getPromptAt returns prompt at or before row", () => {
  const t = new PromptTracker();
  t.recordPrompt(0, "/a");
  t.recordPrompt(10, "/b");
  t.recordPrompt(20, "/c");

  assertEquals(t.getPromptAt(20)!.row, 20);
  assertEquals(t.getPromptAt(25)!.row, 20);
  assertEquals(t.getPromptAt(15)!.row, 10);
  assertEquals(t.getPromptAt(0)!.row, 0);
  assertEquals(t.getPromptAt(-1), null);
});

Deno.test("getPrevious returns the prompt immediately before currentRow", () => {
  const t = new PromptTracker();
  t.recordPrompt(0, "/a");
  t.recordPrompt(10, "/b");
  t.recordPrompt(20, "/c");

  // From row 25, previous is row 20
  assertEquals(t.getPrevious(25)!.row, 20);
  // From row 20, previous is row 10
  assertEquals(t.getPrevious(20)!.row, 10);
  // From row 15, previous is row 10
  assertEquals(t.getPrevious(15)!.row, 10);
  // From row 10, previous is row 0
  assertEquals(t.getPrevious(10)!.row, 0);
  // From row 5, previous is row 0
  assertEquals(t.getPrevious(5)!.row, 0);
  // From row 0, no previous
  assertEquals(t.getPrevious(0), null);
});

Deno.test("getNext returns the prompt immediately after currentRow", () => {
  const t = new PromptTracker();
  t.recordPrompt(0, "/a");
  t.recordPrompt(10, "/b");
  t.recordPrompt(20, "/c");

  assertEquals(t.getNext(0)!.row, 10);
  assertEquals(t.getNext(5)!.row, 10);
  assertEquals(t.getNext(15)!.row, 20);
  assertEquals(t.getNext(20), null);
});

Deno.test("recordFinish and recordCommand update most recent prompt", () => {
  const t = new PromptTracker();
  t.recordPrompt(0, "/home");
  t.recordCommand("ls -la");
  t.recordFinish(0);

  const p = t.getAll()[0]!;
  assertEquals(p.command, "ls -la");
  assertEquals(p.exitCode, 0);
});

Deno.test("capacity limit evicts oldest prompts", () => {
  const t = new PromptTracker();
  // Push 5001 prompts — should trigger eviction at 5000
  for (let i = 0; i < 5001; i++) {
    t.recordPrompt(i, `/dir${i}`);
  }
  // After eviction of 25% (1250), then adding 1 more = 3751
  assertNotEquals(t.size, 5001);
  // The oldest prompts should have been evicted
  const all = t.getAll();
  assertEquals(all[0]!.row > 0, true);
});
