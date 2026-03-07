// extensions/ai/agent.ts
// Autonomous agent mode — iterative goal pursuit with risk-classified actions.

import type { ExtensionContext } from "@marauder/extensions";
import type { LLMClient, ChatMessage } from "./llm_client.ts";
import type { ContextAssembler } from "./context.ts";

const MAX_ITERATIONS = 20;
const PANEL_ID = "ai-chat";

/** Risk level for a proposed command. */
type RiskLevel = "safe" | "moderate" | "destructive";

/** A proposed action from the agent. */
interface ProposedAction {
  command: string;
  reasoning: string;
  risk: RiskLevel;
}

/** Agent iteration result. */
interface IterationResult {
  action: ProposedAction | null;
  done: boolean;
  summary: string;
}

/** Read-only commands that are always safe. */
const SAFE_PATTERNS = [
  /^ls\b/, /^cat\b/, /^head\b/, /^tail\b/, /^grep\b/, /^find\b/,
  /^echo\b/, /^pwd\b/, /^whoami\b/, /^which\b/, /^type\b/, /^file\b/,
  /^wc\b/, /^diff\b/, /^git\s+(status|log|branch|diff|show|remote)\b/,
  /^git\s+stash\s+list\b/, /^env\b/, /^printenv\b/, /^hostname\b/,
  /^uname\b/, /^date\b/, /^df\b/, /^du\b/, /^ps\b/, /^top\b/,
  /^man\b/, /^tree\b/, /^stat\b/, /^curl\b/, /^wget\b/,
];

/** Destructive commands requiring extra caution. */
const DESTRUCTIVE_PATTERNS = [
  /\brm\s/, /\brm\b$/, /\brmdir\b/, /\bmkfs\b/, /\bdd\b/,
  /\bgit\s+(reset|clean|push\s+--force|push\s+-f)\b/,
  /\bDROP\b/i, /\bDELETE\s+FROM\b/i, /\bTRUNCATE\b/i,
  /\bsudo\s+rm\b/, /\bchmod\s+000\b/, /\bkill\s+-9\b/,
  /\b:\s*>\s*\//, // redirect to root paths
];

const AGENT_SYSTEM_PROMPT = `You are an autonomous terminal agent. You will be given a goal and terminal context.

For each iteration, analyze the current state and either:
1. Propose a shell command to run
2. Declare the goal complete

Respond with ONLY a JSON object:
- If proposing an action: {"done": false, "command": "<shell command>", "reasoning": "<why this step>"}
- If goal is complete: {"done": true, "summary": "<what was accomplished>"}

Rules:
- One command per iteration
- Prefer safe, reversible commands
- If uncertain, use read-only commands first (ls, cat, git status)
- Never run destructive commands without explicit reasoning
- If stuck after multiple attempts, declare done with a summary of what was tried`;

export class AgentRunner {
  #ctx: ExtensionContext;
  #llm: LLMClient;
  #assembler: ContextAssembler;
  #isRunning = false;
  #currentGoal = "";
  #iteration = 0;
  #conversationHistory: ChatMessage[] = [];
  #pendingApproval: ProposedAction | null = null;
  #approvalResolver: ((approved: boolean) => void) | null = null;
  readonly unsubscribers: Array<() => void> = [];

  constructor(
    ctx: ExtensionContext,
    llm: LLMClient,
    assembler: ContextAssembler,
  ) {
    this.#ctx = ctx;
    this.#llm = llm;
    this.#assembler = assembler;

    // Listen for agent start requests
    const unsubStart = ctx.events.on("ExtensionMessage", (raw: unknown) => {
      const msg = raw as { source?: string; type?: string; payload?: unknown };
      if (msg.source === "ai" && msg.type === "AgentStart") {
        const p = msg.payload as { goal: string };
        this.start(p.goal);
      }
    });
    this.unsubscribers.push(unsubStart);

    // Listen for approval responses from the panel
    const unsubApproval = ctx.events.on("ExtensionMessage", (raw: unknown) => {
      const msg = raw as { source?: string; type?: string; payload?: unknown };
      if (msg.source === "ai" && msg.type === "AgentApproval") {
        const p = msg.payload as { approved: boolean };
        if (this.#approvalResolver) {
          this.#approvalResolver(p.approved);
          this.#approvalResolver = null;
        }
      }
    });
    this.unsubscribers.push(unsubApproval);
  }

  /** Whether the agent is currently running. */
  get isRunning(): boolean {
    return this.#isRunning;
  }

