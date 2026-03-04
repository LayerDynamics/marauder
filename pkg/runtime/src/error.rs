use thiserror::Error;

/// Runtime errors.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("runtime already booted")]
    AlreadyBooted,

    #[error("runtime not booted")]
    NotBooted,

    #[error("runtime is shutting down")]
    ShuttingDown,

    #[error("PTY error: {0}")]
    Pty(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("pipeline error: {0}")]
    Pipeline(String),

    #[error("lifecycle hook error: {0}")]
    Hook(String),

    #[error("pane not found: {0}")]
    PaneNotFound(u64),

    #[error("invalid shell path: {0}")]
    InvalidShell(String),
}

impl RuntimeError {
    /// Convert an anyhow::Error from PTY operations into RuntimeError::Pty.
    pub fn pty(err: anyhow::Error) -> Self {
        Self::Pty(err.to_string())
    }
}
