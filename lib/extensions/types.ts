// lib/extensions/types.ts
// Canonical shared types for the Marauder extension system.

/** Extension manifest as declared in extension.json. */
export interface ExtensionManifest {
  name: string;
  version: string;
  description: string;
  entry: string;
  permissions?: string[];
  dependencies?: Record<string, string>;
  engines?: Record<string, string>;
  repository?: string;
  activationEvents?: string[];
}

/** Sub-interface: scoped config access for an extension. */
export interface ExtensionConfig {
  get<T>(key: string): T | undefined;
  set(key: string, value: unknown): void;
}

/** Sub-interface: filtered event bus access. */
export interface ExtensionEvents {
  on(type: string, handler: (payload: unknown) => void): () => void;
  emit(type: string, payload: unknown): void;
}

/** Sub-interface: status bar segment control. */
export interface ExtensionStatusBar {
  set(segment: "left" | "center" | "right", text: string): void;
}

/** Sub-interface: desktop notification bridge. */
export interface ExtensionNotifications {
  show(title: string, body?: string): void;
}

/** Sub-interface: command registration. */
export interface ExtensionCommands {
  register(id: string, handler: () => void): void;
}

/** Sub-interface: keybinding registration. */
export interface ExtensionKeybindings {
  register(keys: string, commandId: string): void;
}

/** Sub-interface: webview communication bridge. */
export interface ExtensionWebview {
  postMessage(type: string, data: unknown): void;
}

/** Sub-interface: custom panel registration. */
export interface ExtensionPanels {
  register(config: PanelConfig): void;
  show(id: string): void;
  hide(id: string): void;
  destroy(id: string): void;
  postMessage(id: string, type: string, data: unknown): void;
}

/** Configuration for a custom webview panel. */
export interface PanelConfig {
  id: string;
  title: string;
  html: string;
  icon?: string;
  position?: "sidebar" | "bottom" | "overlay";
}

/** The context object passed to extension activate(). */
export interface ExtensionContext {
  config: ExtensionConfig;
  events: ExtensionEvents;
  statusBar: ExtensionStatusBar;
  notifications: ExtensionNotifications;
  commands: ExtensionCommands;
  keybindings: ExtensionKeybindings;
  webview: ExtensionWebview;
  panels: ExtensionPanels;
}

/** Current extension API version. Extensions declare compatible versions via engines.marauder. */
export const EXTENSION_API_VERSION = "1.0.0";

/** Extension lifecycle state. */
export type ExtensionState = "loaded" | "active" | "error" | "disabled";

/** Runtime info about a loaded extension. */
export interface ExtensionInfo {
  manifest: ExtensionManifest;
  state: ExtensionState;
  error?: string;
  /** Directory path where the extension resides. */
  dir: string;
}

/** An extension module's exports. */
export interface ExtensionModule {
  activate(ctx: ExtensionContext): void | Promise<void>;
  deactivate(): void | Promise<void>;
}

/** Message payload for extension-to-extension or extension-to-webview communication. */
export interface ExtensionMessagePayload {
  source: string;
  type: string;
  payload?: unknown;
}
