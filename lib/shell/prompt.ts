/**
 * @marauder/shell — Prompt metadata tracking and navigation
 */

export interface PromptInfo {
  row: number;
  cwd: string;
  exitCode?: number;
  timestamp: number;
  command?: string;
}

const MAX_PROMPTS = 5000;

export class PromptTracker {
  readonly #prompts: PromptInfo[] = [];

  /** Record a new prompt at the given row. */
  recordPrompt(row: number, cwd: string): void {
    if (this.#prompts.length >= MAX_PROMPTS) {
      // Evict oldest 25% to avoid frequent trimming
      this.#prompts.splice(0, Math.floor(MAX_PROMPTS / 4));
    }
    this.#prompts.push({
      row,
      cwd,
      timestamp: Date.now(),
    });
  }

  /** Record the exit code for the most recent prompt. */
  recordFinish(exitCode: number): void {
    if (this.#prompts.length > 0) {
      this.#prompts[this.#prompts.length - 1]!.exitCode = exitCode;
    }
  }

  /** Record the command text for the most recent prompt. */
  recordCommand(command: string): void {
    if (this.#prompts.length > 0) {
      this.#prompts[this.#prompts.length - 1]!.command = command;
    }
  }

  /** Get the prompt info at or before the given row. */
  getPromptAt(row: number): PromptInfo | null {
    for (let i = this.#prompts.length - 1; i >= 0; i--) {
      if (this.#prompts[i]!.row <= row) {
        return this.#prompts[i]!;
      }
    }
    return null;
  }

  /** Get the prompt before the one at currentRow. */
  getPrevious(currentRow: number): PromptInfo | null {
    for (let i = this.#prompts.length - 1; i >= 0; i--) {
      if (this.#prompts[i]!.row < currentRow) {
        return this.#prompts[i]!;
      }
    }
    return null;
  }

  /** Get the prompt after the one at currentRow. */
  getNext(currentRow: number): PromptInfo | null {
    for (const p of this.#prompts) {
      if (p.row > currentRow) {
        return p;
      }
    }
    return null;
  }

  /** Get all tracked prompts. */
  getAll(): PromptInfo[] {
    return [...this.#prompts];
  }

  /** Number of tracked prompts. */
  get size(): number {
    return this.#prompts.length;
  }
}
