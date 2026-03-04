/**
 * @marauder/dev — Logging + Debug Helpers
 */

export enum LogLevel {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3,
}

const LEVEL_NAMES: Record<LogLevel, string> = {
  [LogLevel.Debug]: "DEBUG",
  [LogLevel.Info]: "INFO",
  [LogLevel.Warn]: "WARN",
  [LogLevel.Error]: "ERROR",
};

const MAX_PREFIX_DEPTH = 5;
const MAX_PERF_MARKS = 1000;

function parseLogLevel(s: string | undefined): LogLevel {
  switch (s?.toLowerCase()) {
    case "debug":
      return LogLevel.Debug;
    case "info":
      return LogLevel.Info;
    case "warn":
      return LogLevel.Warn;
    case "error":
      return LogLevel.Error;
    default:
      return LogLevel.Info;
  }
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    try {
      return String(value);
    } catch {
      return "[unstringifiable]";
    }
  }
}

export class Logger {
  #level: LogLevel;
  readonly #prefix: string;
  readonly #depth: number;

  constructor(prefix = "marauder", level?: LogLevel, depth = 1) {
    this.#prefix = prefix;
    this.#depth = depth;
    this.#level = level ??
      parseLogLevel(Deno.env.get("MARAUDER_LOG_LEVEL"));
  }

  get level(): LogLevel {
    return this.#level;
  }

  /** Adjust log level at runtime */
  setLevel(level: LogLevel): void {
    this.#level = level;
  }

  #format(level: LogLevel, msg: string, args: unknown[]): string {
    const ts = new Date().toISOString();
    const base = `[${ts}] [${LEVEL_NAMES[level]}] [${this.#prefix}] ${msg}`;
    if (args.length === 0) return base;
    return `${base} ${args.map((a) => safeStringify(a)).join(" ")}`;
  }

  debug(msg: string, ...args: unknown[]): void {
    if (this.#level <= LogLevel.Debug) {
      console.debug(this.#format(LogLevel.Debug, msg, args));
    }
  }

  info(msg: string, ...args: unknown[]): void {
    if (this.#level <= LogLevel.Info) {
      console.info(this.#format(LogLevel.Info, msg, args));
    }
  }

  warn(msg: string, ...args: unknown[]): void {
    if (this.#level <= LogLevel.Warn) {
      console.warn(this.#format(LogLevel.Warn, msg, args));
    }
  }

  error(msg: string, ...args: unknown[]): void {
    if (this.#level <= LogLevel.Error) {
      console.error(this.#format(LogLevel.Error, msg, args));
    }
  }

  child(prefix: string): Logger {
    if (this.#depth >= MAX_PREFIX_DEPTH) {
      return new Logger(this.#prefix, this.#level, this.#depth);
    }
    return new Logger(
      `${this.#prefix}:${prefix}`,
      this.#level,
      this.#depth + 1,
    );
  }
}

/** Global default logger */
export const log = new Logger();

/**
 * Debug inspector — dump grid state, cursor, event bus stats
 */
export class DebugInspector {
  readonly #log: Logger;

  constructor() {
    this.#log = new Logger("inspector", LogLevel.Debug);
  }

  dumpGrid(
    grid: {
      getCursor(): { row: number; col: number };
      getDirtyRows(): number[];
      getCell(row: number, col: number): unknown;
    },
    rows: number,
    cols: number,
  ): void {
    const cursor = grid.getCursor();
    this.#log.debug(`Cursor: row=${cursor.row} col=${cursor.col}`);
    const dirty = grid.getDirtyRows();
    this.#log.debug(`Dirty rows: [${dirty.join(", ")}]`);

    const lines: string[] = [];
    for (let r = 0; r < rows; r++) {
      let line = "";
      for (let c = 0; c < cols; c++) {
        const cell = grid.getCell(r, c) as
          | { char: string }
          | null
          | undefined;
        line += cell?.char ?? " ";
      }
      lines.push(line.trimEnd());
    }
    this.#log.debug(`Grid content:\n${lines.join("\n")}`);
  }

  dumpEventBusStats(stats: Record<string, number>): void {
    this.#log.debug("Event bus stats:", stats);
  }
}

const perfMarks = new Map<string, number>();

/** Mark a performance point */
export function perfMark(label: string): void {
  // Evict oldest marks if at capacity
  if (perfMarks.size >= MAX_PERF_MARKS) {
    const oldest = perfMarks.keys().next();
    if (!oldest.done) {
      perfMarks.delete(oldest.value);
      log.warn(
        `perfMarks at capacity (${MAX_PERF_MARKS}), evicted: ${oldest.value}`,
      );
    }
  }
  perfMarks.set(label, performance.now());
}

/** Measure time since a perfMark, returns ms elapsed or null if mark not found */
export function perfMeasure(label: string): number | null {
  const start = perfMarks.get(label);
  if (start === undefined) {
    log.warn(`perfMeasure: no mark found for "${label}"`);
    return null;
  }
  const elapsed = performance.now() - start;
  perfMarks.delete(label);
  log.debug(`[perf] ${label}: ${elapsed.toFixed(2)}ms`);
  return elapsed;
}

/** Clear all pending performance marks */
export function perfClear(): void {
  perfMarks.clear();
}
