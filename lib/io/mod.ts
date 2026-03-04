/**
 * @marauder/io — Stream Types + Buffer Utilities
 */

import type { PtyManager } from "@marauder/ffi-pty";

/**
 * Fixed-size circular byte buffer for PTY output batching.
 */
export class CircularBuffer {
  readonly #buf: Uint8Array;
  readonly #capacity: number;
  #readPos = 0;
  #writePos = 0;
  #length = 0;

  constructor(capacity: number) {
    this.#capacity = capacity;
    this.#buf = new Uint8Array(capacity);
  }

  get length(): number {
    return this.#length;
  }

  get capacity(): number {
    return this.#capacity;
  }

  get available(): number {
    return this.#capacity - this.#length;
  }

  /** Write bytes into the buffer. Returns number of bytes actually written. */
  write(data: Uint8Array): number {
    const toWrite = Math.min(data.length, this.available);
    if (toWrite === 0) return 0;

    const firstChunk = Math.min(toWrite, this.#capacity - this.#writePos);
    this.#buf.set(data.subarray(0, firstChunk), this.#writePos);

    if (firstChunk < toWrite) {
      this.#buf.set(data.subarray(firstChunk, toWrite), 0);
    }

    this.#writePos = (this.#writePos + toWrite) % this.#capacity;
    this.#length += toWrite;
    return toWrite;
  }

  /** Read up to maxBytes from the buffer. Returns a new Uint8Array with the data. */
  read(maxBytes: number): Uint8Array {
    const toRead = Math.min(maxBytes, this.#length);
    if (toRead === 0) return new Uint8Array(0);

    const out = new Uint8Array(toRead);
    const firstChunk = Math.min(toRead, this.#capacity - this.#readPos);
    out.set(this.#buf.subarray(this.#readPos, this.#readPos + firstChunk));

    if (firstChunk < toRead) {
      out.set(this.#buf.subarray(0, toRead - firstChunk), firstChunk);
    }

    this.#readPos = (this.#readPos + toRead) % this.#capacity;
    this.#length -= toRead;
    return out;
  }

  /** Drain all buffered data. */
  drain(): Uint8Array {
    return this.read(this.#length);
  }

  /** Reset buffer to empty state. */
  clear(): void {
    this.#readPos = 0;
    this.#writePos = 0;
    this.#length = 0;
  }
}

/**
 * Async iterable stream wrapping PTY reads.
 */
export class ByteStream implements AsyncIterable<Uint8Array> {
  readonly #pty: PtyManager;
  readonly #paneId: number | bigint;
  readonly #bufSize: number;
  #closed = false;
  #emptyReads = 0;

  constructor(pty: PtyManager, paneId: number | bigint, bufSize = 4096) {
    this.#pty = pty;
    this.#paneId = paneId;
    this.#bufSize = bufSize;
  }

  close(): void {
    this.#closed = true;
  }

  async *[Symbol.asyncIterator](): AsyncGenerator<Uint8Array> {
    while (!this.#closed) {
      try {
        const data = this.#pty.read(this.#paneId, this.#bufSize);
        if (data.length > 0) {
          this.#emptyReads = 0;
          yield data;
        } else {
          // Exponential backoff on empty reads (2ms → 4ms → 8ms → ... → 50ms cap)
          // PTY read should block per FFI docs, so empty reads indicate edge cases
          this.#emptyReads = Math.min(this.#emptyReads + 1, 6);
          const delay = Math.min(1 << this.#emptyReads, 50);
          await new Promise((r) => setTimeout(r, delay));
        }
      } catch {
        // PTY closed or errored
        this.#closed = true;
        break;
      }
    }
  }
}

/**
 * Create a PTY output stream as an async generator.
 */
export function createPtyStream(
  pty: PtyManager,
  paneId: number | bigint,
  bufSize = 4096,
): ByteStream {
  return new ByteStream(pty, paneId, bufSize);
}

export type { PtyManager };
