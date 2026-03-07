// lib/extensions/context.ts
// Factory for creating ExtensionContext instances wired to runtime services.

import type {
  ExtensionContext,
  ExtensionManifest,
  PanelConfig,
} from "./types.ts";
import { CommandRegistry } from "./commands.ts";
import { KeybindingRegistry } from "./keybindings.ts";
import { PanelRegistry } from "./panels.ts";
import { safeHandler } from "./isolation.ts";

/** Runtime services that the context factory wires into each extension. */
export interface RuntimeServices {
  /** Config get/set scoped by extension name. */
  configGet: (key: string) => unknown | undefined;
  configSet: (key: string, value: unknown) => void;

  /** Event bus subscribe/emit. */
  eventOn: (
    type: string,
    handler: (payload: unknown) => void,
  ) => () => void;
  eventEmit: (type: string, payload: unknown) => void;

  /** Status bar bridge. */
  statusBarSet: (
    segment: "left" | "center" | "right",
    text: string,
  ) => void;

  /** Notification bridge. */
  notificationShow: (title: string, body?: string) => void;

  /** Webview message bridge. */
  webviewPostMessage: (type: string, data: unknown) => void;

  /** Callback when circuit breaker trips. */
  onCircuitBreak?: (extensionName: string) => void;
}

/**
 * Create an ExtensionContext for a specific extension, wired to real runtime
 * services via the provided RuntimeServices.
 */
export function createExtensionContext(
  manifest: ExtensionManifest,
  services: RuntimeServices,
  commandRegistry: CommandRegistry,
  keybindingRegistry: KeybindingRegistry,
  panelRegistry: PanelRegistry,
): ExtensionContext {
  const extName = manifest.name;

  // Track unsub functions so we can clean up on deactivate
  const unsubscribers: Array<() => void> = [];

  const config = {
    get<T>(key: string): T | undefined {
      return services.configGet(`${extName}.${key}`) as T | undefined;
    },
    set(key: string, value: unknown): void {
      services.configSet(`${extName}.${key}`, value);
    },
  };

  const events = {
    on(type: string, handler: (payload: unknown) => void): () => void {
      const wrapped = safeHandler(extName, handler, services.onCircuitBreak);
      const unsub = services.eventOn(type, wrapped);
      unsubscribers.push(unsub);
      return unsub;
    },
    emit(type: string, payload: unknown): void {
      services.eventEmit(type, payload);
    },
  };

  const statusBar = {
    set(segment: "left" | "center" | "right", text: string): void {
      services.statusBarSet(segment, text);
    },
  };

  const notifications = {
    show(title: string, body?: string): void {
      services.notificationShow(title, body);
    },
  };

  const commands = {
    register(id: string, handler: () => void): void {
      commandRegistry.register(id, handler, extName);
    },
  };

  const keybindings = {
    register(keys: string, commandId: string): void {
      keybindingRegistry.register(keys, commandId, extName);
    },
  };

  const webview = {
    postMessage(type: string, data: unknown): void {
      services.webviewPostMessage(type, data);
    },
  };

  const panels = {
    register(panelConfig: PanelConfig): void {
      panelRegistry.register(extName, panelConfig);
    },
    show(id: string): void {
      panelRegistry.show(id);
    },
    hide(id: string): void {
      panelRegistry.hide(id);
    },
    destroy(id: string): void {
      panelRegistry.destroy(id);
    },
    postMessage(id: string, type: string, data: unknown): void {
      panelRegistry.postMessage(id, type, data);
    },
  };

  return {
    config,
    events,
    statusBar,
    notifications,
    commands,
    keybindings,
    webview,
    panels,
  };
}

/** Get the list of unsub functions that were tracked (for cleanup). */
export function getContextCleanups(
  _ctx: ExtensionContext,
): Array<() => void> {
  // The cleanup is managed via the registry's addCleanup mechanism.
  // This function exists for future use if needed.
  return [];
}
