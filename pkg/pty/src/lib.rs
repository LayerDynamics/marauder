pub mod pty;
pub mod manager;
pub mod reader;
pub mod ffi;
pub mod ops;
#[cfg(feature = "tauri-commands")]
pub mod commands;
pub mod bindgen;

pub use manager::{PtyManager, PtyConfig, PtySession, PaneId};
pub use pty::{open_pty, resize_master, default_shell, default_config, OpenPtyResult};
pub use reader::PtyReader;
#[cfg(feature = "tauri-commands")]
pub use commands::TauriPtyManager;
