// extensions/git-integration/mod.ts
// Runs git commands when the shell cwd changes and emits branch/status info.

import type { ExtensionContext } from "@marauder/extensions";

interface CwdChangedPayload {
  cwd: string;
  paneId?: string;
}

interface GitStatus {
  branch: string;
  dirty: number;
  ahead: number;
  behind: number;
}

/** Run a subprocess and return its stdout as a trimmed string.
 *  Returns null if the process exits non-zero or throws. */
async function runGit(args: string[], cwd: string): Promise<string | null> {
  try {
    const cmd = new Deno.Command("git", {
      args,
      cwd,
      stdout: "piped",
      stderr: "null",
    });
    const { code, stdout } = await cmd.output();
    if (code !== 0) return null;
    return new TextDecoder().decode(stdout).trim();
  } catch {
    return null;
  }
}

/** Parse `git status --porcelain` output into a count of modified files. */
function parsePorcelain(output: string): number {
  if (output.length === 0) return 0;
  return output.split("\n").filter((line) => line.trim().length > 0).length;
}

/** Parse ahead/behind counts from `git rev-list --count --left-right @{u}...HEAD`. */
function parseAheadBehind(output: string): { ahead: number; behind: number } {
  const parts = output.split(/\s+/);
  if (parts.length < 2) return { ahead: 0, behind: 0 };
  return {
    behind: parseInt(parts[0] ?? "0", 10) || 0,
    ahead: parseInt(parts[1] ?? "0", 10) || 0,
  };
}

async function fetchGitStatus(cwd: string): Promise<GitStatus | null> {
  // Verify this is actually a git repo before running further commands.
  const rootCheck = await runGit(["rev-parse", "--git-dir"], cwd);
  if (rootCheck === null) return null;

  const branch = await runGit(["rev-parse", "--abbrev-ref", "HEAD"], cwd);
  if (branch === null) return null;

  const porcelainOutput = await runGit(["status", "--porcelain"], cwd);
  const dirty = porcelainOutput !== null ? parsePorcelain(porcelainOutput) : 0;

  const abOutput = await runGit(
    ["rev-list", "--count", "--left-right", "@{u}...HEAD"],
    cwd,
  );
  const { ahead, behind } = abOutput !== null
    ? parseAheadBehind(abOutput)
    : { ahead: 0, behind: 0 };

  return { branch, dirty, ahead, behind };
}

const _unsubscribers: Array<() => void> = [];

export function activate(ctx: ExtensionContext): void {
  const unsub = ctx.events.on("ShellCwdChanged", (raw: unknown) => {
    const payload = raw as CwdChangedPayload;
    // Fire-and-forget; errors are swallowed by fetchGitStatus.
    fetchGitStatus(payload.cwd).then((status) => {
      if (status === null) return; // Not a git directory — nothing to emit.
      ctx.events.emit("ExtensionMessage", {
        source: "git-integration",
        type: "GitStatus",
        payload: status,
      });
    });
  });

  _unsubscribers.push(unsub);

  // Probe the initial cwd on activation.
  fetchGitStatus(Deno.cwd()).then((status) => {
    if (status === null) return;
    ctx.events.emit("ExtensionMessage", {
      source: "git-integration",
      type: "GitStatus",
      payload: status,
    });
  });
}

export function deactivate(): void {
  for (const unsub of _unsubscribers) {
    unsub();
  }
  _unsubscribers.length = 0;
}
