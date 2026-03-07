# Marauder Extension Developer Guide

## Overview

Marauder extensions are TypeScript packages that integrate with the terminal runtime through a structured API. Each extension has a manifest (`extension.json`), an entry module (`mod.ts`), and access to a rich `ExtensionContext` during activation.

## Extension Manifest (`extension.json`)

```json
{
  "name": "my-extension",
  "version": "1.0.0",
  "description": "What this extension does",
  "entry": "mod.ts",
  "permissions": ["notifications", "shell"],
  "engines": {
    "marauder": "^1.0.0"
  },
  "repository": "https://github.com/user/my-extension",
  "activationEvents": ["onStartup"],
  "dependencies": {}
}
```

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | `string` | Yes | Unique extension identifier |
| `version` | `string` | Yes | Semver version |
| `description` | `string` | Yes | Human-readable description |
| `entry` | `string` | Yes | Entry module path (relative to extension dir) |
| `permissions` | `string[]` | No | Requested permissions |
| `engines` | `Record<string, string>` | No | Engine compatibility (`marauder: "^1.0.0"`) |
| `repository` | `string` | No | Source repository URL |
| `activationEvents` | `string[]` | No | Events that trigger activation |
| `dependencies` | `Record<string, string>` | No | Other extension dependencies |

## Extension Module

Every extension must export `activate()` and `deactivate()`:

```typescript
import type { ExtensionContext } from "../../lib/extensions/types.ts";

export function activate(ctx: ExtensionContext): void {
  // Setup: register commands, subscribe to events, etc.
}

export function deactivate(): void {
  // Cleanup
}
```

## ExtensionContext API

The `ctx` object passed to `activate()` provides:

### `ctx.config`

Scoped configuration access for the extension.

```typescript
const theme = ctx.config.get<string>("colorScheme");
ctx.config.set("colorScheme", "dark");
```

### `ctx.events`

Filtered event bus — subscribe to and emit events.

```typescript
const unsub = ctx.events.on("TerminalOutput", (payload) => {
  // Handle event
});
ctx.events.emit("MyCustomEvent", { data: "value" });
```

### `ctx.statusBar`

Control status bar segments.

```typescript
ctx.statusBar.set("left", "🔌 Connected");
ctx.statusBar.set("right", "Ln 42, Col 7");
```

### `ctx.notifications`

Desktop notification bridge.

```typescript
ctx.notifications.show("Build Complete", "All tests passed");
```

### `ctx.commands`

Register commands that can be invoked from the command palette or keybindings.

```typescript
ctx.commands.register("my-ext.doThing", () => {
  // Command logic
});
```

### `ctx.keybindings`

Register keyboard shortcuts.

```typescript
ctx.keybindings.register("ctrl+shift+t", "my-ext.doThing");
```

### `ctx.webview`

Send messages to the Tauri webview.

```typescript
ctx.webview.postMessage("updateUI", { count: 42 });
```

### `ctx.panels`

Register custom UI panels.

```typescript
ctx.panels.register({
  id: "my-panel",
  title: "My Panel",
  html: "<div>Panel content</div>",
  position: "sidebar",
});
ctx.panels.show("my-panel");
```

## Extension Lifecycle

```
discovered → loaded → active
                  ↓
               error ← activation failure
                  ↓
              disabled ← user toggle
```

1. **Discovery**: Extension directories scanned from `extensions/` (bundled) and `~/.config/marauder/extensions/` (user-installed)
2. **Loading**: Manifest validated, entry module imported
3. **API Version Check**: `engines.marauder` checked against current API version
4. **Activation**: `activate(ctx)` called with 5-second timeout and error isolation
5. **Error**: If activation throws or times out, state set to `"error"` with message
6. **Disabled**: User can toggle via `marauder-ext disable <name>`

## CLI Usage

### Install an extension

```bash
# From local path (creates symlink)
marauder-ext install ./path/to/extension

# From git repository (clones)
marauder-ext install https://github.com/user/marauder-ext-foo.git
```

### Uninstall

```bash
marauder-ext uninstall my-extension
```

### List installed extensions

```bash
marauder-ext list
```

### Create a new extension

```bash
marauder-ext create my-extension
# Creates extensions/my-extension/ with scaffold files
```

### Show extension details

```bash
marauder-ext info my-extension
```

### Enable / Disable

```bash
marauder-ext enable my-extension
marauder-ext disable my-extension
```

## Example: Status Bar Extension

```typescript
import type { ExtensionContext } from "../../lib/extensions/types.ts";

export function activate(ctx: ExtensionContext): void {
  // Show git branch in status bar
  ctx.statusBar.set("left", "main");

  // Update on directory change
  ctx.events.on("DirectoryChanged", (payload) => {
    const dir = payload as { path: string };
    ctx.statusBar.set("center", dir.path);
  });

  // Register a command
  ctx.commands.register("status-bar.refresh", () => {
    ctx.statusBar.set("right", new Date().toLocaleTimeString());
  });

  ctx.keybindings.register("ctrl+shift+s", "status-bar.refresh");
}

export function deactivate(): void {
  // Cleanup handled automatically by the runtime
}
```

## Bundled Extensions

| Extension | Description |
|-----------|-------------|
| `theme-default` | Default color schemes and theme switching |
| `status-bar` | Bottom status bar with segments |
| `git-integration` | Git branch/status display |
| `search` | In-terminal text search (GPU-accelerated) |
| `notifications` | Desktop notification support |

## Error Isolation

Extensions run with circuit breaker isolation:
- Activation has a 5-second timeout
- Errors in one extension do not affect others
- After repeated failures, the extension is disabled automatically
- Error details available via `marauder-ext info <name>`
