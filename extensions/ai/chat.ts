// extensions/ai/chat.ts
// Chat panel — streaming LLM conversation with terminal context awareness.

import type { ExtensionContext } from "@marauder/extensions";
import type { LLMClient, ChatMessage } from "./llm_client.ts";
import type { ContextAssembler } from "./context.ts";

const PANEL_ID = "ai-chat";

const CHAT_HTML = `<!DOCTYPE html>
<html>
<head>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    font-size: 13px;
    background: var(--bg, #1e1e2e);
    color: var(--fg, #cdd6f4);
    height: 100vh;
    display: flex;
    flex-direction: column;
  }
  #messages {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }
  .msg { margin-bottom: 8px; padding: 6px 8px; border-radius: 4px; white-space: pre-wrap; word-break: break-word; }
  .msg.user { background: rgba(137,180,250,0.15); }
  .msg.assistant { background: rgba(166,227,161,0.1); }
  .msg.system { background: rgba(249,226,175,0.1); font-style: italic; font-size: 12px; }
  .msg .role { font-weight: 600; font-size: 11px; text-transform: uppercase; margin-bottom: 2px; opacity: 0.6; }
  #input-area {
    display: flex;
    border-top: 1px solid rgba(205,214,244,0.1);
    padding: 6px;
  }
  #input {
    flex: 1;
    background: rgba(205,214,244,0.05);
    border: 1px solid rgba(205,214,244,0.15);
    border-radius: 4px;
    color: inherit;
    font-family: inherit;
    font-size: 13px;
    padding: 6px 8px;
    outline: none;
    resize: none;
  }
  #input:focus { border-color: rgba(137,180,250,0.5); }
  #send {
    margin-left: 6px;
    background: rgba(137,180,250,0.2);
    border: none;
    border-radius: 4px;
    color: inherit;
    padding: 6px 12px;
    cursor: pointer;
  }
  #send:hover { background: rgba(137,180,250,0.3); }
</style>
</head>
<body>
  <div id="messages"></div>
  <div id="input-area">
    <textarea id="input" rows="2" placeholder="Ask anything..."></textarea>
    <button id="send">Send</button>
  </div>
  <script>
    const messagesEl = document.getElementById("messages");
    const inputEl = document.getElementById("input");
    const sendBtn = document.getElementById("send");
    let currentAssistantEl = null;

    function addMessage(role, content) {
      const div = document.createElement("div");
      div.className = "msg " + role;
      const roleLabel = document.createElement("div");
      roleLabel.className = "role";
      roleLabel.textContent = role;
      const body = document.createElement("div");
      body.textContent = content;
      div.appendChild(roleLabel);
      div.appendChild(body);
      messagesEl.appendChild(div);
      messagesEl.scrollTop = messagesEl.scrollHeight;
      return body;
    }

    function send() {
      const text = inputEl.value.trim();
      if (!text) return;
      inputEl.value = "";
      addMessage("user", text);
      window.__TAURI_INTERNALS__?.postMessage?.({ type: "chat-input", text });
    }

    sendBtn.addEventListener("click", send);
    inputEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); }
    });

    // Listen for messages from the extension
    window.addEventListener("message", (e) => {
      const msg = e.data;
      if (!msg || !msg.type) return;
      if (msg.type === "token") {
        if (!currentAssistantEl) {
          currentAssistantEl = addMessage("assistant", "");
        }
        currentAssistantEl.textContent += msg.text;
        messagesEl.scrollTop = messagesEl.scrollHeight;
      } else if (msg.type === "stream-end") {
        currentAssistantEl = null;
      } else if (msg.type === "error") {
        addMessage("system", "Error: " + msg.text);
      } else if (msg.type === "clear") {
        messagesEl.innerHTML = "";
        currentAssistantEl = null;
      } else if (msg.type === "assistant-message") {
        addMessage("assistant", msg.text);
      } else if (msg.type === "system-message") {
        addMessage("system", msg.text);
      }
    });
  </script>
</body>
</html>`;

