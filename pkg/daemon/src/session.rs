//! Daemon session tracking.
//!
//! Each session represents a terminal session with its own PTY, parser,
//! grid, and pipeline — managed by the daemon.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use marauder_grid::Grid;
use marauder_parser::MarauderParser;
use marauder_pty::{PtyConfig, PtyManager, PaneId};

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

/// A daemon-managed terminal session with live PTY, parser, and grid.
pub struct Session {
    pub id: SessionId,
    pub state: SessionState,
    pub shell: String,
    pub rows: u16,
    pub cols: u16,
    pub created_at: SystemTime,
    pub attached_clients: u32,
    /// PTY manager holding the live PTY process for this session.
    pub pty_manager: PtyManager,
    /// The PaneId within the PtyManager for this session's PTY.
    pub pane_id: PaneId,
    /// VT parser for converting PTY output bytes to terminal actions.
    pub parser: MarauderParser,
    /// Terminal cell grid (primary + alternate screens).
    pub grid: Grid,
}

impl Session {
    /// Create a new session, spawning a real PTY process.
    ///
    /// Returns an error if the PTY cannot be spawned (e.g., invalid shell path).
    pub fn create(shell: String, rows: u16, cols: u16) -> anyhow::Result<Self> {
        let mut pty_manager = PtyManager::new();
        let config = PtyConfig {
            shell: shell.clone(),
            env: HashMap::new(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            rows,
            cols,
        };
        let pane_id = pty_manager.create(config)?;
        let parser = MarauderParser::new();
        let grid = Grid::new(rows as usize, cols as usize);

        Ok(Self {
            id: next_session_id(),
            state: SessionState::Active,
            shell,
            rows,
            cols,
            created_at: SystemTime::now(),
            attached_clients: 0,
            pty_manager,
            pane_id,
            parser,
            grid,
        })
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

    /// Write input data to the PTY (from client keyboard input).
    pub fn write_to_pty(&mut self, data: &[u8]) -> anyhow::Result<usize> {
        self.pty_manager.write(self.pane_id, data)
    }

    /// Read available data from the PTY, feed through parser, and apply to grid.
    /// Returns the raw bytes read (for forwarding to the client).
    pub fn read_and_process(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
        let mut buf = vec![0u8; 8192];
        match self.pty_manager.read(self.pane_id, &mut buf) {
            Ok(0) => Ok(None),
            Ok(n) => {
                let data = buf[..n].to_vec();
                // Feed through parser → grid
                let grid = &mut self.grid;
                self.parser.feed(&data, |action| {
                    grid.apply_action(&action);
                });
                Ok(Some(data))
            }
            Err(e) => Err(e),
        }
    }

    /// Resize the PTY and grid.
    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.pty_manager.resize(self.pane_id, rows, cols)?;
        self.grid.resize(rows as usize, cols as usize);
        self.rows = rows;
        self.cols = cols;
        Ok(())
    }

    /// Check if the PTY child process has exited.
    pub fn check_alive(&mut self) -> bool {
        match self.pty_manager.try_wait(self.pane_id) {
            Ok(Some(_status)) => {
                self.mark_dead();
                false
            }
            Ok(None) => true, // still running
            Err(_) => {
                self.mark_dead();
                false
            }
        }
    }

    /// Kill the PTY process and clean up.
    pub fn kill(&mut self) {
        let _ = self.pty_manager.close(self.pane_id);
        self.mark_dead();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_ids_are_unique() {
        let s1 = Session::create("/bin/sh".into(), 24, 80).unwrap();
        let s2 = Session::create("/bin/sh".into(), 24, 80).unwrap();
        assert_ne!(s1.id, s2.id);
        assert!(s1.id > 0);
        assert!(s2.id > 0);
    }

    #[test]
    fn test_attach_detach() {
        let mut s = Session::create("/bin/sh".into(), 24, 80).unwrap();
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
        let mut s = Session::create("/bin/sh".into(), 24, 80).unwrap();
        s.mark_dead();
        assert_eq!(s.state, SessionState::Dead);
    }

    #[test]
    fn test_session_info() {
        let s = Session::create("/bin/sh".into(), 48, 120).unwrap();
        let info = s.info();
        assert_eq!(info.id, s.id);
        assert_eq!(info.shell, "/bin/sh");
        assert_eq!(info.rows, 48);
        assert_eq!(info.cols, 120);
        assert!(info.created_at_unix_secs > 1_577_836_800);
    }

    #[test]
    fn test_detach_saturates() {
        let mut s = Session::create("/bin/sh".into(), 24, 80).unwrap();
        s.detach(); // already 0
        assert_eq!(s.attached_clients, 0);
    }

    #[test]
    fn test_write_to_pty() {
        let mut s = Session::create("/bin/sh".into(), 24, 80).unwrap();
        let result = s.write_to_pty(b"echo hello\n");
        assert!(result.is_ok());
    }

    #[test]
    fn test_resize() {
        let mut s = Session::create("/bin/sh".into(), 24, 80).unwrap();
        s.resize(48, 120).expect("resize should succeed");
        assert_eq!(s.rows, 48);
        assert_eq!(s.cols, 120);
    }

    #[test]
    fn test_check_alive() {
        let mut s = Session::create("/bin/sh".into(), 24, 80).unwrap();
        assert!(s.check_alive());
    }

    #[test]
    fn test_kill() {
        let mut s = Session::create("/bin/sh".into(), 24, 80).unwrap();
        s.kill();
        assert_eq!(s.state, SessionState::Dead);
    }
}
