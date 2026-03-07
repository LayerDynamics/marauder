// lib/extensions/installer_test.ts
// Tests for installer functions.

import { assertEquals } from "https://deno.land/std@0.224.0/assert/assert_equals.ts";
import { installFromPath } from "./installer.ts";

Deno.test("installFromPath: returns error for missing directory", async () => {
  const result = await installFromPath("/nonexistent/path/does/not/exist");
  assertEquals(result.success, false);
  assertEquals(result.error?.includes("No extension.json"), true);
});

Deno.test("installFromPath: returns error for invalid JSON", async () => {
  const tmpDir = await Deno.makeTempDir();
  try {
    await Deno.writeTextFile(`${tmpDir}/extension.json`, "not json{{{");
    const result = await installFromPath(tmpDir);
    assertEquals(result.success, false);
    assertEquals(result.error?.includes("Invalid JSON"), true);
  } finally {
    await Deno.remove(tmpDir, { recursive: true });
  }
});

Deno.test("installFromPath: returns error for invalid manifest (path traversal name)", async () => {
  const tmpDir = await Deno.makeTempDir();
  try {
    await Deno.writeTextFile(
      `${tmpDir}/extension.json`,
      JSON.stringify({ name: "../evil", version: "1.0.0", description: "test", entry: "mod.ts" }),
    );
    const result = await installFromPath(tmpDir);
    assertEquals(result.success, false);
    assertEquals(result.error?.includes("Invalid manifest"), true);
  } finally {
    await Deno.remove(tmpDir, { recursive: true });
  }
});
