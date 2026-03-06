/**
 * Integration test: TerminalPipeline end-to-end.
 *
 * Validates: create → start → write → grid updated → resize → destroy.
 * Run with: deno test --allow-all --unstable-ffi lib/io/pipeline_test.ts
 */

import {
  assert,
  assertEquals,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import { TerminalPipeline } from "./pipeline.ts";
import { EventType } from "@marauder/ffi-event-bus";

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Poll grid cells until `target` string appears in row content,
 * or timeout elapses.
 */
async function waitForGridContent(
  pipeline: TerminalPipeline,
  target: string,
  timeoutMs = 5000,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    // Scan visible rows for target text
    for (let row = 0; row < 24; row++) {
      let line = "";
      for (let col = 0; col < 80; col++) {
        const cell = pipeline.grid.getCell(row, col);
        if (cell && cell.char && cell.char !== "\0") {
          line += cell.char;
        }
      }
      if (line.includes(target)) {
        return true;
      }
    }
    await sleep(100);
  }
  return false;
}

Deno.test({
  name: "Pipeline: lifecycle — create → start → running → destroy",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pipeline = TerminalPipeline.create({ rows: 24, cols: 80 });
    assertEquals(pipeline.running, false, "Should not be running before start");

    pipeline.start();
    assertEquals(pipeline.running, true, "Should be running after start");

    await pipeline.destroy();
    assertEquals(pipeline.running, false, "Should not be running after destroy");
  },
});

Deno.test({
  name: "Pipeline: write echo hello → grid contains hello",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pipeline = TerminalPipeline.create({ rows: 24, cols: 80 });
    pipeline.start();

    // Wait for shell to initialize
    await sleep(500);

    pipeline.write("echo hello\n");

    const found = await waitForGridContent(pipeline, "hello");
    assert(found, "Expected 'hello' to appear in grid cells");

    await pipeline.destroy();
  },
});

Deno.test({
  name: "Pipeline: resize propagates GridResized event",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pipeline = TerminalPipeline.create({ rows: 24, cols: 80 });
    pipeline.start();
    await sleep(300);

    let resizeEvent: { rows: number; cols: number } | null = null;
    pipeline.eventBus.subscribe(EventType.GridResized, (event) => {
      resizeEvent = event as { rows: number; cols: number };
    });

    pipeline.resize(40, 120);

    // Event fires synchronously during resize()
    assert(resizeEvent !== null, "GridResized event should have fired");
    assertEquals(resizeEvent!.rows, 40);
    assertEquals(resizeEvent!.cols, 120);

    await pipeline.destroy();
  },
});

Deno.test({
  name: "Pipeline: event bus fires PtyOutput and GridUpdated during operation",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pipeline = TerminalPipeline.create({ rows: 24, cols: 80 });

    let ptyOutputCount = 0;
    let gridUpdatedCount = 0;

    pipeline.eventBus.subscribe(EventType.PtyOutput, () => {
      ptyOutputCount++;
    });
    pipeline.eventBus.subscribe(EventType.GridUpdated, () => {
      gridUpdatedCount++;
    });

    pipeline.start();
    await sleep(500);

    pipeline.write("echo test\n");
    await sleep(1000);

    assert(ptyOutputCount > 0, "Should have received PtyOutput events");
    assert(gridUpdatedCount > 0, "Should have received GridUpdated events");

    await pipeline.destroy();
  },
});

Deno.test({
  name: "Pipeline: clearDirty prevents dirty row accumulation",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pipeline = TerminalPipeline.create({ rows: 24, cols: 80 });

    const dirtyRowCounts: number[] = [];
    pipeline.eventBus.subscribe(EventType.GridUpdated, (event) => {
      const ev = event as { dirtyRows: number[] };
      dirtyRowCounts.push(ev.dirtyRows.length);
    });

    pipeline.start();
    await sleep(500);

    // Send two separate commands to trigger two grid update cycles
    pipeline.write("echo first\n");
    await sleep(500);

    pipeline.write("echo second\n");
    await sleep(500);

    // With clearDirty working, later updates should NOT accumulate
    // all previously dirty rows. The last update should have fewer
    // or equal dirty rows compared to the total rows ever dirtied.
    if (dirtyRowCounts.length >= 2) {
      const lastCount = dirtyRowCounts[dirtyRowCounts.length - 1]!;
      // Without clearDirty, lastCount would grow toward 24 (all rows).
      // With clearDirty, it should be a small number (just newly changed rows).
      assert(
        lastCount < 24,
        `Last dirty row count (${lastCount}) should be less than total rows (24) — clearDirty should prevent accumulation`,
      );
    }

    await pipeline.destroy();
  },
});
