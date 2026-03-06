/**
 * @marauder/ui — Frame loop orchestrator
 *
 * Manages the Deno-side frame tick for polling events, config changes,
 * and UI state updates. Does NOT drive GPU rendering (Rust owns that).
 */

/** Frame timing statistics. */
export interface FrameStats {
  /** Current measured FPS. */
  fps: number;
  /** Average frame callback time in milliseconds. */
  frameTimeMs: number;
  /** Total frames since start. */
  totalFrames: number;
}

/**
 * Deno-side frame loop for orchestration tasks.
 *
 * This runs at a configurable rate to handle:
 * - Event bus polling
 * - Config change checks
 * - UI state synchronization
 * - Telemetry collection
 *
 * GPU rendering is driven by the Rust render thread, not this loop.
 */
export class FrameLoop {
  #targetFps: number;
  #running = false;
  #intervalId: number | null = null;
  #frameCallback: (() => void) | null = null;

  // Stats tracking
  #frameCount = 0;
  #totalFrames = 0;
  #lastFpsTime = 0;
  #currentFps = 0;
  #lastFrameTime = 0;
  #avgFrameTimeMs = 0;

  constructor(targetFps = 120) {
    this.#targetFps = targetFps;
  }

  /** Start the frame loop with the given per-tick callback. */
  start(callback: () => void): void {
    if (this.#running) return;

    this.#running = true;
    this.#frameCallback = callback;
    this.#lastFpsTime = performance.now();
    this.#frameCount = 0;

    const intervalMs = Math.max(1, Math.floor(1000 / this.#targetFps));

    this.#intervalId = setInterval(() => {
      this.#tick();
    }, intervalMs);
  }

  /** Stop the frame loop. */
  stop(): void {
    if (!this.#running) return;

    this.#running = false;
    if (this.#intervalId !== null) {
      clearInterval(this.#intervalId);
      this.#intervalId = null;
    }
    this.#frameCallback = null;
  }

  /** Update the target frame rate. Restarts the interval if running. */
  setTargetFps(fps: number): void {
    this.#targetFps = Math.max(1, fps);

    if (this.#running && this.#frameCallback) {
      const cb = this.#frameCallback;
      this.stop();
      this.start(cb);
    }
  }

  /** Get the current target FPS. */
  get targetFps(): number {
    return this.#targetFps;
  }

  /** Whether the loop is currently running. */
  get running(): boolean {
    return this.#running;
  }

  /** Get current frame statistics. */
  getFrameStats(): FrameStats {
    return {
      fps: this.#currentFps,
      frameTimeMs: this.#avgFrameTimeMs,
      totalFrames: this.#totalFrames,
    };
  }

  #tick(): void {
    const now = performance.now();

    // Measure callback time
    const startTime = now;
    try {
      this.#frameCallback?.();
    } catch {
      // Don't let callback errors kill the loop
    }
    this.#lastFrameTime = performance.now() - startTime;

    // Exponential moving average of frame time
    this.#avgFrameTimeMs = this.#avgFrameTimeMs * 0.9 + this.#lastFrameTime * 0.1;

    // FPS calculation (once per second)
    this.#frameCount++;
    this.#totalFrames++;
    const elapsed = now - this.#lastFpsTime;
    if (elapsed >= 1000) {
      this.#currentFps = Math.round((this.#frameCount * 1000) / elapsed);
      this.#frameCount = 0;
      this.#lastFpsTime = now;
    }
  }
}
