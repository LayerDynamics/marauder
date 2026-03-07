// extensions/status-bar/mod.ts
// Status bar segments: cwd (left), git branch (center), pane id (right).

import type { ExtensionContext } from "@marauder/extensions";

interface CwdChangedPayload {
  cwd: string;
  paneId?: string;
}

interface PaneFocusedPayload {
  paneId: string;
}

interface GitInfoPayload {
  branch: string;
  dirty: number;
  ahead: number;
  behind: number;
}

interface ExtensionMessagePayload {
  source?: string;
  type?: string;
  payload?: unknown;
}

/** Format a path for display: replace $HOME with ~ and truncate if too long. */
function formatCwd(cwd: string): string {
  const home = Deno.env.get("HOME") ?? "";
  let display = home.length > 0 ? cwd.replace(home, "~") : cwd;
  // Truncate very long paths to last 3 segments.
  const parts = display.split("/");
  if (parts.length > 4) {
    display = "…/" + parts.slice(-3).join("/");
  }
  return display;
}

/** Format git branch + status for the center segment. */
function formatGitInfo(info: GitInfoPayload): string {
  let text = ` ${info.branch}`;
  if (info.dirty > 0) {
    text += ` *${info.dirty}`;
  }
  if (info.ahead > 0) {
    text += ` ↑${info.ahead}`;
  }
  if (info.behind > 0) {
    text += ` ↓${info.behind}`;
  }
  return text;
}

const _unsubscribers: Array<() => void> = [];

export function activate(ctx: ExtensionContext): void {
  // Initial placeholder values so the bar is never blank.
  ctx.statusBar.set("left", formatCwd(Deno.cwd()));
  ctx.statusBar.set("center", "");
  ctx.statusBar.set("right", "");

  // Left segment: current working directory.
  const unsubCwd = ctx.events.on("ShellCwdChanged", (raw: unknown) => {
    const payload = raw as CwdChangedPayload;
    ctx.statusBar.set("left", formatCwd(payload.cwd));
  });
  _unsubscribers.push(unsubCwd);

  // Right segment: focused pane identifier.
  const unsubPane = ctx.events.on("PaneFocused", (raw: unknown) => {
    const payload = raw as PaneFocusedPayload;
    ctx.statusBar.set("right", `pane:${payload.paneId}`);
  });
  _unsubscribers.push(unsubPane);

  // Center segment: git info forwarded from the git-integration extension.
  const unsubGit = ctx.events.on("ExtensionMessage", (raw: unknown) => {
    const msg = raw as ExtensionMessagePayload;
    if (msg.source === "git-integration" && msg.type === "GitStatus") {
      const info = msg.payload as GitInfoPayload;
      ctx.statusBar.set("center", formatGitInfo(info));
    }
  });
  _unsubscribers.push(unsubGit);
}

export function deactivate(): void {
  for (const unsub of _unsubscribers) {
    unsub();
  }
  _unsubscribers.length = 0;
}