export class ChatPanel {
  #ctx: ExtensionContext;
  #llm: LLMClient;
  #assembler: ContextAssembler;
  #history: ChatMessage[] = [];
  #isStreaming = false;
  #isVisible = false;
  readonly unsubscribers: Array<() => void> = [];

  constructor(
    ctx: ExtensionContext,
    llm: LLMClient,
    assembler: ContextAssembler,
  ) {
    this.#ctx = ctx;
    this.#llm = llm;
    this.#assembler = assembler;

    // Register the chat panel
    ctx.panels.register({
      id: PANEL_ID,
      title: "AI Chat",
      html: CHAT_HTML,
      icon: "brain",
      position: "sidebar",
    });

    // Listen for chat input from the panel webview
    const unsubInput = ctx.events.on("ExtensionMessage", (raw: unknown) => {
      const msg = raw as { source?: string; type?: string; payload?: unknown };
      if (msg.source === PANEL_ID && msg.type === "chat-input") {
        const p = msg.payload as { text: string };
        this.#handleUserInput(p.text);
      }
    });
    this.unsubscribers.push(unsubInput);

    // Listen for error explanations to show in chat
    const unsubExplain = ctx.events.on("ExtensionMessage", (raw: unknown) => {
      const msg = raw as { source?: string; type?: string; payload?: unknown };
      if (msg.source === "ai" && msg.type === "ErrorExplanation") {
        const p = msg.payload as {
          command: string;
          summary: string;
          explanation: string;
          fixCommand: string | null;
        };
        this.show();
        let text = `Error in: ${p.command}\n\n${p.explanation}`;
        if (p.fixCommand) {
          text += `\n\nSuggested fix:\n  $ ${p.fixCommand}`;
        }
        ctx.panels.postMessage(PANEL_ID, "assistant-message", { text });
      }
    });
    this.unsubscribers.push(unsubExplain);
  }

  /** Toggle chat panel visibility. */
  toggle(): void {
    if (this.#isVisible) {
      this.hide();
    } else {
      this.show();
    }
  }

  /** Show the chat panel. */
  show(): void {
    this.#ctx.panels.show(PANEL_ID);
    this.#isVisible = true;
  }

  /** Hide the chat panel. */
  hide(): void {
    this.#ctx.panels.hide(PANEL_ID);
    this.#isVisible = false;
  }

  /** Destroy and clean up the panel. */
  destroy(): void {
    this.#ctx.panels.destroy(PANEL_ID);
    this.#isVisible = false;
    this.#history = [];
  }

  async #handleUserInput(text: string): Promise<void> {
    // Handle slash commands
    if (text === "/clear") {
      this.#history = [];
      this.#ctx.panels.postMessage(PANEL_ID, "clear", {});
      this.#ctx.panels.postMessage(PANEL_ID, "system-message", {
        text: "Conversation cleared.",
      });
      return;
    }

    if (text.startsWith("/agent ")) {
      const goal = text.slice(7).trim();
      if (goal) {
        this.#ctx.events.emit("ExtensionMessage", {
          source: "ai",
          type: "AgentStart",
          payload: { goal },
        });
        this.#ctx.panels.postMessage(PANEL_ID, "system-message", {
          text: `Agent mode started with goal: ${goal}`,
        });
      }
      return;
    }

    if (this.#isStreaming) return;
    this.#isStreaming = true;

    this.#history.push({ role: "user", content: text });

    try {
      const termCtx = this.#assembler.assembleContext();
      const systemMessages = this.#assembler.formatForLLM(termCtx);
      const messages = [...systemMessages, ...this.#history];

      let fullResponse = "";
      for await (const chunk of this.#llm.chat(messages, { temperature: 0.5 })) {
        fullResponse += chunk;
        this.#ctx.panels.postMessage(PANEL_ID, "token", { text: chunk });
      }
      this.#ctx.panels.postMessage(PANEL_ID, "stream-end", {});

      this.#history.push({ role: "assistant", content: fullResponse });

      // Keep history manageable — drop oldest pairs if > 20 messages
      while (this.#history.length > 20) {
        this.#history.shift();
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      this.#ctx.panels.postMessage(PANEL_ID, "error", { text: message });
    } finally {
      this.#isStreaming = false;
    }
  }
}
