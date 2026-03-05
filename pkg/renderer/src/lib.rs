//! GPU-accelerated terminal renderer (wgpu + cosmic-text).
//!
//! Architecture:
//! - `atlas`: Glyph rasterization and GPU texture atlas management
//! - `pipelines`: wgpu render pipelines (background, text, cursor)
//! - `renderer`: Main `Renderer` struct coordinating frame production
//! - `types`: Shared GPU data types (instance structs, uniforms)
//!
//! The renderer reads the terminal grid (from `marauder-grid`) and produces
//! frames via instanced rendering. Each cell becomes one background instance
//! and optionally one text instance. The cursor is drawn as a blended overlay.

pub mod atlas;
pub mod ffi;
pub mod pipelines;
pub mod renderer;
pub mod types;

pub use renderer::Renderer;
pub use types::{CursorStyle, RendererConfig, ThemeColors};
