/**
 * Unit tests for CommandHistory (ring buffer) and fuzzy search.
 * Run with: deno test lib/shell/history_test.ts
 */

import {
  assertEquals,
  assert,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import { CommandHistory } from "./history.ts";
import type { CommandRecord } from "./mod.ts";

function makeRecord(command: string, cwd = "/home"): CommandRecord {
  return { command, cwd, startTime: Date.now() };
}

Deno.test("add and getAll returns records in insertion order", () => {
  const h = new CommandHistory();
  h.add(makeRecord("ls"));
  h.add(makeRecord("pwd"));
  h.add(makeRecord("echo hi"));
  assertEquals(h.size, 3);
  const all = h.getAll();
  assertEquals(all[0]!.command, "ls");
  assertEquals(all[2]!.command, "echo hi");
});

Deno.test("ring buffer evicts oldest at capacity with O(1)", () => {
  const h = new CommandHistory({ maxSize: 3 });
  h.add(makeRecord("a"));
  h.add(makeRecord("b"));
  h.add(makeRecord("c"));
  h.add(makeRecord("d")); // evicts "a"
  assertEquals(h.size, 3);
  const all = h.getAll();
  assertEquals(all[0]!.command, "b");
  assertEquals(all[1]!.command, "c");
  assertEquals(all[2]!.command, "d");
});

Deno.test("getLast returns last N records", () => {
  const h = new CommandHistory({ maxSize: 5 });
  for (let i = 0; i < 5; i++) h.add(makeRecord(`cmd${i}`));
  const last2 = h.getLast(2);
  assertEquals(last2.length, 2);
  assertEquals(last2[0]!.command, "cmd3");
  assertEquals(last2[1]!.command, "cmd4");
});

Deno.test("clear resets the buffer", () => {
  const h = new CommandHistory();
  h.add(makeRecord("ls"));
  h.clear();
  assertEquals(h.size, 0);
  assertEquals(h.getAll().length, 0);
});

Deno.test("search returns matching records newest first", () => {
  const h = new CommandHistory();
  h.add(makeRecord("git status"));
  h.add(makeRecord("ls -la"));
  h.add(makeRecord("git push"));
  const results = h.search("git");
  assertEquals(results.length, 2);
  assertEquals(results[0]!.command, "git push"); // newest first
  assertEquals(results[1]!.command, "git status");
});

Deno.test("fuzzySearch scores and ranks results", () => {
  const h = new CommandHistory();
  h.add(makeRecord("git status"));
  h.add(makeRecord("gist create"));
  h.add(makeRecord("ls"));
  const results = h.fuzzySearch("gis");
  assert(results.length >= 2);
  // Both "git status" and "gist create" should match "gis"
  const commands = results.map((r) => r.record.command);
  assert(commands.includes("gist create"));
  assert(commands.includes("git status"));
});

Deno.test("ring buffer wraps correctly after many insertions", () => {
  const h = new CommandHistory({ maxSize: 3 });
  for (let i = 0; i < 10; i++) h.add(makeRecord(`cmd${i}`));
  assertEquals(h.size, 3);
  const all = h.getAll();
  assertEquals(all[0]!.command, "cmd7");
  assertEquals(all[1]!.command, "cmd8");
  assertEquals(all[2]!.command, "cmd9");
});

Deno.test("getByExitCode filters correctly", () => {
  const h = new CommandHistory();
  const r1 = makeRecord("ok");
  r1.exitCode = 0;
  const r2 = makeRecord("fail");
  r2.exitCode = 1;
  h.add(r1);
  h.add(r2);
  assertEquals(h.getByExitCode(0).length, 1);
  assertEquals(h.getByExitCode(0)[0]!.command, "ok");
});

Deno.test("getByDirectory filters correctly", () => {
  const h = new CommandHistory();
  h.add(makeRecord("ls", "/home"));
  h.add(makeRecord("pwd", "/tmp"));
  assertEquals(h.getByDirectory("/tmp").length, 1);
  assertEquals(h.getByDirectory("/tmp")[0]!.command, "pwd");
});
