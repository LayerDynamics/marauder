//! Background process management for the Marauder multiplexer daemon.
//!
//! The daemon runs as a headless process, managing multiple terminal sessions
//! that clients can attach/detach from (like tmux/screen). It communicates
//! with clients via the IPC layer (`pkg/ipc`).
//!
//! Each session holds a live PTY, VT parser, and terminal grid.

pub mod error;
pub mod session;
pub mod daemon;
pub mod ffi;

pub use error::DaemonError;
pub use session::{Session, SessionId, SessionInfo};
pub use daemon::MarauderDaemon;
