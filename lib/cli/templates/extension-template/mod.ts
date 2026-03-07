// {{name}} — Marauder Extension
// {{description}}

import type { ExtensionContext } from "@marauder/extensions";

const _unsubscribers: Array<() => void> = [];

export function activate(ctx: ExtensionContext): void {
  console.log("[{{name}}] activated");

  // Register commands
  ctx.commands.register("{{name}}.hello", () => {
    ctx.notifications.show("{{name}}", "Hello from {{name}}!");
  });

  // Subscribe to events
  const unsub = ctx.events.on("TerminalOutput", (_payload) => {
    // Handle terminal output events
  });
  _unsubscribers.push(unsub);
}

export function deactivate(): void {
  for (const unsub of _unsubscribers) {
    unsub();
  }
  _unsubscribers.length = 0;
  console.log("[{{name}}] deactivated");
}
