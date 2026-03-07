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
/** Known permission strings that gate access to context capabilities. */
export type ExtensionPermission =
  | "config"
  | "events"
  | "statusBar"
  | "notifications"
  | "commands"
  | "keybindings"
  | "webview"
  | "panels";

/** All permissions — granted to bundled extensions that declare no permissions. */
const ALL_PERMISSIONS: ReadonlySet<ExtensionPermission> = new Set([
  "config", "events", "statusBar", "notifications",
  "commands", "keybindings", "webview", "panels",
]);

function resolvePermissions(manifest: ExtensionManifest): ReadonlySet<string> {
  // No permissions declared = bundled extension, grant all for backwards compatibility.
  if (!manifest.permissions || manifest.permissions.length === 0) {
    return ALL_PERMISSIONS;
  }
  return new Set(manifest.permissions);
}

function denyAccess(extName: string, capability: string): never {
  throw new Error(
    `Extension "${extName}" does not have the "${capability}" permission`
  );
}

export function createExtensionContext(
  manifest: ExtensionManifest,
  services: RuntimeServices,
  commandRegistry: CommandRegistry,
  keybindingRegistry: KeybindingRegistry,
  panelRegistry: PanelRegistry,
): ExtensionContext {
  const extName = manifest.name;
  const granted = resolvePermissions(manifest);

  // Track unsub functions so we can clean up on deactivate
  const unsubscribers: Array<() => void> = [];

  const config = {
    get<T>(key: string): T | undefined {
      if (!granted.has("config")) denyAccess(extName, "config");
      return services.configGet(`${extName}.${key}`) as T | undefined;
    },
    set(key: string, value: unknown): void {
      if (!granted.has("config")) denyAccess(extName, "config");
      services.configSet(`${extName}.${key}`, value);
    },
  };

  const events = {
    on(type: string, handler: (payload: unknown) => void): () => void {
      if (!granted.has("events")) denyAccess(extName, "events");
      const wrapped = safeHandler(extName, handler, services.onCircuitBreak);
      const unsub = services.eventOn(type, wrapped);
      unsubscribers.push(unsub);
      return unsub;
    },
    emit(type: string, payload: unknown): void {
      if (!granted.has("events")) denyAccess(extName, "events");
      services.eventEmit(type, payload);
    },
  };

  const statusBar = {
    set(segment: "left" | "center" | "right", text: string): void {
      if (!granted.has("statusBar")) denyAccess(extName, "statusBar");
      services.statusBarSet(segment, text);
    },
  };

  const notifications = {
    show(title: string, body?: string): void {
      if (!granted.has("notifications")) denyAccess(extName, "notifications");
      services.notificationShow(title, body);
    },
  };

  const commands = {
    register(id: string, handler: () => void): void {
      if (!granted.has("commands")) denyAccess(extName, "commands");
      commandRegistry.register(id, handler, extName);
    },
  };

  const keybindings = {
    register(keys: string, commandId: string): void {
      if (!granted.has("keybindings")) denyAccess(extName, "keybindings");
      keybindingRegistry.register(keys, commandId, extName);
    },
  };

  const webview = {
    postMessage(type: string, data: unknown): void {
      if (!granted.has("webview")) denyAccess(extName, "webview");
      services.webviewPostMessage(type, data);
    },
  };

  const panels = {
    register(panelConfig: PanelConfig): void {
      if (!granted.has("panels")) denyAccess(extName, "panels");
      panelRegistry.register(extName, panelConfig);
    },
    show(id: string): void {
      if (!granted.has("panels")) denyAccess(extName, "panels");
      panelRegistry.show(id);
    },
    hide(id: string): void {
      if (!granted.has("panels")) denyAccess(extName, "panels");
      panelRegistry.hide(id);
    },
    destroy(id: string): void {
      if (!granted.has("panels")) denyAccess(extName, "panels");
      panelRegistry.destroy(id);
    },
    postMessage(id: string, type: string, data: unknown): void {
      if (!granted.has("panels")) denyAccess(extName, "panels");
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
