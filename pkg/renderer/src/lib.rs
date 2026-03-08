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
pub mod images;
pub mod pipelines;
pub mod renderer;
pub mod types;
pub mod web;

pub use renderer::{PaneBorder, Renderer};
pub use types::{CursorStyle, RendererConfig, ThemeColors};

/// Errors from renderer operations.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("surface error: {0}")]
    Surface(#[from] wgpu::SurfaceError),
    #[error("no suitable GPU adapter found")]
    NoAdapter,
    #[error("failed to request GPU device: {0}")]
    DeviceRequest(String),
    #[error("shader compilation error: {0}")]
    ShaderCompilation(String),
    #[error("pipeline creation error: {0}")]
    PipelineCreation(String),
    #[error("surface creation error: {0}")]
    SurfaceCreation(String),
}
