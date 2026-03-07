// lib/extensions/keybindings.ts
// Central keybinding registry for extensions.

/** A registered keybinding entry. */
interface KeybindingEntry {
  keys: string;
  commandId: string;
  extensionName: string;
}

/** Central registry for extension-contributed keybindings. */
export class KeybindingRegistry {
  readonly #bindings: Map<string, KeybindingEntry> = new Map();

  /** Register a keybinding. The keys string is normalized to lowercase for matching. */
  register(keys: string, commandId: string, extensionName: string): void {
    const normalized = keys.toLowerCase();
    this.#bindings.set(normalized, { keys, commandId, extensionName });
  }

  /** Resolve a key combination to a command ID. Returns undefined if no match. */
  resolve(keys: string): string | undefined {
    return this.#bindings.get(keys.toLowerCase())?.commandId;
  }

  /** Unregister all keybindings for a given extension. */
  unregisterAll(extensionName: string): void {
    for (const [key, entry] of this.#bindings) {
      if (entry.extensionName === extensionName) {
        this.#bindings.delete(key);
      }
    }
  }

  /** List all registered keybindings. */
  list(): Array<{ keys: string; commandId: string; extensionName: string }> {
    return [...this.#bindings.values()];
  }
}
