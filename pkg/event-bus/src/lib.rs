pub mod events;
pub mod bus;
pub mod interceptor;
pub mod ffi;
pub mod bindgen;
pub mod handle_registry;
pub mod ops;
pub mod sync_util;

pub use events::{Event, EventType, EventError};
pub use bus::EventBus;
pub use handle_registry::HandleRegistry;
pub use interceptor::{Interceptor, InterceptorAction, InterceptorId};
pub use sync_util::{lock_or_log, read_or_log, write_or_log};

/// Unique identifier for a pane. Defined here so all crates share one definition.
pub type PaneId = u64;
