// extensions/theme-default/mod.ts
// Catppuccin theme palettes for Marauder.

interface ExtensionConfig {
  get<T>(key: string): T | undefined;
  set(key: string, value: unknown): void;
}

interface ExtensionEvents {
  on(type: string, handler: (payload: unknown) => void): () => void;
  emit(type: string, payload: unknown): void;
}

interface ExtensionStatusBar {
  set(segment: "left" | "center" | "right", text: string): void;
}

interface ExtensionNotifications {
  show(title: string, body?: string): void;
}

interface ExtensionCommands {
  register(id: string, handler: () => void): void;
}

interface ExtensionKeybindings {
  register(keys: string, commandId: string): void;
}

interface ExtensionContext {
  config: ExtensionConfig;
  events: ExtensionEvents;
  statusBar: ExtensionStatusBar;
  notifications: ExtensionNotifications;
  commands: ExtensionCommands;
  keybindings: ExtensionKeybindings;
}

/** RGBA tuple: [r, g, b, a] */
type Rgba = [number, number, number, number];

/** RGB tuple for ANSI palette entries */
type Rgb = [number, number, number];

interface ThemeConfig {
  name: string;
  bg: Rgba;
  fg: Rgba;
  cursor: Rgba;
  selection: Rgba;
  /** 16-color ANSI palette (indices 0–15) */
  ansi: [Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb, Rgb];
}

// ---------------------------------------------------------------------------
// Catppuccin Mocha
// ---------------------------------------------------------------------------
const mochaPalette: ThemeConfig = {
  name: "Catppuccin Mocha",
  bg: [30, 30, 46, 255],
  fg: [205, 214, 244, 255],
  cursor: [137, 180, 250, 255],
  selection: [88, 91, 112, 128],
  ansi: [
    [69, 71, 90],    // Black
    [243, 139, 168], // Red
    [166, 227, 161], // Green
    [249, 226, 175], // Yellow
    [137, 180, 250], // Blue
    [245, 194, 231], // Magenta
    [148, 226, 213], // Cyan
    [186, 194, 222], // White
    [88, 91, 112],   // Bright Black
    [243, 139, 168], // Bright Red
    [166, 227, 161], // Bright Green
    [249, 226, 175], // Bright Yellow
    [137, 180, 250], // Bright Blue
    [245, 194, 231], // Bright Magenta
    [148, 226, 213], // Bright Cyan
    [205, 214, 244], // Bright White
  ],
};

// ---------------------------------------------------------------------------
// Catppuccin Latte (light)
// ---------------------------------------------------------------------------
const lattePalette: ThemeConfig = {
  name: "Catppuccin Latte",
  bg: [239, 241, 245, 255],
  fg: [76, 79, 105, 255],
  cursor: [30, 102, 245, 255],
  selection: [172, 176, 190, 128],
  ansi: [
    [108, 111, 133], // Black
    [210, 15, 57],   // Red
    [64, 160, 43],   // Green
    [223, 142, 29],  // Yellow
    [30, 102, 245],  // Blue
    [234, 118, 203], // Magenta
    [23, 146, 153],  // Cyan
    [172, 176, 190], // White
    [100, 104, 128], // Bright Black
    [210, 15, 57],   // Bright Red
    [64, 160, 43],   // Bright Green
    [223, 142, 29],  // Bright Yellow
    [30, 102, 245],  // Bright Blue
    [234, 118, 203], // Bright Magenta
    [23, 146, 153],  // Bright Cyan
    [76, 79, 105],   // Bright White
  ],
};

// ---------------------------------------------------------------------------
// Catppuccin Frappe
// ---------------------------------------------------------------------------
const frappePalette: ThemeConfig = {
  name: "Catppuccin Frappe",
  bg: [48, 52, 70, 255],
  fg: [198, 208, 245, 255],
  cursor: [140, 170, 238, 255],
  selection: [81, 87, 109, 128],
  ansi: [
    [81, 87, 109],   // Black
    [231, 130, 132], // Red
    [166, 209, 137], // Green
    [229, 200, 144], // Yellow
    [140, 170, 238], // Blue
    [244, 184, 228], // Magenta
    [129, 200, 190], // Cyan
    [176, 187, 229], // White
    [99, 104, 128],  // Bright Black
    [231, 130, 132], // Bright Red
    [166, 209, 137], // Bright Green
    [229, 200, 144], // Bright Yellow
    [140, 170, 238], // Bright Blue
    [244, 184, 228], // Bright Magenta
    [129, 200, 190], // Bright Cyan
    [198, 208, 245], // Bright White
  ],
};

// ---------------------------------------------------------------------------
// Catppuccin Macchiato
// ---------------------------------------------------------------------------
const macchiatoPalette: ThemeConfig = {
  name: "Catppuccin Macchiato",
  bg: [36, 39, 58, 255],
  fg: [202, 211, 245, 255],
  cursor: [138, 173, 244, 255],
  selection: [84, 89, 112, 128],
  ansi: [
    [84, 89, 112],   // Black
    [237, 135, 150], // Red
    [166, 218, 149], // Green
    [238, 212, 159], // Yellow
    [138, 173, 244], // Blue
    [245, 189, 230], // Magenta
    [139, 213, 202], // Cyan
    [184, 192, 224], // White
    [101, 107, 131], // Bright Black
    [237, 135, 150], // Bright Red
    [166, 218, 149], // Bright Green
    [238, 212, 159], // Bright Yellow
    [138, 173, 244], // Bright Blue
    [245, 189, 230], // Bright Magenta
    [139, 213, 202], // Bright Cyan
    [202, 211, 245], // Bright White
  ],
};

const ALL_THEMES: ThemeConfig[] = [
  mochaPalette,
  lattePalette,
  frappePalette,
  macchiatoPalette,
];

let _deactivateCleanup: (() => void) | null = null;

export function activate(ctx: ExtensionContext): void {
  // Register all palettes so the config layer knows about them.
  ctx.config.set("themes.available", ALL_THEMES.map((t) => t.name));
  ctx.config.set("themes.catppuccin-mocha", mochaPalette);
  ctx.config.set("themes.catppuccin-latte", lattePalette);
  ctx.config.set("themes.catppuccin-frappe", frappePalette);
  ctx.config.set("themes.catppuccin-macchiato", macchiatoPalette);

  // Apply Mocha as the default only when no theme is already configured.
  const existing = ctx.config.get<ThemeConfig>("theme");
  if (existing === undefined) {
    ctx.config.set("theme", mochaPalette);
  }

  // Emit so any already-running renderer picks up the initial theme.
  ctx.events.emit("ExtensionMessage", {
    source: "theme-default",
    type: "ThemeApplied",
    payload: ctx.config.get<ThemeConfig>("theme") ?? mochaPalette,
  });

  // Listen for runtime requests to switch themes.
  const unsub = ctx.events.on("ExtensionMessage", (raw: unknown) => {
    const msg = raw as {
      source?: string;
      type?: string;
      payload?: { name?: string };
    };
    if (msg.source !== "theme-default" && msg.type === "SetTheme") {
      const requested = ALL_THEMES.find((t) => t.name === msg.payload?.name);
      if (requested !== undefined) {
        ctx.config.set("theme", requested);
        ctx.events.emit("ExtensionMessage", {
          source: "theme-default",
          type: "ThemeApplied",
          payload: requested,
        });
      }
    }
  });

  _deactivateCleanup = unsub;
}

export function deactivate(): void {
  if (_deactivateCleanup !== null) {
    _deactivateCleanup();
    _deactivateCleanup = null;
  }
}
