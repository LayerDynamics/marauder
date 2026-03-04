//! Daemon session tracking.
//!
//! Each session represents a terminal session with its own PTY, parser,
//! grid, and pipeline — managed by a `MarauderRuntime` instance.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Unique session identifier.
pub type SessionId = u64;

/// Global session ID counter.
///
/// NOTE: This is a global atomic counter that is never reset. Tests should only
/// assert that IDs are unique and positive (`s1.id != s2.id`, `id > 0`), never
/// specific values, because ordering depends on test execution order.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate the next session ID (always > 0).
pub fn next_session_id() -> SessionId {
    NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
}

/// Session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session is active with a running PTY.
    Active,
    /// Session has no attached clients but PTY is still running.
    Detached,
    /// Session's PTY has exited.
    Dead,
}

/// Serializable session info for client queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    pub state: SessionState,
    pub shell: String,
    pub rows: u16,
    pub cols: u16,
    pub created_at_unix_secs: u64,
    pub attached_clients: u32,
}

/// A daemon-managed terminal session.
pub struct Session {
    pub id: SessionId,
    pub state: SessionState,
    pub shell: String,
    pub rows: u16,
    pub cols: u16,
    pub created_at: SystemTime,
    pub attached_clients: u32,
}

impl Session {
    /// Create a new session.
    pub fn new(shell: String, rows: u16, cols: u16) -> Self {
        Self {
            id: next_session_id(),
            state: SessionState::Active,
            shell,
            rows,
            cols,
            created_at: SystemTime::now(),
            attached_clients: 0,
        }
    }

    /// Convert to serializable info.
    pub fn info(&self) -> SessionInfo {
        let created_at_unix_secs = self
            .created_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        SessionInfo {
            id: self.id,
            state: self.state,
            shell: self.shell.clone(),
            rows: self.rows,
            cols: self.cols,
            created_at_unix_secs,
            attached_clients: self.attached_clients,
        }
    }

    /// Attach a client.
    pub fn attach(&mut self) {
        self.attached_clients += 1;
        if self.state == SessionState::Detached {
            self.state = SessionState::Active;
        }
    }

    /// Detach a client.
    pub fn detach(&mut self) {
        self.attached_clients = self.attached_clients.saturating_sub(1);
        if self.attached_clients == 0 && self.state == SessionState::Active {
            self.state = SessionState::Detached;
        }
    }

    /// Mark as dead (PTY exited).
    pub fn mark_dead(&mut self) {
        self.state = SessionState::Dead;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_ids_are_unique() {
        let s1 = Session::new("/bin/sh".into(), 24, 80);
        let s2 = Session::new("/bin/sh".into(), 24, 80);
        assert_ne!(s1.id, s2.id);
        assert!(s1.id > 0);
        assert!(s2.id > 0);
    }

    #[test]
    fn test_attach_detach() {
        let mut s = Session::new("/bin/sh".into(), 24, 80);
        assert_eq!(s.state, SessionState::Active);
        assert_eq!(s.attached_clients, 0);

        s.attach();
        assert_eq!(s.attached_clients, 1);
        assert_eq!(s.state, SessionState::Active);

        s.detach();
        assert_eq!(s.attached_clients, 0);
        assert_eq!(s.state, SessionState::Detached);

        s.attach();
        assert_eq!(s.state, SessionState::Active);
    }

    #[test]
    fn test_mark_dead() {
        let mut s = Session::new("/bin/sh".into(), 24, 80);
        s.mark_dead();
        assert_eq!(s.state, SessionState::Dead);
    }

    #[test]
    fn test_session_info() {
        let s = Session::new("/bin/zsh".into(), 48, 120);
        let info = s.info();
        assert_eq!(info.id, s.id);
        assert_eq!(info.shell, "/bin/zsh");
        assert_eq!(info.rows, 48);
        assert_eq!(info.cols, 120);
        // created_at_unix_secs should be a reasonable timestamp (after year 2020)
        assert!(info.created_at_unix_secs > 1_577_836_800);
    }

    #[test]
    fn test_detach_saturates() {
        let mut s = Session::new("/bin/sh".into(), 24, 80);
        s.detach(); // already 0
        assert_eq!(s.attached_clients, 0);
    }
}
