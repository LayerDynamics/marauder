// extensions/ai/errors.ts
// Error explanation — analyzes failed commands and provides fix suggestions.

import type { ExtensionContext } from "@marauder/extensions";
import type { LLMClient } from "./llm_client.ts";
import type { ContextAssembler } from "./context.ts";

const ERROR_PROMPT = `The user's terminal command just failed. Analyze the error and provide:
1. A brief one-line summary of what went wrong
2. A detailed explanation
3. A suggested fix command (if applicable)

Return ONLY a JSON object with fields: "summary", "explanation", "fixCommand" (string or null). No markdown wrapping.`;

export interface ErrorExplanation {
  summary: string;
  explanation: string;
  fixCommand: string | null;
}

export class ErrorExplainer {
  #ctx: ExtensionContext;
  #llm: LLMClient;
  #assembler: ContextAssembler;
  #autoExplain: boolean;
  readonly unsubscribers: Array<() => void> = [];

  constructor(
    ctx: ExtensionContext,
    llm: LLMClient,
    assembler: ContextAssembler,
  ) {
    this.#ctx = ctx;
    this.#llm = llm;
    this.#assembler = assembler;
    this.#autoExplain = ctx.config.get<boolean>("autoExplain") ?? false;

    // Auto-explain on command failure if enabled
    const unsub = ctx.events.on("ShellCommandFinished", (raw: unknown) => {
      const p = raw as { command: string; exitCode: number; output?: string };
      if (p.exitCode !== 0 && this.#autoExplain) {
        this.explain(p.command, p.exitCode, p.output);
      }
    });
    this.unsubscribers.push(unsub);
  }

  /** Set auto-explain mode. */
  setAutoExplain(enabled: boolean): void {
    this.#autoExplain = enabled;
    this.#ctx.config.set("autoExplain", enabled);
  }

  /** Explain the most recent failed command, or a specific one. */
  async explain(
    command?: string,
    exitCode?: number,
    output?: string,
  ): Promise<void> {
    const termCtx = this.#assembler.assembleContext();

    // Use provided args or fall back to last failed command from context
    const targetCommand = command ?? this.#lastFailedCommand(termCtx);
    const targetExitCode = exitCode ?? termCtx.lastExitCode;

    if (!targetCommand || targetExitCode === 0) {
      this.#ctx.notifications.show(
        "AI Explain",
        "No failed command to explain.",
      );
      return;
    }

    this.#ctx.events.emit("ExtensionMessage", {
      source: "ai",
      type: "ExplainStarted",
      payload: { command: targetCommand },
    });

    try {
      const systemMessages = this.#assembler.formatForLLM(termCtx);
      const userContent = [
        `Failed command: ${targetCommand}`,
        `Exit code: ${targetExitCode}`,
      ];
      if (output) {
        userContent.push(`Output:\n${output}`);
      }
      userContent.push("", ERROR_PROMPT);

      const result = await this.#llm.complete(
        [...systemMessages, { role: "user", content: userContent.join("\n") }],
        { temperature: 0.2, maxTokens: 1024 },
      );

      const explanation = this.#parseExplanation(result.content);

      // Show notification with summary
      this.#ctx.notifications.show("AI Explain", explanation.summary);

      // Emit full explanation for chat panel
      this.#ctx.events.emit("ExtensionMessage", {
        source: "ai",
        type: "ErrorExplanation",
        payload: {
          command: targetCommand,
          exitCode: targetExitCode,
          ...explanation,
        },
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      this.#ctx.notifications.show("AI Explain Error", message);
      this.#ctx.events.emit("ExtensionMessage", {
        source: "ai",
        type: "ExplainError",
        payload: { error: message },
      });
    }
  }

  #lastFailedCommand(
    termCtx: ReturnType<ContextAssembler["assembleContext"]>,
  ): string | undefined {
    for (let i = termCtx.lastCommands.length - 1; i >= 0; i--) {
      if (termCtx.lastCommands[i].exitCode !== 0) {
        return termCtx.lastCommands[i].command;
      }
    }
    return undefined;
  }

  #parseExplanation(content: string): ErrorExplanation {
    try {
      const match = content.match(/\{[\s\S]*\}/);
      if (!match) {
        return { summary: content.slice(0, 100), explanation: content, fixCommand: null };
      }
      const parsed = JSON.parse(match[0]);
      return {
        summary: typeof parsed.summary === "string" ? parsed.summary : "Error analyzed",
        explanation: typeof parsed.explanation === "string" ? parsed.explanation : content,
        fixCommand: typeof parsed.fixCommand === "string" ? parsed.fixCommand : null,
      };
    } catch {
      return { summary: content.slice(0, 100), explanation: content, fixCommand: null };
    }
  }
}
