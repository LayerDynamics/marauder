//! WebGPU surface creation for browser-based rendering.
//!
//! When compiled for `wasm32-unknown-unknown`, this module provides
//! surface creation from an HTML `<canvas>` element via the WebGPU API,
//! replacing the native wgpu surface path used on desktop.
//!
//! The browser terminal connects to `marauder-server` via WebSocket for
//! PTY data, and runs the VT parser + grid + renderer locally in WASM.

/// Configuration for the browser terminal renderer.
#[derive(Debug, Clone)]
pub struct WebRendererConfig {
    /// CSS selector for the target canvas element.
    pub canvas_selector: String,
    /// Initial terminal dimensions (rows, cols).
    pub rows: u32,
    pub cols: u32,
    /// WebSocket URL for connecting to the daemon.
    pub ws_url: String,
    /// Font family for terminal text.
    pub font_family: String,
    /// Font size in pixels.
    pub font_size: f32,
}

impl Default for WebRendererConfig {
    fn default() -> Self {
        Self {
            canvas_selector: "#marauder-terminal".into(),
            rows: 24,
            cols: 80,
            ws_url: "ws://localhost:7680".into(),
            font_family: "monospace".into(),
            font_size: 14.0,
        }
    }
}

/// Create a wgpu surface from an HTML canvas element.
///
/// This function is only available when targeting `wasm32-unknown-unknown`.
/// It uses `web_sys` to get the canvas element and create a WebGPU surface.
///
/// # Example (from JavaScript via wasm-bindgen)
/// ```js
/// import init, { createWebTerminal } from './marauder_wasm.js';
/// await init();
/// const terminal = await createWebTerminal({
///   canvasSelector: '#terminal',
///   wsUrl: 'ws://localhost:7680',
///   rows: 24,
///   cols: 80,
/// });
/// ```
#[cfg(target_arch = "wasm32")]
pub async fn create_web_surface(
    instance: &wgpu::Instance,
    canvas_selector: &str,
) -> Result<(wgpu::Surface<'static>, u32, u32), String> {
    use wasm_bindgen::JsCast;
    use web_sys::{window, HtmlCanvasElement};

    let window = window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let canvas = document
        .query_selector(canvas_selector)
        .map_err(|_| "querySelector failed")?
        .ok_or("canvas element not found")?;
    let canvas: HtmlCanvasElement = canvas
        .dyn_into()
        .map_err(|_| "element is not a canvas")?;

    let width = canvas.width();
    let height = canvas.height();

    let surface_target = wgpu::SurfaceTarget::Canvas(canvas);
    let surface = instance
        .create_surface(surface_target)
        .map_err(|e| format!("failed to create surface: {e}"))?;

    Ok((surface, width, height))
}

/// Stub for non-WASM targets — this module is only meaningful on wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub fn create_web_surface_stub() {
    // This function exists only so the module compiles on native targets.
    // The actual WebGPU surface creation happens only in WASM builds.
}

/// Web component definition for `<marauder-terminal>`.
///
/// This is a conceptual specification — the actual web component will be
/// implemented in JavaScript/TypeScript and bundled with the WASM module.
///
/// Usage:
/// ```html
/// <marauder-terminal
///   ws-url="ws://localhost:7680"
///   rows="24"
///   cols="80"
///   font-size="14"
///   font-family="JetBrains Mono"
///   theme="catppuccin-mocha"
/// ></marauder-terminal>
/// ```
///
/// The web component:
/// 1. Creates a shadow DOM with a `<canvas>` element
/// 2. Loads the WASM module and initializes the renderer on the canvas
/// 3. Connects to the daemon via WebSocket at `ws-url`
/// 4. Handles keyboard/mouse input and forwards to the PTY via WebSocket
/// 5. Renders terminal output via WebGPU (or falls back to Canvas 2D)
#[derive(Debug, Clone)]
pub struct MarauderTerminalElement {
    pub ws_url: String,
    pub rows: u32,
    pub cols: u32,
    pub font_size: f32,
    pub font_family: String,
    pub theme: String,
}

impl Default for MarauderTerminalElement {
    fn default() -> Self {
        Self {
            ws_url: "ws://localhost:7680".into(),
            rows: 24,
            cols: 80,
            font_size: 14.0,
            font_family: "JetBrains Mono, monospace".into(),
            theme: "catppuccin-mocha".into(),
        }
    }
}
