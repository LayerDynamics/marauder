// extensions/ai/mod.ts
// AI extension entry point — LLM-powered suggestions, error explanation, chat, and agent mode.

import type { ExtensionContext } from "@marauder/extensions";
import { LLMClient } from "./llm_client.ts";
import { ContextAssembler } from "./context.ts";
import { SuggestionEngine } from "./suggestions.ts";
import { ErrorExplainer } from "./errors.ts";
import { ChatPanel } from "./chat.ts";
import { AgentRunner } from "./agent.ts";

const _unsubscribers: Array<() => void> = [];

let _llm: LLMClient | null = null;
let _assembler: ContextAssembler | null = null;
let _suggestions: SuggestionEngine | null = null;
let _errors: ErrorExplainer | null = null;
let _chat: ChatPanel | null = null;
let _agent: AgentRunner | null = null;

export function activate(ctx: ExtensionContext): void {
  // Initialize LLM client from extension config
  _llm = new LLMClient({
    endpoint: ctx.config.get<string>("endpoint") ?? "https://api.openai.com",
    apiKey: ctx.config.get<string>("apiKey") ?? "",
    model: ctx.config.get<string>("model") ?? "gpt-4o",
  });

  // Context assembler — subscribes to shell events
  _assembler = new ContextAssembler(ctx);
  _unsubscribers.push(..._assembler.unsubscribers);

  // Suggestion engine
  _suggestions = new SuggestionEngine(ctx, _llm, _assembler);

  // Error explainer — subscribes to ShellCommandFinished
  _errors = new ErrorExplainer(ctx, _llm, _assembler);
  _unsubscribers.push(..._errors.unsubscribers);

  // Chat panel
  _chat = new ChatPanel(ctx, _llm, _assembler);
  _unsubscribers.push(..._chat.unsubscribers);

  // Agent runner
  _agent = new AgentRunner(ctx, _llm, _assembler);
  _unsubscribers.push(..._agent.unsubscribers);

  // ── Commands ──────────────────────────────────────────────────────────

  ctx.commands.register("ai.chat.toggle", () => {
    _chat?.toggle();
  });

  ctx.commands.register("ai.suggest", () => {
    _suggestions?.trigger();
  });

  ctx.commands.register("ai.explain", () => {
    _errors?.explain();
  });

  ctx.commands.register("ai.agent.start", () => {
    // Default goal prompt — real goal comes from chat panel /agent command
    _agent?.start("Help the user with their current task");
  });

  ctx.commands.register("ai.agent.stop", () => {
    _agent?.stop();
  });

  ctx.commands.register("ai.configure", () => {
    ctx.events.emit("ExtensionMessage", {
      source: "ai",
      type: "ConfigureRequested",
      payload: {
        currentModel: _llm?.usage,
        endpoint: ctx.config.get<string>("endpoint"),
        model: ctx.config.get<string>("model"),
      },
    });
  });

  // ── Keybindings ───────────────────────────────────────────────────────

  ctx.keybindings.register("Ctrl+Shift+I", "ai.chat.toggle");
  ctx.keybindings.register("Ctrl+Shift+E", "ai.explain");
  ctx.keybindings.register("Ctrl+Shift+Space", "ai.suggest");

  // ── Status bar ────────────────────────────────────────────────────────

  ctx.statusBar.set("right", "AI: idle");

  const unsubStatus = ctx.events.on("ExtensionMessage", (raw: unknown) => {
    const msg = raw as { source?: string; type?: string; payload?: unknown };
    if (msg.source === "ai" && msg.type === "StatusUpdate") {
      const p = msg.payload as { state: string };
      const icon = p.state === "thinking" ? "..." : p.state === "error" ? "!" : "";
      ctx.statusBar.set("right", `AI: ${p.state}${icon}`);
    }
  });
  _unsubscribers.push(unsubStatus);

  // ── Config change listener ────────────────────────────────────────────

  const unsubConfig = ctx.events.on("ExtensionMessage", (raw: unknown) => {
    const msg = raw as { source?: string; type?: string; payload?: unknown };
    if (msg.source === "ai" && msg.type === "ConfigUpdate") {
      const p = msg.payload as Partial<{
        endpoint: string;
        apiKey: string;
        model: string;
        autoExplain: boolean;
      }>;
      if (p.endpoint || p.apiKey || p.model) {
        _llm?.configure({
          endpoint: p.endpoint,
          apiKey: p.apiKey,
          model: p.model,
        });
      }
      if (p.autoExplain !== undefined) {
        _errors?.setAutoExplain(p.autoExplain);
      }
    }
  });
  _unsubscribers.push(unsubConfig);
}

export function deactivate(): void {
  // Stop agent if running
  _agent?.stop();

  // Destroy chat panel
  _chat?.destroy();

  // Cancel pending suggestions
  _suggestions?.cancel();

  // Clean up all event subscriptions
  for (const unsub of _unsubscribers) {
    unsub();
  }
  _unsubscribers.length = 0;

  // Clear references
  _llm = null;
  _assembler = null;
  _suggestions = null;
  _errors = null;
  _chat = null;
  _agent = null;
}
