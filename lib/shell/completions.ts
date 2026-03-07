/**
 * @marauder/shell — Tab completion engine
 */

import type { CommandHistory } from "./history.ts";

export type CompletionKind = "history" | "file" | "directory" | "command" | "argument";

export interface CompletionItem {
  label: string;
  kind: CompletionKind;
  detail?: string;
  insertText?: string;
}

export interface CompletionContext {
  input: string;
  cursorPosition: number;
  cwd: string;
  history: CommandHistory;
}

export interface CompletionProvider {
  id: string;
  provide(context: CompletionContext): CompletionItem[] | Promise<CompletionItem[]>;
}

export class CompletionEngine {
  readonly #providers: Map<string, CompletionProvider> = new Map();

  registerProvider(provider: CompletionProvider): void {
    this.#providers.set(provider.id, provider);
  }

  unregisterProvider(id: string): void {
    this.#providers.delete(id);
  }

  /** Run all providers concurrently with timeout. Individual provider failures are isolated. */
  async complete(context: CompletionContext, timeoutMs = 2000): Promise<CompletionItem[]> {
    const providers = [...this.#providers.values()];
    const timeout = new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error("completion timeout")), timeoutMs)
    );
    const settled = await Promise.allSettled(
      providers.map((p) => Promise.race([
        Promise.resolve(p.provide(context)),
        timeout,
      ])),
    );
    const results: CompletionItem[] = [];
    for (const outcome of settled) {
      if (outcome.status === "fulfilled") {
        results.push(...outcome.value);
      }
    }
    return results;
  }

  getProviderIds(): string[] {
    return [...this.#providers.keys()];
  }
}

/** Completes from command history using prefix matching. */
export class HistoryCompletionProvider implements CompletionProvider {
  readonly id = "history";

  provide(context: CompletionContext): CompletionItem[] {
    // Extract the token at cursor position, not the full input line
    const beforeCursor = context.input.slice(0, context.cursorPosition);
    const tokens = beforeCursor.split(/\s+/);
    const prefix = (tokens[tokens.length - 1] ?? "").trim();
    if (!prefix) return [];

    const seen = new Set<string>();
    const items: CompletionItem[] = [];
    const matches = context.history.search(prefix);

    for (const record of matches) {
      if (seen.has(record.command)) continue;
      seen.add(record.command);
      items.push({
        label: record.command,
        kind: "history",
        detail: `cwd: ${record.cwd}`,
      });
      if (items.length >= 20) break;
    }

    return items;
  }
}

/** Completes file/directory paths using Deno.readDir. */
export class PathCompletionProvider implements CompletionProvider {
  readonly id = "path";

  async provide(context: CompletionContext): Promise<CompletionItem[]> {
    const input = context.input.slice(0, context.cursorPosition);
    // Extract the last whitespace-delimited token as the path fragment
    const tokens = input.split(/\s+/);
    const fragment = tokens[tokens.length - 1] ?? "";
    if (!fragment) return [];

    // Resolve relative to cwd, normalizing to prevent unexpected traversal
    const isAbsolute = fragment.startsWith("/");
    const lastSlash = fragment.lastIndexOf("/");
    const rawDir = lastSlash >= 0
      ? (isAbsolute ? fragment.slice(0, lastSlash + 1) : `${context.cwd}/${fragment.slice(0, lastSlash + 1)}`)
      : context.cwd;
    const prefix = lastSlash >= 0 ? fragment.slice(lastSlash + 1) : fragment;

    // Normalize path to resolve .. and . segments
    const dir = normalizePath(rawDir);

    const items: CompletionItem[] = [];
    try {
      for await (const entry of Deno.readDir(dir)) {
        if (prefix && !entry.name.startsWith(prefix)) continue;
        items.push({
          label: entry.name + (entry.isDirectory ? "/" : ""),
          kind: entry.isDirectory ? "directory" : "file",
          insertText: entry.name + (entry.isDirectory ? "/" : ""),
        });
        if (items.length >= 50) break;
      }
    } catch {
      // Directory doesn't exist or not readable
    }

    return items;
  }
}

/** Normalize a path by resolving . and .. segments. */
function normalizePath(path: string): string {
  const parts = path.split("/");
  const resolved: string[] = [];
  for (const part of parts) {
    if (part === "." || part === "") {
      if (resolved.length === 0) resolved.push("");
      continue;
    }
    if (part === "..") {
      if (resolved.length > 1) resolved.pop();
    } else {
      resolved.push(part);
    }
  }
  return resolved.join("/") || "/";
}
