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
  stash: number;
  state: string;
}

interface ExtensionMessagePayload {
  source?: string;
  type?: string;
  payload?: unknown;
}

/** Format a path for display: replace $HOME with ~ and truncate if too long. */
function formatCwd(cwd: string): string {
  const home = Deno.env.get("HOME") ?? "";
  let display = home.length > 0 && cwd.startsWith(home)
    ? "~" + cwd.slice(home.length)
    : cwd;
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
  if (info.state) {
    text += ` (${info.state})`;
  }
  if (info.dirty > 0) {
    text += ` *${info.dirty}`;
  }
  if (info.ahead > 0) {
    text += ` ↑${info.ahead}`;
  }
  if (info.behind > 0) {
    text += ` ↓${info.behind}`;
  }
  if (info.stash > 0) {
    text += ` ⚑${info.stash}`;
  }
  return text;
}

/** Format current time as HH:MM. */
function formatTime(): string {
  const now = new Date();
  const h = now.getHours().toString().padStart(2, "0");
  const m = now.getMinutes().toString().padStart(2, "0");
  return `${h}:${m}`;
}

/** Query battery percentage on macOS via pmset. Returns empty string on non-macOS or failure. */
async function getBattery(): Promise<string> {
  if (Deno.build.os !== "darwin") return "";
  try {
    const cmd = new Deno.Command("pmset", {
      args: ["-g", "batt"],
      stdout: "piped",
      stderr: "null",
    });
    const { code, stdout } = await cmd.output();
    if (code !== 0) return "";
    const text = new TextDecoder().decode(stdout);
    const match = text.match(/(\d+)%/);
    return match ? `${match[1]}%` : "";
  } catch {
    return "";
  }
}

const _unsubscribers: Array<() => void> = [];
let _clockInterval: ReturnType<typeof setInterval> | null = null;
let _currentPaneId = "";
let _lastBattery = "";

export function activate(ctx: ExtensionContext): void {
  // Clean up any previous activation to prevent leaked intervals/subscriptions.
  if (_unsubscribers.length > 0 || _clockInterval !== null) {
    deactivate();
  }

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

  // Right segment: focused pane identifier + time + cached battery.
  const updateRightSegment = (): void => {
    const parts: string[] = [formatTime()];
    if (_lastBattery) parts.push(_lastBattery);
    if (_currentPaneId) parts.push(`pane:${_currentPaneId}`);
    ctx.statusBar.set("right", parts.join(" | "));
  };

  // Async battery fetch refreshes the cache then updates the segment.
  const updateRightWithBattery = async (): Promise<void> => {
    const paneId = _currentPaneId;
    _lastBattery = await getBattery();
    const parts: string[] = [formatTime()];
    if (_lastBattery) parts.push(_lastBattery);
    if (paneId) parts.push(`pane:${paneId}`);
    ctx.statusBar.set("right", parts.join(" | "));
  };

  const unsubPane = ctx.events.on("PaneFocused", (raw: unknown) => {
    const payload = raw as PaneFocusedPayload;
    _currentPaneId = payload.paneId;
    updateRightSegment();
  });
  _unsubscribers.push(unsubPane);

  // Clock update every 30 seconds
  updateRightWithBattery();
  _clockInterval = setInterval(() => {
    updateRightWithBattery();
  }, 30_000);

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
  if (_clockInterval !== null) {
    clearInterval(_clockInterval);
    _clockInterval = null;
  }
  _currentPaneId = "";
  _lastBattery = "";
}
