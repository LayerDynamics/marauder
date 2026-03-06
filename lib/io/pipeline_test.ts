/**
 * Integration test: TerminalPipeline end-to-end.
 *
 * Validates: create → start → write → grid updated → resize → destroy.
 * Run with: deno test --allow-all --unstable-ffi lib/io/pipeline_test.ts
 */

import {
  assert,
  assertEquals,
  assertGreater,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import { TerminalPipeline } from "./pipeline.ts";
import { EventType } from "@marauder/ffi-event-bus";

const TEST_ROWS = 24;
const TEST_COLS = 80;

/**
 * Wait for a specific event to fire, with timeout.
 * Returns the event payload, or throws if timeout elapses.
 */
function waitForEvent(
  pipeline: TerminalPipeline,
  eventType: EventType,
  timeoutMs = 5000,
): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`Timed out waiting for event ${eventType}`));
    }, timeoutMs);

    pipeline.eventBus.subscribe(eventType, (event) => {
      clearTimeout(timer);
      resolve(event);
    });
  });
}

/**
 * Poll grid cells until `target` string appears in row content,
 * or timeout elapses. Reads dimensions from the grid instance.
 */
async function waitForGridContent(
  pipeline: TerminalPipeline,
  target: string,
  timeoutMs = 5000,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    const rows = pipeline.grid.rows;
    const cols = pipeline.grid.cols;

    for (let row = 0; row < rows; row++) {
      let line = "";
      for (let col = 0; col < cols; col++) {
        const cell = pipeline.grid.getCell(row, col);
        if (cell && cell.char && cell.char !== "\0") {
          line += cell.char;
        }
      }
      if (line.includes(target)) {
        return true;
      }
    }
    await new Promise((r) => setTimeout(r, 50));
  }
  return false;
}

/**
 * Wait for the shell to be ready by polling for a GridUpdated event,
 * which indicates the shell has produced initial output.
 */
function waitForShellReady(
  pipeline: TerminalPipeline,
  timeoutMs = 5000,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error("Timed out waiting for shell to initialize"));
    }, timeoutMs);

    pipeline.eventBus.subscribe(EventType.GridUpdated, () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

Deno.test({
  name: "Pipeline: lifecycle — create → start → running → destroy",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pipeline = TerminalPipeline.create({
      rows: TEST_ROWS,
      cols: TEST_COLS,
    });
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
    const pipeline = TerminalPipeline.create({
      rows: TEST_ROWS,
      cols: TEST_COLS,
    });

    // Subscribe before starting so we catch the first output
    const shellReady = waitForShellReady(pipeline);
    pipeline.start();
    await shellReady;

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
    const pipeline = TerminalPipeline.create({
      rows: TEST_ROWS,
      cols: TEST_COLS,
    });

    const shellReady = waitForShellReady(pipeline);
    pipeline.start();
    await shellReady;

    let resizeEvent: { rows: number; cols: number } | null = null;
    pipeline.eventBus.subscribe(EventType.GridResized, (event) => {
      resizeEvent = event as { rows: number; cols: number };
    });

    pipeline.resize(40, 120);

    // Event fires synchronously during resize()
    assert(resizeEvent !== null, "GridResized event should have fired");
    assertEquals(resizeEvent!.rows, 40);
    assertEquals(resizeEvent!.cols, 120);

    // Verify grid dimensions updated
    assertEquals(pipeline.grid.rows, 40);
    assertEquals(pipeline.grid.cols, 120);

    await pipeline.destroy();
  },
});

Deno.test({
  name: "Pipeline: event bus fires PtyOutput and GridUpdated during operation",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pipeline = TerminalPipeline.create({
      rows: TEST_ROWS,
      cols: TEST_COLS,
    });

    let ptyOutputCount = 0;
    let gridUpdatedCount = 0;

    pipeline.eventBus.subscribe(EventType.PtyOutput, () => {
      ptyOutputCount++;
    });
    pipeline.eventBus.subscribe(EventType.GridUpdated, () => {
      gridUpdatedCount++;
    });

    pipeline.start();

    // Wait for events by polling on the condition rather than a fixed sleep
    const deadline = Date.now() + 5000;
    while (
      Date.now() < deadline &&
      (ptyOutputCount === 0 || gridUpdatedCount === 0)
    ) {
      await new Promise((r) => setTimeout(r, 50));
    }

    // Shell startup alone should produce output
    assertGreater(ptyOutputCount, 0, "Should have received PtyOutput events");
    assertGreater(
      gridUpdatedCount,
      0,
      "Should have received GridUpdated events",
    );

    await pipeline.destroy();
  },
});

Deno.test({
  name: "Pipeline: clearDirty prevents dirty row accumulation",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pipeline = TerminalPipeline.create({
      rows: TEST_ROWS,
      cols: TEST_COLS,
    });

    const dirtyRowCounts: number[] = [];
    pipeline.eventBus.subscribe(EventType.GridUpdated, (event) => {
      const ev = event as { dirtyRows: number[] };
      dirtyRowCounts.push(ev.dirtyRows.length);
    });

    const shellReady = waitForShellReady(pipeline);
    pipeline.start();
    await shellReady;

    // Send first command and wait for grid to reflect it
    pipeline.write("echo first\n");
    await waitForGridContent(pipeline, "first");

    const countAfterFirst = dirtyRowCounts.length;

    // Send second command and wait for grid to reflect it
    pipeline.write("echo second\n");
    await waitForGridContent(pipeline, "second");

    // We must have received updates for both commands
    assertGreater(
      dirtyRowCounts.length,
      countAfterFirst,
      "Should have received GridUpdated events for 'echo second' — " +
        `only got ${dirtyRowCounts.length} total updates (${countAfterFirst} before second command)`,
    );

    // With clearDirty working, later updates should NOT accumulate
    // all previously dirty rows. Check that the last update reports
    // fewer dirty rows than the total grid height.
    const lastCount = dirtyRowCounts[dirtyRowCounts.length - 1]!;
    assert(
      lastCount < TEST_ROWS,
      `Last dirty row count (${lastCount}) should be less than total rows ` +
        `(${TEST_ROWS}) — clearDirty should prevent accumulation`,
    );

    await pipeline.destroy();
  },
});
