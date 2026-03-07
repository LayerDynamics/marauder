// lib/extensions/commands.ts
// Central command registry for extensions.

/** A registered command entry. */
interface CommandEntry {
  id: string;
  handler: () => void;
  extensionName: string;
}

/** Central registry for extension-contributed commands. */
export class CommandRegistry {
  readonly #commands: Map<string, CommandEntry> = new Map();

  /** Register a command. Throws if the command ID is already registered. */
  register(id: string, handler: () => void, extensionName: string): void {
    if (this.#commands.has(id)) {
      throw new Error(
        `Command "${id}" is already registered by extension "${this.#commands.get(id)!.extensionName}"`,
      );
    }
    this.#commands.set(id, { id, handler, extensionName });
  }

  /** Execute a registered command by ID. Returns false if not found. */
  execute(id: string): boolean {
    const entry = this.#commands.get(id);
    if (!entry) return false;
    entry.handler();
    return true;
  }

  /** Check if a command is registered. */
  has(id: string): boolean {
    return this.#commands.has(id);
  }

  /** Unregister all commands belonging to a given extension. */
  unregisterAll(extensionName: string): void {
    for (const [id, entry] of this.#commands) {
      if (entry.extensionName === extensionName) {
        this.#commands.delete(id);
      }
    }
  }

  /** List all registered command IDs. */
  list(): Array<{ id: string; extensionName: string }> {
    return [...this.#commands.values()].map((e) => ({
      id: e.id,
      extensionName: e.extensionName,
    }));
  }
}
