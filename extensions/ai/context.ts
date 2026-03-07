// extensions/ai/context.ts
// Terminal context assembler — maintains rolling state from shell events.

import type { ExtensionContext } from "@marauder/extensions";
import type { ChatMessage } from "./llm_client.ts";

/** Snapshot of the terminal's current state for LLM context. */
export interface TerminalContext {
  cwd: string;
  lastCommands: CommandRecord[];
  lastExitCode: number;
  recentOutput: string[];
  gitBranch: string;
  envVars: Record<string, string>;
}

/** A recorded command with its result. */
export interface CommandRecord {
  command: string;
  exitCode: number;
  durationMs: number;
}

const MAX_COMMANDS = 10;
const MAX_OUTPUT_LINES = 50;
const TOKEN_BUDGET = 4096;
const CHARS_PER_TOKEN = 4; // rough estimate

/** Filtered env var prefixes that are safe/useful to share. */
const ENV_PREFIXES = [
  "SHELL",
  "TERM",
  "LANG",
  "HOME",
  "USER",
  "PATH",
  "EDITOR",
  "NODE_ENV",
  "VIRTUAL_ENV",
  "CONDA_DEFAULT_ENV",
];

export class ContextAssembler {
  #cwd = "";
  #lastExitCode = 0;
  #commands: CommandRecord[] = [];
  #outputLines: string[] = [];
  #gitBranch = "";
  readonly unsubscribers: Array<() => void> = [];

  constructor(ctx: ExtensionContext) {
    const unsubFinished = ctx.events.on("ShellCommandFinished", (raw: unknown) => {
      const p = raw as { command: string; exitCode: number; duration: number; output?: string };
      this.#lastExitCode = p.exitCode;
      this.#commands.push({
        command: p.command,
        exitCode: p.exitCode,
        durationMs: p.duration,
      });
      if (this.#commands.length > MAX_COMMANDS) {
        this.#commands.shift();
      }
      if (typeof p.output === "string") {
        const lines = p.output.split("\n");
        this.#outputLines.push(...lines);
        while (this.#outputLines.length > MAX_OUTPUT_LINES) {
          this.#outputLines.shift();
        }
      }
    });
    this.unsubscribers.push(unsubFinished);

    const unsubCwd = ctx.events.on("ShellCwdChanged", (raw: unknown) => {
      const p = raw as { cwd: string };
      this.#cwd = p.cwd;
    });
    this.unsubscribers.push(unsubCwd);

    const unsubOutput = ctx.events.on("ExtensionMessage", (raw: unknown) => {
      const msg = raw as { source?: string; type?: string; payload?: unknown };
      if (msg.source === "ai" && msg.type === "TerminalOutput") {
        const p = msg.payload as { lines: string[] };
        this.#outputLines.push(...p.lines);
        while (this.#outputLines.length > MAX_OUTPUT_LINES) {
          this.#outputLines.shift();
        }
      }
    });
    this.unsubscribers.push(unsubOutput);

    // Try to detect initial CWD
    this.#refreshGitBranch();
  }

  /** Assemble the current terminal context snapshot. */
  assembleContext(): TerminalContext {
    this.#refreshGitBranch();
    return {
      cwd: this.#cwd || this.#detectCwd(),
      lastCommands: [...this.#commands],
      lastExitCode: this.#lastExitCode,
      recentOutput: [...this.#outputLines],
      gitBranch: this.#gitBranch,
      envVars: this.#filteredEnv(),
    };
  }

  /** Convert terminal context into LLM system messages, respecting token budget. */
  formatForLLM(ctx: TerminalContext): ChatMessage[] {
    const parts: string[] = [
      "You are an AI assistant embedded in the Marauder terminal emulator.",
      `Current working directory: ${ctx.cwd}`,
    ];

    if (ctx.gitBranch) {
      parts.push(`Git branch: ${ctx.gitBranch}`);
    }

    if (ctx.lastCommands.length > 0) {
      parts.push("\nRecent commands:");
      for (const cmd of ctx.lastCommands) {
        const status = cmd.exitCode === 0 ? "OK" : `FAILED(${cmd.exitCode})`;
        parts.push(`  $ ${cmd.command}  [${status}, ${cmd.durationMs}ms]`);
      }
    }

    if (ctx.recentOutput.length > 0) {
      parts.push("\nRecent terminal output (newest last):");
      // Truncate output to fit token budget
      let outputText = ctx.recentOutput.join("\n");
      const headerChars = parts.join("\n").length;
      const remainingChars = TOKEN_BUDGET * CHARS_PER_TOKEN - headerChars - 200;
      if (outputText.length > remainingChars) {
        outputText = "...(truncated)\n" + outputText.slice(-remainingChars);
      }
      parts.push(outputText);
    }

    const envEntries = Object.entries(ctx.envVars);
    if (envEntries.length > 0) {
      parts.push("\nEnvironment:");
      for (const [k, v] of envEntries) {
        parts.push(`  ${k}=${v}`);
      }
    }

    return [{ role: "system", content: parts.join("\n") }];
  }

  #detectCwd(): string {
    try {
      return Deno.cwd();
    } catch {
      return "";
    }
  }

  #refreshGitBranch(): void {
    try {
      const cmd = new Deno.Command("git", {
        args: ["rev-parse", "--abbrev-ref", "HEAD"],
        stdout: "piped",
        stderr: "null",
      });
      const result = cmd.outputSync();
      if (result.success) {
        this.#gitBranch = new TextDecoder().decode(result.stdout).trim();
      }
    } catch {
      // git not available or not in a repo
    }
  }

  #filteredEnv(): Record<string, string> {
    const result: Record<string, string> = {};
    for (const prefix of ENV_PREFIXES) {
      const val = Deno.env.get(prefix);
      if (val !== undefined) {
        // Truncate PATH to avoid huge values
        result[prefix] = prefix === "PATH" ? val.slice(0, 200) + "..." : val;
      }
    }
    return result;
  }
}
