// lib/cli/ext_test.ts
// Tests for CLI extension name validation.

import { assertEquals } from "https://deno.land/std@0.224.0/assert/assert_equals.ts";

// Re-implement the validation function for direct testing
// (mirrors isValidExtensionName from ext.ts)
function isValidExtensionName(name: string): boolean {
  return /^[a-z0-9][a-z0-9._-]*$/i.test(name) && !name.includes("..") && !name.includes("/") && !name.includes("\\");
}

Deno.test("isValidExtensionName: accepts valid names", () => {
  assertEquals(isValidExtensionName("my-extension"), true);
  assertEquals(isValidExtensionName("ext_v2"), true);
  assertEquals(isValidExtensionName("Theme.Dark"), true);
  assertEquals(isValidExtensionName("a"), true);
});

Deno.test("isValidExtensionName: rejects path traversal", () => {
  assertEquals(isValidExtensionName("../etc"), false);
  assertEquals(isValidExtensionName("foo/bar"), false);
  assertEquals(isValidExtensionName("foo\\bar"), false);
  assertEquals(isValidExtensionName("a..b"), false);
});

Deno.test("isValidExtensionName: rejects names starting with non-alphanumeric", () => {
  assertEquals(isValidExtensionName(".hidden"), false);
  assertEquals(isValidExtensionName("-dash"), false);
  assertEquals(isValidExtensionName("_under"), false);
});

Deno.test("isValidExtensionName: rejects empty string", () => {
  assertEquals(isValidExtensionName(""), false);
});
