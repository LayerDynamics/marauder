/**
 * @marauder/shell/zones — Command zone tracking and ShellCommandFinished events.
 *
 * Provides a lightweight zone tracker that emits ShellCommandFinished with
 * timing and exit code information when a command zone completes.
 */

import type { EventBus } from "@marauder/ffi-event-bus";
import { EventType } from "@marauder/ffi-event-bus";

export interface CommandZone {
  command: string;
  cwd: string;
  paneId: string;
  startTime: number;
}

/**
 * Track active command zones and emit ShellCommandFinished when they complete.
 *
 * Usage: call `startCommand()` when a command begins executing (OSC 133;C),
 * and `finishCommand()` when it completes (OSC 133;D).
 */
export class ZoneTracker {
  readonly #eventBus: EventBus;
  readonly #activeZones = new Map<string, CommandZone>();

  constructor(eventBus: EventBus) {
    this.#eventBus = eventBus;
  }

  /** Record a command starting execution in a pane. */
  startCommand(paneId: string, command: string, cwd: string): void {
    this.#activeZones.set(paneId, {
      command,
      cwd,
      paneId,
      startTime: Date.now(),
    });
  }

  /** Record a command finishing and emit ShellCommandFinished event. */
  finishCommand(paneId: string, exitCode: number): void {
    const zone = this.#activeZones.get(paneId);
    if (!zone) return;
    this.#activeZones.delete(paneId);

    const durationMs = Date.now() - zone.startTime;
    this.#eventBus.publish(EventType.ShellCommandFinished, {
      command: zone.command,
      exitCode,
      durationMs,
      paneId: zone.paneId,
    });
  }

  /** Check if a pane has an active command. */
  hasActiveCommand(paneId: string): boolean {
    return this.#activeZones.has(paneId);
  }

  /** Get the active command zone for a pane. */
  getActiveZone(paneId: string): CommandZone | undefined {
    return this.#activeZones.get(paneId);
  }

  /** Remove a pane's zone entry on pane close to prevent leaks. */
  removePane(paneId: string): void {
    this.#activeZones.delete(paneId);
  }
}
