/**
 * Unit tests for CompletionEngine and providers.
 * Run with: deno test --allow-read lib/shell/completions_test.ts
 */

import {
  assertEquals,
  assert,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  CompletionEngine,
  HistoryCompletionProvider,
  type CompletionProvider,
  type CompletionContext,
  type CompletionItem,
} from "./completions.ts";
import { CommandHistory } from "./history.ts";
import type { CommandRecord } from "./mod.ts";

function makeContext(input: string, cursorPosition?: number): CompletionContext {
  const h = new CommandHistory();
  h.add({ command: "git status", cwd: "/home", startTime: 0 });
  h.add({ command: "git push", cwd: "/home", startTime: 0 });
  h.add({ command: "ls -la", cwd: "/home", startTime: 0 });
  h.add({ command: "grep foo bar.txt", cwd: "/home", startTime: 0 });
  return {
    input,
    cursorPosition: cursorPosition ?? input.length,
    cwd: "/home",
    history: h,
  };
}

Deno.test("HistoryCompletionProvider extracts token at cursor, not full line", () => {
  const provider = new HistoryCompletionProvider();
  // "ls | grep fo" — token at cursor is "fo", should match nothing directly
  // but "grep" alone should match "grep foo bar.txt"
  const ctx = makeContext("ls | grep", 9);
  const items = provider.provide(ctx);
  // The token at cursor position 9 is "grep"
  const labels = items.map((i) => i.label);
  assert(labels.some((l) => l.includes("grep")));
});

Deno.test("HistoryCompletionProvider returns empty for empty token", () => {
  const provider = new HistoryCompletionProvider();
  const items = provider.provide(makeContext("  "));
  assertEquals(items.length, 0);
});

Deno.test("CompletionEngine isolates provider failures", async () => {
  const engine = new CompletionEngine();
  const failingProvider: CompletionProvider = {
    id: "failing",
    provide(): CompletionItem[] {
      throw new Error("provider crash");
    },
  };
  const workingProvider: CompletionProvider = {
    id: "working",
    provide(): CompletionItem[] {
      return [{ label: "test", kind: "command" }];
    },
  };
  engine.registerProvider(failingProvider);
  engine.registerProvider(workingProvider);
  const results = await engine.complete(makeContext("t"));
  // Working provider's results should still be returned despite failing provider
  assertEquals(results.length, 1);
  assertEquals(results[0]!.label, "test");
});

Deno.test("CompletionEngine respects timeout", async () => {
  const engine = new CompletionEngine();
  const slowProvider: CompletionProvider = {
    id: "slow",
    provide(): Promise<CompletionItem[]> {
      return new Promise((resolve) => setTimeout(() => resolve([{ label: "slow", kind: "command" }]), 5000));
    },
  };
  const fastProvider: CompletionProvider = {
    id: "fast",
    provide(): CompletionItem[] {
      return [{ label: "fast", kind: "command" }];
    },
  };
  engine.registerProvider(slowProvider);
  engine.registerProvider(fastProvider);

  const start = Date.now();
  const results = await engine.complete(makeContext("t"), 100);
  const elapsed = Date.now() - start;

  // Should complete within ~200ms (100ms timeout + overhead), not 5000ms
  assert(elapsed < 1000, `Took too long: ${elapsed}ms`);
  // Fast provider results should be present
  assert(results.some((r) => r.label === "fast"));
});

Deno.test("CompletionEngine register and unregister", () => {
  const engine = new CompletionEngine();
  const p: CompletionProvider = { id: "test", provide: () => [] };
  engine.registerProvider(p);
  assertEquals(engine.getProviderIds(), ["test"]);
  engine.unregisterProvider("test");
  assertEquals(engine.getProviderIds(), []);
});