  /** Start the agent with a goal. */
  async start(goal: string): Promise<void> {
    if (this.#isRunning) {
      this.#postToPanel("system-message", {
        text: "Agent is already running. Use /agent stop to cancel.",
      });
      return;
    }

    this.#isRunning = true;
    this.#currentGoal = goal;
    this.#iteration = 0;
    this.#conversationHistory = [];
    this.#pendingApproval = null;

    this.#updateStatus("thinking");
    this.#postToPanel("system-message", {
      text: `Agent started with goal: ${goal}`,
    });

    try {
      await this.#runLoop();
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      this.#postToPanel("error", { text: `Agent error: ${message}` });
    } finally {
      this.#isRunning = false;
      this.#updateStatus("idle");
    }
  }

  /** Stop the agent. */
  stop(): void {
    if (!this.#isRunning) return;
    this.#isRunning = false;
    if (this.#approvalResolver) {
      this.#approvalResolver(false);
      this.#approvalResolver = null;
    }
    this.#postToPanel("system-message", { text: "Agent stopped by user." });
    this.#updateStatus("idle");
  }

  async #runLoop(): Promise<void> {
    while (this.#isRunning && this.#iteration < MAX_ITERATIONS) {
      this.#iteration++;
      this.#updateStatus("thinking");

      const result = await this.#assess();

      if (result.done) {
        this.#postToPanel("assistant-message", {
          text: `Goal complete (${this.#iteration} iterations):\n${result.summary}`,
        });
        return;
      }

      if (!result.action) {
        this.#postToPanel("error", { text: "Agent failed to propose an action." });
        return;
      }

      const action = result.action;
      const riskLabel = action.risk === "safe" ? "" : ` [${action.risk.toUpperCase()}]`;
      this.#postToPanel("assistant-message", {
        text: `Step ${this.#iteration}: ${action.reasoning}\n$ ${action.command}${riskLabel}`,
      });

      // Auto-approve safe commands, request approval for others
      let approved = action.risk === "safe";
      if (!approved) {
        this.#updateStatus("waiting");
        this.#pendingApproval = action;
        this.#ctx.events.emit("ExtensionMessage", {
          source: "ai",
          type: "AgentApprovalRequest",
          payload: {
            command: action.command,
            reasoning: action.reasoning,
            risk: action.risk,
            iteration: this.#iteration,
          },
        });

        approved = await new Promise<boolean>((resolve) => {
          this.#approvalResolver = resolve;
        });
        this.#pendingApproval = null;

        if (!approved) {
          this.#postToPanel("system-message", {
            text: "Action rejected. Agent will try an alternative approach.",
          });
          this.#conversationHistory.push({
            role: "user",
            content: "The user rejected this command. Try a different, safer approach.",
          });
          continue;
        }
      }

      // Execute the command
      this.#updateStatus("executing");
      const output = await this.#executeCommand(action.command);

      this.#conversationHistory.push({
        role: "assistant",
        content: JSON.stringify({
          done: false,
          command: action.command,
          reasoning: action.reasoning,
        }),
      });
      this.#conversationHistory.push({
        role: "user",
        content: `Command output:\n${output}`,
      });
    }

    if (this.#iteration >= MAX_ITERATIONS) {
      this.#postToPanel("system-message", {
        text: `Agent reached maximum iterations (${MAX_ITERATIONS}). Stopping.`,
      });
    }
  }

  async #assess(): Promise<IterationResult> {
    const termCtx = this.#assembler.assembleContext();
    const systemMessages = this.#assembler.formatForLLM(termCtx);

    const messages: ChatMessage[] = [
      ...systemMessages,
      { role: "system", content: AGENT_SYSTEM_PROMPT },
      { role: "user", content: `Goal: ${this.#currentGoal}` },
      ...this.#conversationHistory,
    ];

    if (this.#iteration > 1) {
      messages.push({
        role: "user",
        content: `This is iteration ${this.#iteration}/${MAX_ITERATIONS}. What is the next step?`,
      });
    }

    const result = await this.#llm.complete(messages, {
      temperature: 0.2,
      maxTokens: 512,
    });

    return this.#parseAssessment(result.content);
  }

  #parseAssessment(content: string): IterationResult {
    try {
      const match = content.match(/\{[\s\S]*\}/);
      if (!match) {
        return { action: null, done: false, summary: "" };
      }
      const parsed = JSON.parse(match[0]);

      if (parsed.done) {
        return {
          action: null,
          done: true,
          summary: typeof parsed.summary === "string" ? parsed.summary : "Goal completed.",
        };
      }

      const command = typeof parsed.command === "string" ? parsed.command : "";
      if (!command) {
        return { action: null, done: false, summary: "" };
      }

      return {
        action: {
          command,
          reasoning: typeof parsed.reasoning === "string" ? parsed.reasoning : "",
          risk: this.#classifyRisk(command),
        },
        done: false,
        summary: "",
      };
    } catch {
      return { action: null, done: false, summary: "" };
    }
  }

  #classifyRisk(command: string): RiskLevel {
    const trimmed = command.trim();
    if (DESTRUCTIVE_PATTERNS.some((p) => p.test(trimmed))) return "destructive";
    if (SAFE_PATTERNS.some((p) => p.test(trimmed))) return "safe";
    return "moderate";
  }

  async #executeCommand(command: string): Promise<string> {
    // Emit the command for PTY execution
    this.#ctx.events.emit("ExtensionMessage", {
      source: "ai",
      type: "PtyWrite",
      payload: { data: command + "\n" },
    });

    // Wait for ShellCommandFinished
    return new Promise<string>((resolve) => {
      const timeout = setTimeout(() => {
        unsub();
        resolve("(command timed out after 30s)");
      }, 30_000);

      const unsub = this.#ctx.events.on("ShellCommandFinished", (raw: unknown) => {
        const p = raw as { command: string; exitCode: number; output?: string };
        clearTimeout(timeout);
        unsub();
        const exitInfo = p.exitCode === 0 ? "" : `\n[exit code: ${p.exitCode}]`;
        resolve((p.output ?? "(no output)") + exitInfo);
      });
    });
  }

  #postToPanel(type: string, data: unknown): void {
    this.#ctx.panels.postMessage(PANEL_ID, type, data);
  }

  #updateStatus(state: string): void {
    this.#ctx.events.emit("ExtensionMessage", {
      source: "ai",
      type: "StatusUpdate",
      payload: { state },
    });
  }
}
