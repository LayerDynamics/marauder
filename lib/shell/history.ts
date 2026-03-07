/**
 * @marauder/shell — Command history management with search
 */

import type { CommandRecord } from "./mod.ts";

export interface HistoryConfig {
  maxSize: number;
}

const DEFAULT_CONFIG: HistoryConfig = {
  maxSize: 10000,
};

export interface FuzzyMatch {
  record: CommandRecord;
  score: number;
}

export class CommandHistory {
  readonly #buffer: (CommandRecord | undefined)[];
  readonly #config: HistoryConfig;
  #head = 0;  // Next write position
  #count = 0; // Current number of records

  constructor(config: Partial<HistoryConfig> = {}) {
    this.#config = { ...DEFAULT_CONFIG, ...config };
    this.#buffer = new Array(this.#config.maxSize);
  }

  /** Add a command record. O(1) eviction via ring buffer. */
  add(record: CommandRecord): void {
    this.#buffer[this.#head] = record;
    this.#head = (this.#head + 1) % this.#config.maxSize;
    if (this.#count < this.#config.maxSize) {
      this.#count++;
    }
  }

  /** Get record at logical index (0 = oldest). */
  #at(logicalIndex: number): CommandRecord {
    const start = this.#count < this.#config.maxSize ? 0 : this.#head;
    const physicalIndex = (start + logicalIndex) % this.#config.maxSize;
    return this.#buffer[physicalIndex]!;
  }

  /** Get all records, newest last. */
  getAll(): CommandRecord[] {
    const result: CommandRecord[] = [];
    for (let i = 0; i < this.#count; i++) {
      result.push(this.#at(i));
    }
    return result;
  }

  /** Get the last N records, newest last. */
  getLast(n: number): CommandRecord[] {
    const start = Math.max(0, this.#count - n);
    const result: CommandRecord[] = [];
    for (let i = start; i < this.#count; i++) {
      result.push(this.#at(i));
    }
    return result;
  }

  /** Clear all history. */
  clear(): void {
    this.#buffer.fill(undefined);
    this.#head = 0;
    this.#count = 0;
  }

  /** Number of records. */
  get size(): number {
    return this.#count;
  }

  /** Substring search sorted by recency (newest first). */
  search(query: string): CommandRecord[] {
    const lower = query.toLowerCase();
    const results: CommandRecord[] = [];
    for (let i = this.#count - 1; i >= 0; i--) {
      const rec = this.#at(i);
      if (rec.command.toLowerCase().includes(lower)) {
        results.push(rec);
      }
    }
    return results;
  }

  /** Fuzzy search with character-by-character scoring, sorted by score descending.
   *  Empty query returns all records sorted by recency (newest first). */
  fuzzySearch(query: string): FuzzyMatch[] {
    const lower = query.toLowerCase();
    const results: FuzzyMatch[] = [];

    if (lower.length === 0) {
      // Return all records sorted by recency
      for (let i = this.#count - 1; i >= 0; i--) {
        results.push({ record: this.#at(i), score: i });
      }
      return results;
    }

    for (let i = this.#count - 1; i >= 0; i--) {
      const record = this.#at(i);
      const score = fuzzyScore(lower, record.command.toLowerCase());
      if (score > 0) {
        // Boost recent entries
        const recencyBonus = (i / this.#count) * 10;
        results.push({ record, score: score + recencyBonus });
      }
    }

    results.sort((a, b) => b.score - a.score);
    return results;
  }

  /** Filter records by exit code. */
  getByExitCode(code: number): CommandRecord[] {
    const results: CommandRecord[] = [];
    for (let i = 0; i < this.#count; i++) {
      const rec = this.#at(i);
      if (rec.exitCode === code) results.push(rec);
    }
    return results;
  }

  /** Filter records by working directory. */
  getByDirectory(cwd: string): CommandRecord[] {
    const results: CommandRecord[] = [];
    for (let i = 0; i < this.#count; i++) {
      const rec = this.#at(i);
      if (rec.cwd === cwd) results.push(rec);
    }
    return results;
  }
}

/** Character-by-character fuzzy match scoring. Returns 0 for no match. */
function fuzzyScore(query: string, target: string): number {
  if (query.length === 0) return 0;
  if (target.length === 0) return 0;

  let score = 0;
  let qi = 0;
  let prevMatchIdx = -1;

  for (let ti = 0; ti < target.length && qi < query.length; ti++) {
    if (target[ti] === query[qi]) {
      score += 1;
      // Consecutive match bonus
      if (prevMatchIdx === ti - 1) {
        score += 2;
      }
      // Word boundary bonus
      if (ti === 0 || target[ti - 1] === " " || target[ti - 1] === "/") {
        score += 3;
      }
      prevMatchIdx = ti;
      qi++;
    }
  }

  // All query chars must match
  return qi === query.length ? score : 0;
}
