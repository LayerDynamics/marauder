// extensions/ai/llm_client.ts
// OpenAI-compatible LLM client with streaming SSE support and retry logic.

/** A single chat message. */
export interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

/** Options for chat/complete calls. */
export interface LLMOptions {
  model?: string;
  temperature?: number;
  maxTokens?: number;
  stop?: string[];
}

/** Non-streaming completion result. */
export interface CompletionResult {
  content: string;
  usage: { promptTokens: number; completionTokens: number; totalTokens: number };
}

/** Client configuration. */
export interface LLMConfig {
  endpoint: string;
  apiKey: string;
  model: string;
}

const DEFAULT_CONFIG: LLMConfig = {
  endpoint: "https://api.openai.com",
  apiKey: "",
  model: "gpt-4o",
};

const MAX_RETRIES = 3;
const BACKOFF_BASE_MS = 1000;
const REQUEST_TIMEOUT_MS = 30_000;

export class LLMClient {
  #config: LLMConfig;
  #totalPromptTokens = 0;
  #totalCompletionTokens = 0;

  constructor(config?: Partial<LLMConfig>) {
    this.#config = { ...DEFAULT_CONFIG, ...config };
  }

  /** Update configuration (e.g. when user changes settings). */
  configure(config: Partial<LLMConfig>): void {
    this.#config = { ...this.#config, ...config };
  }

  /** Get cumulative token usage. */
  get usage(): { promptTokens: number; completionTokens: number } {
    return {
      promptTokens: this.#totalPromptTokens,
      completionTokens: this.#totalCompletionTokens,
    };
  }

  /** Streaming chat — returns an async iterable of content delta strings. */
  async *chat(
    messages: ChatMessage[],
    opts?: LLMOptions,
  ): AsyncIterable<string> {
    const body = this.#buildRequestBody(messages, opts, true);
    const response = await this.#fetchWithRetry(body);

    if (!response.body) {
      throw new Error("LLM response has no body for streaming");
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n");
        buffer = lines.pop() ?? "";

        for (const line of lines) {
          const trimmed = line.trim();
          if (!trimmed.startsWith("data: ")) continue;
          const data = trimmed.slice(6);
          if (data === "[DONE]") return;

          try {
            const parsed = JSON.parse(data);
            const delta = parsed.choices?.[0]?.delta?.content;
            if (typeof delta === "string" && delta.length > 0) {
              yield delta;
            }
          } catch {
            // Skip malformed JSON lines
          }
        }
      }
    } finally {
      reader.releaseLock();
    }
  }

  /** Non-streaming completion. */
  async complete(
    messages: ChatMessage[],
    opts?: LLMOptions,
  ): Promise<CompletionResult> {
    const body = this.#buildRequestBody(messages, opts, false);
    const response = await this.#fetchWithRetry(body);
    const json = await response.json();

    const content = json.choices?.[0]?.message?.content ?? "";
    const usage = json.usage ?? {};
    const promptTokens = usage.prompt_tokens ?? 0;
    const completionTokens = usage.completion_tokens ?? 0;

    this.#totalPromptTokens += promptTokens;
    this.#totalCompletionTokens += completionTokens;

    return {
      content,
      usage: {
        promptTokens,
        completionTokens,
        totalTokens: promptTokens + completionTokens,
      },
    };
  }

  #buildRequestBody(
    messages: ChatMessage[],
    opts: LLMOptions | undefined,
    stream: boolean,
  ): string {
    const payload: Record<string, unknown> = {
      model: opts?.model ?? this.#config.model,
      messages,
      stream,
    };
    if (opts?.temperature !== undefined) payload.temperature = opts.temperature;
    if (opts?.maxTokens !== undefined) payload.max_tokens = opts.maxTokens;
    if (opts?.stop !== undefined) payload.stop = opts.stop;
    return JSON.stringify(payload);
  }

  async #fetchWithRetry(body: string): Promise<Response> {
    let lastError: Error | undefined;

    for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
      if (attempt > 0) {
        const delay = BACKOFF_BASE_MS * Math.pow(2, attempt - 1);
        await new Promise((r) => setTimeout(r, delay));
      }

      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);

      try {
        const url = `${this.#config.endpoint}/v1/chat/completions`;
        const response = await fetch(url, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${this.#config.apiKey}`,
          },
          body,
          signal: controller.signal,
        });

        if (response.status === 429 || response.status >= 500) {
          lastError = new Error(`LLM API error: ${response.status}`);
          continue;
        }

        if (!response.ok) {
          const text = await response.text().catch(() => "");
          throw new Error(`LLM API error ${response.status}: ${text}`);
        }

        return response;
      } catch (err) {
        if (err instanceof DOMException && err.name === "AbortError") {
          lastError = new Error("LLM request timed out");
          continue;
        }
        if (attempt === MAX_RETRIES - 1) throw err;
        lastError = err instanceof Error ? err : new Error(String(err));
      } finally {
        clearTimeout(timeout);
      }
    }

    throw lastError ?? new Error("LLM request failed after retries");
  }
}
