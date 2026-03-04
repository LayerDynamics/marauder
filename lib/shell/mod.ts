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

export class ShellEngine {
  readonly #eventBus: EventBus;
  readonly #grid: Grid;
  readonly #log: Logger;
  readonly #history: CommandRecord[] = [];
  readonly #zones: ShellZone[] = [];
  #cwd: string;
  #currentCommand: CommandRecord | null = null;
  #subscriberId: bigint | null = null;
  #promptRow: number | null = null;

  constructor(eventBus: EventBus, grid: Grid, initialCwd: string) {
    this.#eventBus = eventBus;
    this.#grid = grid;
    this.#cwd = initialCwd;
    this.#log = new Logger("shell-engine");
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

    const actionType = payload.type as string;

    // OSC dispatch — look for shell integration sequences
    if (actionType === "OscDispatch") {
      this.#handleOsc(payload);
    }
  }

  #handleOsc(payload: Record<string, unknown>): void {
    const params = payload.params as number[] | undefined;
    const data = payload.data as string | undefined;

    if (!params || params.length === 0) return;

    // OSC 133 — Shell integration (FinalTerm)
    if (params[0] === 133) {
      this.#handleOsc133(data ?? "");
    }

    // OSC 7 — Current working directory
    if (params[0] === 7) {
      this.#handleOsc7(data ?? "");
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
        // Strip prompt prefix — the command is typically on the last line after the prompt char
        const lines = commandText.split("\n");
        const lastLine = lines[lines.length - 1] ?? "";
        // Remove common prompt patterns ($ , % , > , # ) from the beginning
        const strippedCommand = lastLine.replace(/^[^$%>#]*[$%>#]\s*/, "")
          .trim();

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
          this.#history.push(this.#currentCommand);
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
      if (data.startsWith("/") && data.length > 1) {
        if (data !== this.#cwd) {
          this.#cwd = data;
          this.#eventBus.publish(EventType.ShellCwdChanged, { cwd: data });
          this.#log.debug(`CWD changed (raw path): ${data}`);
        }
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
    return [...this.#history];
  }

  getLastCommand(): CommandRecord | undefined {
    return this.#history[this.#history.length - 1];
  }

  getZones(): ShellZone[] {
    return [...this.#zones];
  }
}
