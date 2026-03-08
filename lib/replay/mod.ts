/**
 * Replay engine — plays back recorded terminal sessions with speed control,
 * seeking, and pause support.
 *
 * Supports asciinema v2 (.cast) format: a JSON header line followed by
 * event lines as [time, event_type, data] tuples.
 */

/** A single replay event parsed from a .cast file. */
export interface ReplayEvent {
  time: number;
  eventType: string;
  data: string;
}

/** Replay session metadata from the .cast header. */
export interface ReplayHeader {
  version: number;
  width: number;
  height: number;
  timestamp?: number;
  title?: string;
  env?: Record<string, string>;
}

/** Replay state. */
export type ReplayState = "idle" | "playing" | "paused" | "finished";

/** Callback for replayed output events. */
export type ReplayOutputCallback = (data: string, time: number) => void;

/** Callback for replay state changes. */
export type ReplayStateCallback = (state: ReplayState) => void;

/**
 * Terminal session replay engine.
 *
 * Parses asciinema v2 .cast files and replays them with configurable
 * playback speed, seeking, and pause/resume.
 */
export class ReplayEngine {
  #header: ReplayHeader | null = null;
  #events: ReplayEvent[] = [];
  #state: ReplayState = "idle";
  #speed = 1.0;
  #currentIndex = 0;
  #startTime = 0;
  #pausedAt = 0;
  #timerId: ReturnType<typeof setTimeout> | null = null;
  #onOutput: ReplayOutputCallback | null = null;
  #onStateChange: ReplayStateCallback | null = null;

  /** Parse a .cast file content string. */
  load(content: string): void {
    this.stop();
    const lines = content.split("\n").filter((l) => l.trim().length > 0);
    if (lines.length === 0) throw new Error("Empty .cast file");

    // First line is the header JSON
    this.#header = JSON.parse(lines[0]) as ReplayHeader;
    if (this.#header.version !== 2) {
      throw new Error(`Unsupported asciinema version: ${this.#header.version}`);
    }

    // Remaining lines are event tuples
    this.#events = [];
    for (let i = 1; i < lines.length; i++) {
      const tuple = JSON.parse(lines[i]) as [number, string, string];
      this.#events.push({
        time: tuple[0],
        eventType: tuple[1],
        data: tuple[2],
      });
    }

    this.#currentIndex = 0;
    this.#setState("idle");
  }

  /** Get session header. */
  get header(): ReplayHeader | null {
    return this.#header;
  }

  /** Get total duration in seconds. */
  get duration(): number {
    if (this.#events.length === 0) return 0;
    return this.#events[this.#events.length - 1].time;
  }

  /** Get current playback position in seconds. */
  get position(): number {
    if (this.#state === "idle") return 0;
    if (this.#state === "paused") return this.#pausedAt;
    if (this.#currentIndex >= this.#events.length) return this.duration;
    return this.#events[this.#currentIndex]?.time ?? 0;
  }

  /** Get current playback speed. */
  get speed(): number {
    return this.#speed;
  }

  /** Set playback speed (0.25 to 16). */
  set speed(s: number) {
    this.#speed = Math.max(0.25, Math.min(16, s));
  }

  /** Get current state. */
  get state(): ReplayState {
    return this.#state;
  }

  /** Get total event count. */
  get eventCount(): number {
    return this.#events.length;
  }

  /** Register output callback. */
  onOutput(cb: ReplayOutputCallback): void {
    this.#onOutput = cb;
  }

  /** Register state change callback. */
  onStateChange(cb: ReplayStateCallback): void {
    this.#onStateChange = cb;
  }

  /** Start or resume playback. */
  play(): void {
    if (this.#events.length === 0) return;

    if (this.#state === "paused") {
      // Resume from paused position
      this.#setState("playing");
      this.#scheduleNext();
      return;
    }

    // Start from beginning or current index
    this.#startTime = performance.now();
    this.#setState("playing");
    this.#scheduleNext();
  }

  /** Pause playback. */
  pause(): void {
    if (this.#state !== "playing") return;
    if (this.#timerId !== null) {
      clearTimeout(this.#timerId);
      this.#timerId = null;
    }
    this.#pausedAt = this.position;
    this.#setState("paused");
  }

  /** Stop playback and reset to beginning. */
  stop(): void {
    if (this.#timerId !== null) {
      clearTimeout(this.#timerId);
      this.#timerId = null;
    }
    this.#currentIndex = 0;
    this.#pausedAt = 0;
    this.#setState("idle");
  }

  /** Seek to a specific time in seconds. */
  seek(time: number): void {
    const wasPlaying = this.#state === "playing";
    if (this.#timerId !== null) {
      clearTimeout(this.#timerId);
      this.#timerId = null;
    }

    // Find the event index closest to the target time
    this.#currentIndex = 0;
    for (let i = 0; i < this.#events.length; i++) {
      if (this.#events[i].time > time) break;
      this.#currentIndex = i;
    }

    // Replay all events up to the seek point (instant, for screen state)
    for (let i = 0; i <= this.#currentIndex; i++) {
      const ev = this.#events[i];
      if (ev.eventType === "o" && this.#onOutput) {
        this.#onOutput(ev.data, ev.time);
      }
    }
    this.#currentIndex++;

    this.#pausedAt = time;
    if (wasPlaying) {
      this.#setState("playing");
      this.#scheduleNext();
    } else {
      this.#setState("paused");
    }
  }

  /** Search events for a text pattern. Returns matching event indices and times. */
  search(pattern: string): Array<{ index: number; time: number; snippet: string }> {
    const results: Array<{ index: number; time: number; snippet: string }> = [];
    const lower = pattern.toLowerCase();
    for (let i = 0; i < this.#events.length; i++) {
      const ev = this.#events[i];
      const idx = ev.data.toLowerCase().indexOf(lower);
      if (idx !== -1) {
        const start = Math.max(0, idx - 20);
        const end = Math.min(ev.data.length, idx + pattern.length + 20);
        results.push({
          index: i,
          time: ev.time,
          snippet: ev.data.slice(start, end),
        });
      }
    }
    return results;
  }

  #scheduleNext(): void {
    if (this.#state !== "playing") return;
    if (this.#currentIndex >= this.#events.length) {
      this.#setState("finished");
      return;
    }

    const event = this.#events[this.#currentIndex];
    const prevTime = this.#currentIndex > 0
      ? this.#events[this.#currentIndex - 1].time
      : (this.#pausedAt || 0);
    const delay = Math.max(0, (event.time - prevTime) / this.#speed);

    // Cap individual delays to 2 seconds (skip long idle periods)
    const cappedDelay = Math.min(delay * 1000, 2000);

    this.#timerId = setTimeout(() => {
      this.#timerId = null;
      if (this.#state !== "playing") return;

      if (event.eventType === "o" && this.#onOutput) {
        this.#onOutput(event.data, event.time);
      }
      this.#currentIndex++;
      this.#scheduleNext();
    }, cappedDelay);
  }

  #setState(state: ReplayState): void {
    this.#state = state;
    this.#onStateChange?.(state);
  }
}
