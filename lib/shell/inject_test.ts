/**
 * Unit tests for shell injection detection and path resolution.
 * Run with: deno test lib/shell/inject_test.ts
 */

import {
  assertEquals,
  assert,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import { detectShell, getIntegrationScript } from "./inject.ts";

Deno.test("detectShell identifies zsh", () => {
  assertEquals(detectShell("/bin/zsh"), "zsh");
  assertEquals(detectShell("/usr/local/bin/zsh"), "zsh");
});

Deno.test("detectShell identifies bash", () => {
  assertEquals(detectShell("/bin/bash"), "bash");
  assertEquals(detectShell("/usr/bin/bash"), "bash");
});

Deno.test("detectShell identifies fish", () => {
  assertEquals(detectShell("/usr/bin/fish"), "fish");
});

Deno.test("detectShell returns null for unknown shells", () => {
  assertEquals(detectShell("/bin/sh"), null);
  assertEquals(detectShell("/usr/bin/csh"), null);
  assertEquals(detectShell(""), null);
});

Deno.test("getIntegrationScript returns absolute path", () => {
  const path = getIntegrationScript("zsh");
  // Should be an absolute path (starts with /)
  assert(path.startsWith("/"), `Expected absolute path, got: ${path}`);
  assert(path.endsWith("resources/shell-integrations/marauder.zsh"));
});

Deno.test("getIntegrationScript respects MARAUDER_APP_DIR", () => {
  const original = Deno.env.get("MARAUDER_APP_DIR");
  try {
    Deno.env.set("MARAUDER_APP_DIR", "/Applications/Marauder.app/Contents/Resources");
    const path = getIntegrationScript("bash");
    assertEquals(path, "/Applications/Marauder.app/Contents/Resources/resources/shell-integrations/marauder.bash");
  } finally {
    if (original) {
      Deno.env.set("MARAUDER_APP_DIR", original);
    } else {
      Deno.env.delete("MARAUDER_APP_DIR");
    }
  }
});
