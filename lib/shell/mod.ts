/**
 * @marauder/shell — ShellEngine Skeleton
 *
 * Detects shell integration sequences (OSC 133, OSC 7) and tracks
 * command zones, history, and working directory.
 */

import { type EventBus, EventType } from "@marauder/ffi-event-bus";
import type { BusEvent } from "@marauder/ffi-event-bus";
import type { Grid } from "@marauder/ffi-grid";
import { decodeBusPayload, Logger } from "@marauder/dev";
import { CommandHistory } from "./history.ts";
import { PromptTracker } from "./prompt.ts";
import {
  CompletionEngine,
  HistoryCompletionProvider,
  PathCompletionProvider,
} from "./completions.ts";

export type { FuzzyMatch, HistoryConfig } from "./history.ts";
export { CommandHistory } from "./history.ts";
export type {
  CompletionContext,
  CompletionItem,
  CompletionKind,
  CompletionProvider,
} from "./completions.ts";
export { CompletionEngine, HistoryCompletionProvider, PathCompletionProvider } from "./completions.ts";
export type { PromptInfo } from "./prompt.ts";
export { PromptTracker } from "./prompt.ts";
export {
  detectShell,
  getIntegrationScript,
  injectShellIntegration,
  isInjected,
} from "./inject.ts";

export interface ShellZone {
  type: "prompt" | "command" | "output";
  startRow: number;
  endRow: number;
  content?: string;
}

export interface CommandRecord {
  command: string;
  cwd: string;
  startTime: number;
  endTime?: number;
  exitCode?: number;
}

const MAX_ZONES = 10000;

export class ShellEngine {
  readonly #eventBus: EventBus;
  readonly #grid: Grid;
  readonly #log: Logger;
  readonly #history: CommandHistory;
  readonly #zones: ShellZone[] = [];
  readonly #promptTracker: PromptTracker;
  readonly #completionEngine: CompletionEngine;
  #cwd: string;
  #currentCommand: CommandRecord | null = null;
  #subscriberId: bigint | null = null;
  #promptRow: number | null = null;

  constructor(eventBus: EventBus, grid: Grid, initialCwd: string) {
    this.#eventBus = eventBus;
    this.#grid = grid;
    this.#cwd = initialCwd;
    this.#log = new Logger("shell-engine");
    this.#history = new CommandHistory();
    this.#promptTracker = new PromptTracker();
    this.#completionEngine = new CompletionEngine();
    this.#completionEngine.registerProvider(new HistoryCompletionProvider());
    this.#completionEngine.registerProvider(new PathCompletionProvider());
  }

