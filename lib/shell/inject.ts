/**
 * @marauder/shell — Shell integration auto-injection
 *
 * Detects the user's shell and injects Marauder's shell integration
 * scripts into the appropriate RC file.
 */

import { Logger } from "@marauder/dev";

const log = new Logger("shell-inject");

type ShellType = "zsh" | "bash" | "fish";

const RC_FILES: Record<ShellType, string> = {
  zsh: ".zshrc",
  bash: ".bashrc",
  fish: ".config/fish/config.fish",
};

const MARKER = "# Marauder shell integration";

/** Detect shell type from binary path. */
export function detectShell(shellPath: string): ShellType | null {
  const name = shellPath.split("/").pop()?.toLowerCase();
  if (!name) return null;
  if (name === "zsh") return "zsh";
  if (name === "bash") return "bash";
  if (name === "fish") return "fish";
  return null;
}

/** Get path to the integration script for a given shell. */
export function getIntegrationScript(shell: ShellType): string {
  // Resolve to absolute path: use MARAUDER_APP_DIR if set (Tauri bundle),
  // otherwise resolve relative to the current working directory.
  const appDir = Deno.env.get("MARAUDER_APP_DIR") ?? Deno.cwd();
  return `${appDir}/resources/shell-integrations/marauder.${shell}`;
}

/** Get the full path to the RC file for the given shell. */
function getRcPath(shell: ShellType): string {
  const home = Deno.env.get("HOME") ?? Deno.env.get("USERPROFILE") ?? "~";
  return `${home}/${RC_FILES[shell]}`;
}

/** Check if the integration marker already exists in the RC file. */
export async function isInjected(shell: ShellType): Promise<boolean> {
  const rcPath = getRcPath(shell);
  try {
    const content = await Deno.readTextFile(rcPath);
    return content.includes(MARKER);
  } catch (err) {
    if (err instanceof Deno.errors.NotFound) {
      return false;
    }
    log.warn(`Failed to read RC file ${rcPath}: ${err}`);
    return false;
  }
}

/**
 * Inject shell integration into the user's RC file.
 * Idempotent — skips if marker already present.
 * Returns true if injection was performed, false if already injected.
 */
export async function injectShellIntegration(shellPath: string): Promise<boolean> {
  const shell = detectShell(shellPath);
  if (!shell) return false;

  if (await isInjected(shell)) return false;

  const rcPath = getRcPath(shell);
  const scriptPath = getIntegrationScript(shell);

  const sourceLine = shell === "fish"
    ? `\n${MARKER}\nsource ${scriptPath}\n`
    : `\n${MARKER}\n[ -f "${scriptPath}" ] && source "${scriptPath}"\n`;

  try {
    let existing = "";
    try {
      existing = await Deno.readTextFile(rcPath);
    } catch (readErr) {
      if (!(readErr instanceof Deno.errors.NotFound)) {
        log.warn(`Failed to read existing RC file ${rcPath}: ${readErr}`);
      }
    }
    await Deno.writeTextFile(rcPath, existing + sourceLine);
    return true;
  } catch (err) {
    log.error(`Failed to inject shell integration into ${rcPath}: ${err}`);
    return false;
  }
}
