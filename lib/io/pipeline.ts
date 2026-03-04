/**
 * @marauder/io — Terminal Pipeline (Standalone FFI Mode)
 *
 * Core pipeline: PTY read → parse → grid apply → event bus notify
 * This is the entry point for `deno task dev`.
 */

import { EventBus, EventType } from "@marauder/ffi-event-bus";
import { PtyManager } from "@marauder/ffi-pty";
import type { PtyConfig } from "@marauder/ffi-pty";
import { Parser } from "@marauder/ffi-parser";
import type { TerminalAction } from "@marauder/ffi-parser";
import { Grid } from "@marauder/ffi-grid";
import { ConfigStore } from "@marauder/ffi-config-store";
import type { ConfigPaths } from "@marauder/ffi-config-store";
import { Logger } from "@marauder/dev";
import { createPtyStream } from "./mod.ts";
import type { ByteStream } from "./mod.ts";

export interface PipelineConfig {
  shell?: string;
  cwd?: string;
  rows: number;
  cols: number;
  scrollback?: number;
  logLevel?: string;
  env?: Record<string, string>;
  configPaths?: ConfigPaths;
}

const DEFAULT_CONFIG: Required<Omit<PipelineConfig, "env" | "configPaths">> = {
  shell: Deno.env.get("SHELL") ?? "/bin/zsh",
  cwd: Deno.cwd(),
  rows: 24,
  cols: 80,
  scrollback: 10000,
  logLevel: "info",
};

/** Encode a Uint8Array as base64 for efficient event serialization */
function encodeBase64(data: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < data.length; i++) {
    binary += String.fromCharCode(data[i]!);
  }
  return btoa(binary);
}

export class TerminalPipeline {
  readonly eventBus: EventBus;
  readonly ptyManager: PtyManager;
  readonly parser: Parser;
  readonly grid: Grid;
  readonly configStore: ConfigStore;
  readonly #log: Logger;
  readonly #config: Required<Omit<PipelineConfig, "env" | "configPaths">>;
  readonly #env: Record<string, string> | undefined;

  #paneId: number | bigint = 0n;
  #stream: ByteStream | null = null;
  #running = false;
  #readLoop: Promise<void> | null = null;

  private constructor(
    config: PipelineConfig,
    eventBus: EventBus,
    ptyManager: PtyManager,
    parser: Parser,
    grid: Grid,
    configStore: ConfigStore,
  ) {
    this.#config = { ...DEFAULT_CONFIG, ...config };
    this.#env = config.env;
    this.eventBus = eventBus;
    this.ptyManager = ptyManager;
    this.parser = parser;
    this.grid = grid;
    this.configStore = configStore;
    this.#log = new Logger("pipeline");
  }

  static create(config: Partial<PipelineConfig> = {}): TerminalPipeline {
    const merged = { ...DEFAULT_CONFIG, ...config };
    const eventBus = new EventBus();
    const ptyManager = new PtyManager();
    const parser = new Parser();
    const grid = new Grid(merged.rows, merged.cols);
    const configStore = new ConfigStore();

    // Initialize ConfigStore with layered config paths
    const configPaths: ConfigPaths = config.configPaths ?? {
      system: "/etc/marauder/config.toml",
      user: `${Deno.env.get("HOME") ?? "~"}/.config/marauder/config.toml`,
    };
    try {
      configStore.load(configPaths);
      configStore.watch();
    } catch (err) {
      const log = new Logger("pipeline");
      log.warn("Failed to load config, using defaults", err);
    }

    return new TerminalPipeline(
      merged,
      eventBus,
      ptyManager,
      parser,
      grid,
      configStore,
    );
  }

