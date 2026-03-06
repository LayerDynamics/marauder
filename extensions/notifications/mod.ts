// extensions/notifications/mod.ts
// Desktop notifications for long-running shell commands.

interface ExtensionConfig {
  get<T>(key: string): T | undefined;
  set(key: string, value: unknown): void;
}

interface ExtensionEvents {
  on(type: string, handler: (payload: unknown) => void): () => void;
  emit(type: string, payload: unknown): void;
}

interface ExtensionStatusBar {
  set(segment: "left" | "center" | "right", text: string): void;
}

interface ExtensionNotifications {
  show(title: string, body?: string): void;
}

interface ExtensionCommands {
  register(id: string, handler: () => void): void;
}

interface ExtensionKeybindings {
  register(keys: string, commandId: string): void;
}

interface ExtensionContext {
  config: ExtensionConfig;
  events: ExtensionEvents;
  statusBar: ExtensionStatusBar;
  notifications: ExtensionNotifications;
  commands: ExtensionCommands;
  keybindings: ExtensionKeybindings;
}

interface ShellCommandFinishedPayload {
  /** The command string that was executed. */
  command: string;
  /** Exit code of the process. */
  exitCode: number;
  /** Duration of the command in milliseconds. */
  durationMs: number;
  /** Pane that ran the command, if available. */
  paneId?: string;
}

/** Default threshold in seconds before a notification is shown. */
const DEFAULT_THRESHOLD_SECONDS = 10;

/**
 * Request notification permission from the browser Notification API if it has
 * not already been granted.  Returns true when notifications may be shown.
 */
async function ensurePermission(): Promise<boolean> {
  // deno-lint-ignore no-explicit-any
  const g = globalThis as any;
  if (!g.Notification) return false;
  if (g.Notification.permission === "granted") return true;
  if (g.Notification.permission === "denied") return false;
  const result = await g.Notification.requestPermission();
  return result === "granted";
}

/** Show a native desktop notification.  Falls back to ctx.notifications when
 *  the Notification API is unavailable or permission is not granted. */
async function showDesktopNotification(
  ctx: ExtensionContext,
  title: string,
  body: string,
): Promise<void> {
  const permitted = await ensurePermission();
  if (permitted) {
    // deno-lint-ignore no-explicit-any
    const NotificationCtor = (globalThis as any).Notification;
    new NotificationCtor(title, { body, silent: false });
  } else {
    // Fallback to the runtime notification bridge.
    ctx.notifications.show(title, body);
  }
}

/** Format milliseconds as a human-readable duration string. */
function formatDuration(ms: number): string {
  const totalSeconds = Math.round(ms / 1000);
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
}

const _unsubscribers: Array<() => void> = [];

export function activate(ctx: ExtensionContext): void {
  const unsub = ctx.events.on("ShellCommandFinished", (raw: unknown) => {
    const payload = raw as ShellCommandFinishedPayload;

    const thresholdSeconds =
      ctx.config.get<number>("notifications.threshold") ??
      DEFAULT_THRESHOLD_SECONDS;
    const thresholdMs = thresholdSeconds * 1000;

    if (payload.durationMs < thresholdMs) return;

    const duration = formatDuration(payload.durationMs);
    const exitLabel = payload.exitCode === 0 ? "completed" : "failed";
    const title = `Command ${exitLabel} (${duration})`;

    // Truncate very long command strings for readability.
    const maxCmdLen = 80;
    const cmd = payload.command.length > maxCmdLen
      ? payload.command.slice(0, maxCmdLen - 1) + "…"
      : payload.command;

    const body = `${cmd}\nExit code: ${payload.exitCode}`;

    // Fire-and-forget; permission errors are handled inside.
    showDesktopNotification(ctx, title, body);
  });

  _unsubscribers.push(unsub);
}

export function deactivate(): void {
  for (const unsub of _unsubscribers) {
    unsub();
  }
  _unsubscribers.length = 0;
}
