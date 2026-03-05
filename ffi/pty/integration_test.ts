/**
 * Integration test: FFI opens PTY → writes `echo hello` → reads output.
 *
 * Validates the full Deno FFI → Rust cdylib → portable-pty chain.
 * Run with: deno test --allow-all --unstable-ffi ffi/pty/integration_test.ts
 */

import { assertEquals, assertGreater, assert } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { PtyManager } from "./mod.ts";

/** Sleep helper. */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Poll `pty.read()` until the accumulated output contains `target`,
 * or `timeoutMs` elapses. Returns all output collected.
 */
async function readUntil(
  pty: PtyManager,
  paneId: number | bigint,
  target: string,
  timeoutMs: number = 5000,
): Promise<string> {
  const decoder = new TextDecoder();
  let output = "";
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    try {
      const chunk = pty.read(paneId, 4096);
      if (chunk.length > 0) {
        output += decoder.decode(chunk);
        if (output.includes(target)) {
          return output;
        }
      }
    } catch (err: unknown) {
      const error: any = err;
      const code = error?.code;
      const message = typeof error?.message === "string" ? error.message : "";

      const isTransient =
        code === "EAGAIN" ||
        code === "EWOULDBLOCK" ||
        (typeof message === "string" &&
          (message.includes("EAGAIN") || message.includes("EWOULDBLOCK")));

      if (!isTransient) {
        // Unexpected failure: surface it so tests fail loudly instead of timing out.
        throw err;
      }

      // Transient “no data yet” error – ignore and let the loop retry.
    }
    await sleep(50);
  }

  return output;
}

Deno.test({
  name: "FFI PTY: create → write echo hello → read output → close",
  // Sanitizers disabled because Deno.dlopen resources and spawned PTY
  // processes do not participate in Deno's sanitizer tracking.
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    // 1. Create manager
    const pty = new PtyManager();
    assertEquals(pty.count(), 0, "Fresh manager should have 0 sessions");

    // 2. Open a PTY session
    const paneId = pty.create({ rows: 24, cols: 80 });
    assertGreater(
      Number(paneId),
      0,
      "create() should return a positive pane ID",
    );
    assertEquals(pty.count(), 1, "Should have 1 active session");

    // 3. Verify we got a child PID
    const pid = pty.getPid(paneId);
    assertGreater(pid, 0, "Child process should have a positive PID");

    // 4. Wait for shell prompt (give the shell time to start)
    await sleep(500);

    // 5. Write a command that produces known output
    const written = pty.write(paneId, "echo hello\n");
    assertGreater(written, 0, "write() should return bytes written");

    // 6. Read until we see "hello" in the output
    const output = await readUntil(pty, paneId, "hello");
    assert(
      output.includes("hello"),
      `Expected output to contain "hello", got: ${JSON.stringify(output)}`,
    );

    // 7. Verify child is still running
    assertEquals(pty.hasExited(paneId), false, "Shell should still be running");

    // 8. Close the session
    pty.close(paneId);
    assertEquals(pty.count(), 0, "Should have 0 sessions after close");

    // 9. Destroy the manager
    pty.destroy();
  },
});

Deno.test({
  name: "FFI PTY: resize works without error",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pty = new PtyManager();
    const paneId = pty.create({ rows: 24, cols: 80 });

    await sleep(200);

    // Resize should not throw
    pty.resize(paneId, 40, 120);

    // Verify the PTY is still functional after resize
    pty.write(paneId, "echo resized\n");
    const output = await readUntil(pty, paneId, "resized");
    assert(
      output.includes("resized"),
      `Expected "resized" in output after resize, got: ${JSON.stringify(output)}`,
    );

    pty.close(paneId);
    pty.destroy();
  },
});

Deno.test({
  name: "FFI PTY: multiple sessions are independent",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: async () => {
    const pty = new PtyManager();

    const pane1 = pty.create({ rows: 24, cols: 80 });
    const pane2 = pty.create({ rows: 24, cols: 80 });
    assertEquals(pty.count(), 2, "Should have 2 sessions");

    await sleep(500);

    // Write different commands to each
    pty.write(pane1, "echo pane_one\n");
    pty.write(pane2, "echo pane_two\n");

    const output1 = await readUntil(pty, pane1, "pane_one");
    const output2 = await readUntil(pty, pane2, "pane_two");

    assert(output1.includes("pane_one"), `Pane 1 should see "pane_one"`);
    assert(output2.includes("pane_two"), `Pane 2 should see "pane_two"`);

    // Close one, other should still work
    pty.close(pane1);
    assertEquals(pty.count(), 1);

    pty.write(pane2, "echo still_alive\n");
    const output3 = await readUntil(pty, pane2, "still_alive");
    assert(output3.includes("still_alive"));

    pty.close(pane2);
    assertEquals(pty.count(), 0);
    pty.destroy();
  },
});

Deno.test({
  name: "FFI PTY: destroy cleans up after double-destroy is safe",
  sanitizeOps: false,
  sanitizeResources: false,
  fn: () => {
    const pty = new PtyManager();
    pty.create({ rows: 24, cols: 80 });
    pty.destroy();
    // Second destroy should be a no-op, not a crash
    pty.destroy();
  },
});
