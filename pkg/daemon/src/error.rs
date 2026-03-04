//! Daemon error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("session not found: {0}")]
    SessionNotFound(u64),

    #[error("session already exists: {0}")]
    SessionExists(u64),

    #[error("daemon already running")]
    AlreadyRunning,

    #[error("daemon not running")]
    NotRunning,

    #[error("IPC error: {0}")]
    Ipc(#[from] marauder_ipc::IpcError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}