  /** Start the read loop: PTY → Parser → Grid → EventBus */
  start(): void {
    if (this.#running) return;
    this.#running = true;

    const ptyConfig: PtyConfig = {
      shell: this.#config.shell,
      cwd: this.#config.cwd,
      env: this.#env,
      rows: this.#config.rows,
      cols: this.#config.cols,
    };

    this.#paneId = this.ptyManager.create(ptyConfig);
    this.#stream = createPtyStream(this.ptyManager, this.#paneId);
    this.#log.info(
      `Pipeline started: shell=${ptyConfig.shell} pane=${this.#paneId}`,
    );

    this.eventBus.publish(EventType.SessionCreated, {
      paneId: Number(this.#paneId),
    });

    this.#readLoop = this.#runReadLoop();
  }

  async #runReadLoop(): Promise<void> {
    if (!this.#stream) return;
    const decoder = new TextDecoder();

    for await (const chunk of this.#stream) {
      if (!this.#running) break;

      try {
        // Debug log decoded PTY output
        this.#log.debug(
          `PTY output (${chunk.length}b): ${
            decoder.decode(chunk, { stream: true })
          }`,
        );

        // Publish raw PTY output event (base64 encoded for efficiency)
        this.eventBus.publish(EventType.PtyOutput, {
          paneId: Number(this.#paneId),
          data: encodeBase64(chunk),
          length: chunk.length,
        });

        // Parse and apply to grid
        const actions: TerminalAction[] = this.parser.feed(chunk);
        for (const action of actions) {
          this.grid.applyAction(action);

          // Notify event bus of parser actions
          this.eventBus.publish(EventType.ParserAction, action);
        }

        // Notify grid updated
        const dirtyRows = this.grid.getDirtyRows();
        if (dirtyRows.length > 0) {
          this.eventBus.publish(EventType.GridUpdated, {
            dirtyRows,
            cursor: this.grid.getCursor(),
          });
        }
      } catch (err) {
        this.#log.error("Error in read loop iteration", err);
        // Continue processing — don't let one bad chunk crash the loop
      }
    }

    // PTY stream ended
    if (this.#running) {
      this.#running = false;
      try {
        this.eventBus.publish(EventType.PtyExit, {
          paneId: Number(this.#paneId),
        });
      } catch {
        // Bus may already be closed during teardown
      }
      this.#log.info("PTY stream ended");
    }
  }

  /** Write input data to the PTY */
  write(data: string | Uint8Array): number {
    return this.ptyManager.write(this.#paneId, data);
  }

  /** Resize PTY and grid, publish event */
  resize(rows: number, cols: number): void {
    this.ptyManager.resize(this.#paneId, rows, cols);
    this.grid.resize(rows, cols);
    this.eventBus.publish(EventType.GridResized, { rows, cols });
    this.#log.info(`Resized: ${rows}x${cols}`);
  }

  get paneId(): number | bigint {
    return this.#paneId;
  }

  get running(): boolean {
    return this.#running;
  }

  /** Tear down all handles in correct order */
  async destroy(): Promise<void> {
    this.#running = false;
    this.#stream?.close();

    if (this.#readLoop) {
      await this.#readLoop;
      this.#readLoop = null;
    }

    this.configStore.unwatch();
    this.ptyManager.destroy();
    this.parser.destroy();
    this.grid.destroy();
    this.configStore.destroy();
    this.eventBus.close();
    this.#log.info("Pipeline destroyed");
  }

  [Symbol.dispose](): void {
    this.#running = false;
    this.#stream?.close();
    this.configStore.unwatch();
    this.ptyManager.destroy();
    this.parser.destroy();
    this.grid.destroy();
    this.configStore.destroy();
    this.eventBus.close();
  }
}

/** Entry point for `deno task dev` */
async function main(): Promise<void> {
  const log = new Logger("main");
  log.info("Starting Marauder terminal pipeline...");

  const pipeline = TerminalPipeline.create({
    rows: 24,
    cols: 80,
  });

  // Handle SIGINT for graceful shutdown
  let shuttingDown = false;
  const sigHandler = async () => {
    if (shuttingDown) return;
    shuttingDown = true;
    log.info("Received SIGINT, shutting down...");
    await pipeline.destroy();
    Deno.exit(0);
  };
  Deno.addSignalListener("SIGINT", () => {
    sigHandler();
  });

  pipeline.start();

  // Pipe stdin to PTY
  const stdinReader = Deno.stdin.readable.getReader();
  try {
    while (pipeline.running) {
      const { done, value } = await stdinReader.read();
      if (done) break;
      pipeline.write(value);
    }
  } catch {
    // stdin closed
  } finally {
    stdinReader.releaseLock();
  }

  await pipeline.destroy();
  log.info("Marauder exited.");
}

if (import.meta.main) {
  main();
}
