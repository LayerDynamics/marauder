// lib/extensions/mod.ts
// Barrel export for the Marauder extension system.

export type {
  ExtensionManifest,
  ExtensionContext,
  ExtensionConfig,
  ExtensionEvents,
  ExtensionStatusBar,
  ExtensionNotifications,
  ExtensionCommands,
  ExtensionKeybindings,
  ExtensionWebview,
  ExtensionPanels,
  ExtensionState,
  ExtensionInfo,
  ExtensionModule,
  ExtensionMessagePayload,
  PanelConfig,
} from "./types.ts";

export { ExtensionRegistry } from "./registry.ts";
export { ExtensionLoader, discover, validateManifest } from "./loader.ts";
export { createExtensionContext } from "./context.ts";
export type { RuntimeServices } from "./context.ts";
export { CommandRegistry } from "./commands.ts";
export { KeybindingRegistry } from "./keybindings.ts";
export { PanelRegistry } from "./panels.ts";
export type { PanelEvent } from "./panels.ts";
export { ExtensionWatcher } from "./watcher.ts";
export { ExtensionBridgeServer } from "./bridge.ts";
export type { ExtensionBridgeMessage } from "./bridge.ts";
export {
  safeActivate,
  safeDeactivate,
  safeHandler,
  clearErrors,
  isCircuitBroken,
} from "./isolation.ts";
export { installFromPath, installFromGit } from "./installer.ts";
export type { InstallResult } from "./installer.ts";
