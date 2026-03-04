//! IPC error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("frame too large: {size} bytes (max {max})")]
    FrameTooLarge { size: u32, max: u32 },

    #[error("connection closed")]
    ConnectionClosed,

    #[error("server not running")]
    ServerNotRunning,

    #[error("invalid socket path: {0}")]
    InvalidSocketPath(String),

    #[error("response ID mismatch: expected {expected}, got {got}")]
    ResponseIdMismatch { expected: u64, got: u64 },

    #[error("protocol violation: received unexpected request from server")]
    ProtocolViolation,
}
