// extensions/theme-default/mod.ts
// Catppuccin theme palettes for Marauder.

import type { ExtensionContext } from "@marauder/extensions";

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

// ---------------------------------------------------------------------------
// Dracula
// ---------------------------------------------------------------------------
const draculaPalette: ThemeConfig = {
  name: "Dracula",
  bg: [40, 42, 54, 255],
  fg: [248, 248, 242, 255],
  cursor: [248, 248, 242, 255],
  selection: [68, 71, 90, 128],
  ansi: [
    [33, 34, 44],    // Black
    [255, 85, 85],   // Red
    [80, 250, 123],  // Green
    [241, 250, 140], // Yellow
    [189, 147, 249], // Blue
    [255, 121, 198], // Magenta
    [139, 233, 253], // Cyan
    [248, 248, 242], // White
    [98, 114, 164],  // Bright Black
    [255, 110, 110], // Bright Red
    [105, 255, 148], // Bright Green
    [255, 255, 165], // Bright Yellow
    [214, 172, 255], // Bright Blue
    [255, 146, 223], // Bright Magenta
    [164, 255, 255], // Bright Cyan
    [255, 255, 255], // Bright White
  ],
};

// ---------------------------------------------------------------------------
// Solarized Dark
// ---------------------------------------------------------------------------
const solarizedDarkPalette: ThemeConfig = {
  name: "Solarized Dark",
  bg: [0, 43, 54, 255],
  fg: [131, 148, 150, 255],
  cursor: [131, 148, 150, 255],
  selection: [7, 54, 66, 128],
  ansi: [
    [7, 54, 66],     // Black (base02)
    [220, 50, 47],   // Red
    [133, 153, 0],   // Green
    [181, 137, 0],   // Yellow
    [38, 139, 210],  // Blue
    [211, 54, 130],  // Magenta
    [42, 161, 152],  // Cyan
    [238, 232, 213], // White (base2)
    [0, 43, 54],     // Bright Black (base03)
    [203, 75, 22],   // Bright Red (orange)
    [88, 110, 117],  // Bright Green (base01)
    [101, 123, 131], // Bright Yellow (base00)
    [131, 148, 150], // Bright Blue (base0)
    [108, 113, 196], // Bright Magenta (violet)
    [147, 161, 161], // Bright Cyan (base1)
    [253, 246, 227], // Bright White (base3)
  ],
};

// ---------------------------------------------------------------------------
// Solarized Light
// ---------------------------------------------------------------------------
const solarizedLightPalette: ThemeConfig = {
  name: "Solarized Light",
  bg: [253, 246, 227, 255],
  fg: [101, 123, 131, 255],
  cursor: [101, 123, 131, 255],
  selection: [238, 232, 213, 128],
  ansi: [
    [7, 54, 66],     // Black (base02)
    [220, 50, 47],   // Red
    [133, 153, 0],   // Green
    [181, 137, 0],   // Yellow
    [38, 139, 210],  // Blue
    [211, 54, 130],  // Magenta
    [42, 161, 152],  // Cyan
    [238, 232, 213], // White (base2)
    [0, 43, 54],     // Bright Black (base03)
    [203, 75, 22],   // Bright Red (orange)
    [88, 110, 117],  // Bright Green (base01)
    [101, 123, 131], // Bright Yellow (base00)
    [131, 148, 150], // Bright Blue (base0)
    [108, 113, 196], // Bright Magenta (violet)
    [147, 161, 161], // Bright Cyan (base1)
    [253, 246, 227], // Bright White (base3)
  ],
};

// ---------------------------------------------------------------------------
// Nord
// ---------------------------------------------------------------------------
const nordPalette: ThemeConfig = {
  name: "Nord",
  bg: [46, 52, 64, 255],
  fg: [216, 222, 233, 255],
  cursor: [216, 222, 233, 255],
  selection: [67, 76, 94, 128],
  ansi: [
    [59, 66, 82],    // Black (nord1)
    [191, 97, 106],  // Red (nord11)
    [163, 190, 140], // Green (nord14)
    [235, 203, 139], // Yellow (nord13)
    [129, 161, 193], // Blue (nord9)
    [180, 142, 173], // Magenta (nord15)
    [136, 192, 208], // Cyan (nord8)
    [229, 233, 240], // White (nord5)
    [76, 86, 106],   // Bright Black (nord3)
    [191, 97, 106],  // Bright Red
    [163, 190, 140], // Bright Green
    [235, 203, 139], // Bright Yellow
    [129, 161, 193], // Bright Blue
    [180, 142, 173], // Bright Magenta
    [143, 188, 187], // Bright Cyan (nord7)
    [236, 239, 244], // Bright White (nord6)
  ],
};

const ALL_THEMES: ThemeConfig[] = [
  mochaPalette,
  lattePalette,
  frappePalette,
  macchiatoPalette,
  draculaPalette,
  solarizedDarkPalette,
  solarizedLightPalette,
  nordPalette,
];

let _deactivateCleanup: (() => void) | null = null;

export function activate(ctx: ExtensionContext): void {
  // Clean up any previous activation to prevent leaked subscriptions.
  if (_deactivateCleanup !== null) {
    deactivate();
  }

  // Register all palettes so the config layer knows about them.
  ctx.config.set("themes.available", ALL_THEMES.map((t) => t.name));
  ctx.config.set("themes.catppuccin-mocha", mochaPalette);
  ctx.config.set("themes.catppuccin-latte", lattePalette);
  ctx.config.set("themes.catppuccin-frappe", frappePalette);
  ctx.config.set("themes.catppuccin-macchiato", macchiatoPalette);
  ctx.config.set("themes.dracula", draculaPalette);
  ctx.config.set("themes.solarized-dark", solarizedDarkPalette);
  ctx.config.set("themes.solarized-light", solarizedLightPalette);
  ctx.config.set("themes.nord", nordPalette);

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
