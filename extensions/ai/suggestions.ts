// extensions/ai/suggestions.ts
// Command suggestion engine — context-aware LLM suggestions triggered by keybinding.

import type { ExtensionContext } from "@marauder/extensions";
import type { LLMClient } from "./llm_client.ts";
import type { ContextAssembler } from "./context.ts";

const DEBOUNCE_MS = 500;
const SUGGESTION_COUNT = 3;

const SUGGESTION_PROMPT = `Based on the terminal context, suggest exactly ${SUGGESTION_COUNT} useful shell commands the user might want to run next. Return ONLY a JSON array of objects with "command" and "description" fields. No markdown, no explanation, just the JSON array.`;

export interface CommandSuggestion {
  command: string;
  description: string;
}

export class SuggestionEngine {
  #ctx: ExtensionContext;
  #llm: LLMClient;
  #assembler: ContextAssembler;
  #debounceTimer: number | undefined;
  #isRequesting = false;

  constructor(
    ctx: ExtensionContext,
    llm: LLMClient,
    assembler: ContextAssembler,
  ) {
    this.#ctx = ctx;
    this.#llm = llm;
    this.#assembler = assembler;
  }

  /** Trigger suggestion generation (debounced). */
  trigger(): void {
    if (this.#debounceTimer !== undefined) {
      clearTimeout(this.#debounceTimer);
    }
    this.#debounceTimer = setTimeout(() => {
      this.#debounceTimer = undefined;
      this.#generate();
    }, DEBOUNCE_MS) as unknown as number;
  }

  /** Cancel any pending suggestion request. */
  cancel(): void {
    if (this.#debounceTimer !== undefined) {
      clearTimeout(this.#debounceTimer);
      this.#debounceTimer = undefined;
    }
  }

  async #generate(): Promise<void> {
    if (this.#isRequesting) return;
    this.#isRequesting = true;

    try {
      const termCtx = this.#assembler.assembleContext();
      const systemMessages = this.#assembler.formatForLLM(termCtx);
      const messages = [
        ...systemMessages,
        { role: "user" as const, content: SUGGESTION_PROMPT },
      ];

      const result = await this.#llm.complete(messages, {
        temperature: 0.3,
        maxTokens: 512,
      });

      const suggestions = this.#parseSuggestions(result.content);

      this.#ctx.events.emit("ExtensionMessage", {
        source: "ai",
        type: "Suggestions",
        payload: { suggestions },
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      this.#ctx.events.emit("ExtensionMessage", {
        source: "ai",
        type: "SuggestionError",
        payload: { error: message },
      });
    } finally {
      this.#isRequesting = false;
    }
  }

  #parseSuggestions(content: string): CommandSuggestion[] {
    try {
      // Extract JSON array from content (may have surrounding text)
      const match = content.match(/\[[\s\S]*\]/);
      if (!match) return [];
      const parsed = JSON.parse(match[0]);
      if (!Array.isArray(parsed)) return [];
      return parsed
        .filter(
          (s: unknown): s is CommandSuggestion =>
            typeof s === "object" &&
            s !== null &&
            typeof (s as CommandSuggestion).command === "string" &&
            typeof (s as CommandSuggestion).description === "string",
        )
        .slice(0, SUGGESTION_COUNT);
    } catch {
      return [];
    }
  }
}
