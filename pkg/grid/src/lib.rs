pub mod cell;
pub mod ffi;
pub mod grid;
pub mod screen;
pub mod bindgen;
pub mod ops;
#[cfg(feature = "tauri-commands")]
pub mod commands;

pub use cell::{Cell, CellAttributes, Color};
pub use grid::{Grid, Cursor, SavedCursor, Selection};
pub use screen::{Screen, Row};

/// Re-export the canonical PaneId from event-bus.
pub use marauder_event_bus::PaneId;

/// Shared grid handle for concurrent access.
pub type SharedGrid = std::sync::Arc<std::sync::Mutex<Grid>>;

/// Map of pane_id → grid, used as Tauri managed state.
pub type PaneGridMap = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<PaneId, SharedGrid>>>;
