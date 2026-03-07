// lib/extensions/loader_test.ts
// Tests for validateManifest and isCompatibleVersion.

import { assertEquals } from "https://deno.land/std@0.224.0/assert/assert_equals.ts";
import { assertNotEquals } from "https://deno.land/std@0.224.0/assert/assert_not_equals.ts";
import { validateManifest, isCompatibleVersion } from "./loader.ts";

// --- validateManifest ---

Deno.test("validateManifest: accepts valid manifest", () => {
  const result = validateManifest({
    name: "my-ext",
    version: "1.0.0",
    description: "A test extension",
    entry: "mod.ts",
  });
  assertNotEquals(result, null);
  assertEquals(result!.name, "my-ext");
});

Deno.test("validateManifest: rejects missing required fields", () => {
  assertEquals(validateManifest({ name: "x" }), null);
  assertEquals(validateManifest({ name: "x", version: "1", description: "d" }), null);
});

Deno.test("validateManifest: rejects name with path separators", () => {
  const base = { version: "1.0.0", description: "test", entry: "mod.ts" };
  assertEquals(validateManifest({ ...base, name: "../evil" }), null);
  assertEquals(validateManifest({ ...base, name: "foo/bar" }), null);
  assertEquals(validateManifest({ ...base, name: "foo\\bar" }), null);
  assertEquals(validateManifest({ ...base, name: ".hidden" }), null);
  assertEquals(validateManifest({ ...base, name: "a..b" }), null);
});

Deno.test("validateManifest: rejects entry with path traversal", () => {
  const base = { name: "good", version: "1.0.0", description: "test" };
  assertEquals(validateManifest({ ...base, entry: "../etc/passwd" }), null);
  assertEquals(validateManifest({ ...base, entry: "/absolute/path.ts" }), null);
  assertEquals(validateManifest({ ...base, entry: "\\windows\\path.ts" }), null);
});

Deno.test("validateManifest: accepts valid nested entry", () => {
  const result = validateManifest({
    name: "my-ext",
    version: "1.0.0",
    description: "test",
    entry: "src/mod.ts",
  });
  assertNotEquals(result, null);
});

// --- isCompatibleVersion ---

Deno.test("isCompatibleVersion: exact match", () => {
  assertEquals(isCompatibleVersion("1.0.0", "1.0.0"), true);
  assertEquals(isCompatibleVersion("1.0.0", "1.0.1"), false);
});

Deno.test("isCompatibleVersion: caret range", () => {
  assertEquals(isCompatibleVersion("^1.0.0", "1.0.0"), true);
  assertEquals(isCompatibleVersion("^1.0.0", "1.5.0"), true);
  assertEquals(isCompatibleVersion("^1.0.0", "2.0.0"), false);
  assertEquals(isCompatibleVersion("^1.2.0", "1.1.0"), false);
});

Deno.test("isCompatibleVersion: tilde range", () => {
  assertEquals(isCompatibleVersion("~1.2.0", "1.2.5"), true);
  assertEquals(isCompatibleVersion("~1.2.0", "1.3.0"), false);
});

Deno.test("isCompatibleVersion: rejects non-numeric versions", () => {
  assertEquals(isCompatibleVersion("not.a.version", "1.0.0"), false);
  assertEquals(isCompatibleVersion("1.0.0", "not.a.version"), false);
  assertEquals(isCompatibleVersion("^abc", "1.0.0"), false);
});

Deno.test("isCompatibleVersion: rejects negative numbers", () => {
  assertEquals(isCompatibleVersion("-1.0.0", "1.0.0"), false);
  assertEquals(isCompatibleVersion("1.-1.0", "1.0.0"), false);
});

// --- empty string validation ---

Deno.test("validateManifest: rejects empty name", () => {
  assertEquals(validateManifest({ name: "", version: "1.0.0", description: "test", entry: "mod.ts" }), null);
});

Deno.test("validateManifest: rejects empty version", () => {
  assertEquals(validateManifest({ name: "ext", version: "", description: "test", entry: "mod.ts" }), null);
});

Deno.test("validateManifest: rejects empty entry", () => {
  assertEquals(validateManifest({ name: "ext", version: "1.0.0", description: "test", entry: "" }), null);
});

Deno.test("validateManifest: allows empty description", () => {
  const result = validateManifest({ name: "ext", version: "1.0.0", description: "", entry: "mod.ts" });
  assertNotEquals(result, null);
});