  /** Start listening for parser actions on the event bus */
  start(): void {
    this.#subscriberId = this.#eventBus.subscribe(
      EventType.ParserAction,
      (event: BusEvent) => {
        this.#handleParserAction(event);
      },
    );
    this.#log.info("ShellEngine started");
  }

  /** Stop listening and clean up */
  stop(): void {
    if (this.#subscriberId !== null) {
      this.#eventBus.unsubscribe(EventType.ParserAction, this.#subscriberId);
      this.#subscriberId = null;
    }
    this.#log.info("ShellEngine stopped");
  }

  [Symbol.dispose](): void {
    this.stop();
  }

  #handleParserAction(event: BusEvent): void {
    const payload = this.#decodePayload(event);
    if (!payload) return;

    // Rust serde externally-tagged enum: {"VariantName": {...}}
    // Dispatch based on the variant key present in the payload
    if (payload["OscDispatch"]) {
      const oscData = payload["OscDispatch"] as { command: number; data: string };
      this.#handleOsc(oscData.command, oscData.data ?? "");
    } else if (payload["SetMode"]) {
      this.#log.debug(`SetMode action received: ${JSON.stringify(payload["SetMode"])}`);
    } else if (payload["Print"]) {
      // Print actions are high-frequency, no logging needed
    } else if (payload["CursorMove"]) {
      this.#log.debug(`CursorMove action received`);
    } else if (payload["Execute"]) {
      // Control character execution (e.g. BEL, BS, CR, LF)
    } else {
      // Log unhandled variants for future implementation
      const variant = Object.keys(payload)[0];
      if (variant) {
        this.#log.debug(`Unhandled parser action variant: ${variant}`);
      }
    }
  }

  #handleOsc(command: number, data: string): void {
    // OSC 133 — Shell integration (FinalTerm)
    if (command === 133) {
      this.#handleOsc133(data);
    }

    // OSC 7 — Current working directory
    if (command === 7) {
      this.#handleOsc7(data);
    }
  }

  /** Read text from grid between two row/col positions. Stops at null cells (end of row). */
  #readGridText(
    startRow: number,
    startCol: number,
    endRow: number,
    endCol: number,
  ): string {
    let text = "";
    for (let r = startRow; r <= endRow; r++) {
      const colStart = r === startRow ? startCol : 0;
      const colLimit = r === endRow ? endCol : Infinity;
      for (let c = colStart; c < colLimit; c++) {
        const cell = this.#grid.getCell(r, c);
        if (!cell) break; // End of row or out of bounds
        text += (cell as { char?: string }).char ?? "";
      }
      if (r < endRow) text += "\n";
    }
    return text.trim();
  }

  /** Get current cursor row from the grid */
  #getCursorRow(): number {
    return this.#grid.getCursor().row;
  }

  /**
   * OSC 133 sequences (FinalTerm shell integration):
   * - 133;A — Prompt start
   * - 133;B — Command start (after prompt, before execution)
   * - 133;C — Command executed (output begins)
   * - 133;D;exitcode — Command finished
   */
  #handleOsc133(data: string): void {
    // Prevent unbounded zone growth
    if (this.#zones.length >= MAX_ZONES) {
      this.#zones.splice(0, Math.floor(MAX_ZONES / 4));
    }

    const parts = data.split(";");
    const code = parts[0];
    const cursorRow = this.#getCursorRow();

    switch (code) {
      case "A": {
        // Prompt start — record the row for zone tracking
        this.#promptRow = cursorRow;
        // Close previous zone if open
        if (this.#zones.length > 0) {
          const lastZone = this.#zones[this.#zones.length - 1]!;
          if (lastZone.endRow === lastZone.startRow) {
            lastZone.endRow = Math.max(cursorRow - 1, lastZone.startRow);
          }
        }
        this.#zones.push({
          type: "prompt",
          startRow: cursorRow,
          endRow: cursorRow,
        });
        this.#promptTracker.recordPrompt(cursorRow, this.#cwd);
        this.#eventBus.publish(EventType.ShellPromptDetected, {
          cwd: this.#cwd,
        });
        this.#log.debug(`Prompt detected at row ${cursorRow}`);
        break;
      }
      case "B": {
        // Command start — user finished typing, capture command text from grid
        // Text between prompt start row and current cursor position is the command
        const commandRow = cursorRow;

        // Close prompt zone, start command zone
        if (this.#zones.length > 0) {
          const lastZone = this.#zones[this.#zones.length - 1]!;
          if (lastZone.type === "prompt") {
            lastZone.endRow = commandRow;
          }
        }

        // Read command text from grid between prompt row and cursor
        const promptStartRow = this.#promptRow ?? cursorRow;
        const commandText = this.#readGridText(
          promptStartRow,
          0,
          commandRow,
          Infinity,
        );
        // Strip prompt prefix — find the last prompt delimiter on the final line
        const lines = commandText.split("\n");
        const lastLine = lines[lines.length - 1] ?? "";
        // Find the last occurrence of a prompt delimiter ($ % # >) followed by whitespace
        // This handles complex prompts like "user@host:~/path>"
        const promptMatch = lastLine.match(/^.*[\$%#>]\s*/);
        const strippedCommand = promptMatch
          ? lastLine.slice(promptMatch[0].length).trim()
          : lastLine.trim();

        this.#zones.push({
          type: "command",
          startRow: promptStartRow,
          endRow: commandRow,
          content: strippedCommand,
        });

        // Prepare command record for when execution starts
        this.#currentCommand = {
          command: strippedCommand,
          cwd: this.#cwd,
          startTime: Date.now(),
        };
        this.#promptTracker.recordCommand(strippedCommand);

        this.#log.debug(`Command captured: "${strippedCommand}"`);
        break;
      }
      case "C": {
        // Command executed — output begins
        if (this.#currentCommand === null) {
          // No B event received (shell integration may be partial)
          this.#currentCommand = {
            command: "",
            cwd: this.#cwd,
            startTime: Date.now(),
          };
        }
        this.#zones.push({
          type: "output",
          startRow: cursorRow,
          endRow: cursorRow,
        });
        this.#eventBus.publish(EventType.ShellCommandStarted, {
          command: this.#currentCommand.command,
          cwd: this.#cwd,
        });
        this.#log.debug("Command execution started");
        break;
      }
      case "D": {
        // Command finished
        const exitCode = parts.length > 1 ? parseInt(parts[1] ?? "0", 10) : 0;
        // Close output zone
        if (this.#zones.length > 0) {
          const lastZone = this.#zones[this.#zones.length - 1]!;
          if (lastZone.type === "output") {
            lastZone.endRow = cursorRow;
          }
        }
        if (this.#currentCommand) {
          this.#currentCommand.endTime = Date.now();
          this.#currentCommand.exitCode = exitCode;
          this.#history.add(this.#currentCommand);
          this.#promptTracker.recordFinish(exitCode);
          this.#eventBus.publish(EventType.ShellCommandFinished, {
            command: this.#currentCommand.command,
            exitCode,
            duration: this.#currentCommand.endTime -
              this.#currentCommand.startTime,
          });
          this.#log.debug(`Command finished: exit=${exitCode}`);
          this.#currentCommand = null;
        }
        this.#promptRow = null;
        break;
      }
    }
  }

  /** OSC 7 — file://hostname/path format for CWD updates */
  #handleOsc7(data: string): void {
    try {
      // Format: file://hostname/path
      const url = new URL(data);
      if (url.protocol === "file:") {
        const newCwd = decodeURIComponent(url.pathname);
        if (newCwd !== this.#cwd) {
          this.#cwd = newCwd;
          this.#eventBus.publish(EventType.ShellCwdChanged, { cwd: newCwd });
          this.#log.debug(`CWD changed: ${newCwd}`);
        }
      }
    } catch {
      // Not a valid URL — might be a raw path, validate it starts with /
      if (data.startsWith("/") && data.length > 1 && data !== this.#cwd) {
        this.#cwd = data;
        this.#eventBus.publish(EventType.ShellCwdChanged, { cwd: data });
        this.#log.debug(`CWD changed (raw path): ${data}`);
      }
    }
  }

  #decodePayload(event: BusEvent): Record<string, unknown> | null {
    const result = decodeBusPayload(event.payload);
    if (result === null && event.payload) {
      this.#log.warn("Failed to decode event payload");
    }
    return result;
  }

  getCwd(): string {
    return this.#cwd;
  }

  getHistory(): CommandRecord[] {
    return this.#history.getAll();
  }

  getLastCommand(): CommandRecord | undefined {
    const all = this.#history.getAll();
    return all[all.length - 1];
  }

  getZones(): ShellZone[] {
    return [...this.#zones];
  }

  getCommandHistory(): CommandHistory {
    return this.#history;
  }

  getPromptTracker(): PromptTracker {
    return this.#promptTracker;
  }

  getCompletionEngine(): CompletionEngine {
    return this.#completionEngine;
  }
}
