use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;

use portable_pty::{Child, MasterPty};
use marauder_event_bus::bus::SharedEventBus;
use marauder_event_bus::events::{Event, EventType};

use crate::pty;

/// Re-export the canonical PaneId from event-bus.
pub use marauder_event_bus::PaneId;

/// Configuration for spawning a PTY.
#[derive(Debug, Clone)]
pub struct PtyConfig {
    pub shell: String,
    pub env: HashMap<String, String>,
    pub cwd: PathBuf,
    pub rows: u16,
    pub cols: u16,
}

/// An active PTY session holding the master, reader, writer, and child process.
///
/// The `reader` field is `Option` because it may be taken by `PtyReader::spawn()` for
/// async reading. Once taken, `PtyManager::read()` will return an error for that session.
/// The two read modes are mutually exclusive: synchronous (`PtyManager::read`) or
/// async (`PtyReader::spawn`).
pub struct PtySession {
    pub(crate) master: Box<dyn MasterPty + Send>,
    pub(crate) reader: Option<Box<dyn Read + Send>>,
    pub(crate) writer: Box<dyn Write + Send>,
    pub(crate) child: Box<dyn Child + Send + Sync>,
    pub(crate) config: PtyConfig,
}

/// Manages multiple PTY sessions, one per pane.
pub struct PtyManager {
    sessions: HashMap<PaneId, PtySession>,
    next_id: PaneId,
    event_bus: Option<SharedEventBus>,
}

impl PtyManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_id: 1,
            event_bus: None,
        }
    }

    pub fn with_event_bus(mut self, bus: SharedEventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Create a new PTY session with the given config. Returns the assigned PaneId.
    pub fn create(&mut self, config: PtyConfig) -> anyhow::Result<PaneId> {
        let result = pty::open_pty(&config)?;
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("PTY pane ID overflow"))?;

        self.sessions.insert(id, PtySession {
            master: result.master,
            reader: Some(result.reader),
            writer: result.writer,
            child: result.child,
            config,
        });

        if let Some(bus) = &self.event_bus {
            bus.publish(Event::new(EventType::PaneCreated, id));
        }

        tracing::info!(pane_id = id, "PTY session created");
        Ok(id)
    }

    /// Get a reference to a PTY session.
    pub fn get(&self, id: PaneId) -> Option<&PtySession> {
        self.sessions.get(&id)
    }

    /// Get a mutable reference to a PTY session.
    pub fn get_mut(&mut self, id: PaneId) -> Option<&mut PtySession> {
        self.sessions.get_mut(&id)
    }

    /// Close and remove a PTY session.
    pub fn close(&mut self, id: PaneId) -> anyhow::Result<()> {
        if let Some(mut session) = self.sessions.remove(&id) {
            // Kill the child process if still running
            session.child.kill()?;

            if let Some(bus) = &self.event_bus {
                bus.publish(Event::new(EventType::PaneClosed, id));
            }

            tracing::info!(pane_id = id, "PTY session closed");
            Ok(())
        } else {
            anyhow::bail!("No PTY session with id {id}")
        }
    }

    /// Close all sessions, killing child processes.
    pub fn close_all(&mut self) {
        let ids: Vec<PaneId> = self.sessions.keys().copied().collect();
        for id in ids {
            if let Err(e) = self.close(id) {
                tracing::warn!(pane_id = id, error = %e, "Failed to close PTY session during cleanup");
            }
        }
    }

    /// Resize a PTY session.
    ///
    /// Delegates to `portable_pty::MasterPty::resize()`, which internally issues
    /// an `ioctl(TIOCSWINSZ)` call. The kernel automatically delivers SIGWINCH to
    /// the child process group when the window size changes, so no manual signal
    /// forwarding is needed.
    pub fn resize(&mut self, id: PaneId, rows: u16, cols: u16) -> anyhow::Result<()> {
        let session = self.sessions.get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("No PTY session with id {id}"))?;

        pty::resize_master(session.master.as_ref(), rows, cols)?;
        session.config.rows = rows;
        session.config.cols = cols;

        tracing::debug!(pane_id = id, rows, cols, "PTY resized");
        Ok(())
    }

    /// Write data to a PTY session's master writer.
    pub fn write(&mut self, id: PaneId, data: &[u8]) -> anyhow::Result<usize> {
        let session = self.sessions.get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("No PTY session with id {id}"))?;

        session.writer.write_all(data)?;
        Ok(data.len())
    }

    /// Read available data from a PTY session. Non-blocking best-effort read.
    ///
    /// Returns an error if the reader has been taken by `take_reader()` for async mode.
    pub fn read(&mut self, id: PaneId, buf: &mut [u8]) -> anyhow::Result<usize> {
        let session = self.sessions.get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("No PTY session with id {id}"))?;

        let reader = session.reader.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Reader for pane {id} has been taken for async mode"))?;

        let n = reader.read(buf)?;
        Ok(n)
    }

    /// Take ownership of the reader stream for async reading (e.g., `PtyReader::spawn`).
    /// After this, `read()` will return an error for this session.
    pub fn take_reader(&mut self, id: PaneId) -> anyhow::Result<Box<dyn std::io::Read + Send>> {
        let session = self.sessions.get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("No PTY session with id {id}"))?;

        session.reader.take()
            .ok_or_else(|| anyhow::anyhow!("Reader for pane {id} already taken"))
    }

    /// Get the child process ID for a session.
    pub fn get_pid(&self, id: PaneId) -> anyhow::Result<Option<u32>> {
        let session = self.sessions.get(&id)
            .ok_or_else(|| anyhow::anyhow!("No PTY session with id {id}"))?;

        Ok(session.child.process_id())
    }

    /// List all active pane IDs.
    pub fn list(&self) -> Vec<PaneId> {
        self.sessions.keys().copied().collect()
    }

    /// Number of active sessions.
    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    /// Check if a child process has exited. Returns exit status if so.
    pub fn try_wait(&mut self, id: PaneId) -> anyhow::Result<Option<portable_pty::ExitStatus>> {
        let session = self.sessions.get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("No PTY session with id {id}"))?;

        let status = session.child.try_wait()?;
        if let Some(ref status) = status {
            if let Some(bus) = &self.event_bus {
                bus.publish(Event::new(EventType::PtyExit, format!("pane:{id} status:{status:?}")));
            }
        }
        Ok(status)
    }

    /// Get the session config for a pane.
    pub fn get_config(&self, id: PaneId) -> Option<&PtyConfig> {
        self.sessions.get(&id).map(|s| &s.config)
    }
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        self.close_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> PtyConfig {
        PtyConfig {
            shell: crate::pty::default_shell(),
            env: HashMap::new(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            rows: 24,
            cols: 80,
        }
    }

    #[test]
    fn test_create_and_close() {
        let mut mgr = PtyManager::new();
        let id = mgr.create(test_config()).expect("create should succeed");
        assert_eq!(id, 1);
        assert_eq!(mgr.count(), 1);
        assert!(mgr.get(id).is_some());

        mgr.close(id).expect("close should succeed");
        assert_eq!(mgr.count(), 0);
        assert!(mgr.get(id).is_none());
    }

    #[test]
    fn test_multiple_sessions() {
        let mut mgr = PtyManager::new();
        let id1 = mgr.create(test_config()).unwrap();
        let id2 = mgr.create(test_config()).unwrap();
        assert_ne!(id1, id2);
        assert_eq!(mgr.count(), 2);

        let ids = mgr.list();
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));

        mgr.close(id1).unwrap();
        mgr.close(id2).unwrap();
    }

    #[test]
    fn test_close_nonexistent() {
        let mut mgr = PtyManager::new();
        assert!(mgr.close(999).is_err());
    }

    #[test]
    fn test_resize() {
        let mut mgr = PtyManager::new();
        let id = mgr.create(test_config()).unwrap();
        mgr.resize(id, 48, 120).expect("resize should succeed");

        let config = mgr.get_config(id).unwrap();
        assert_eq!(config.rows, 48);
        assert_eq!(config.cols, 120);

        mgr.close(id).unwrap();
    }

    #[test]
    fn test_invalid_rows_cols() {
        let mut mgr = PtyManager::new();
        let mut config = test_config();
        config.rows = 0;
        assert!(mgr.create(config).is_err());
    }

    #[test]
    fn test_get_pid() {
        let mut mgr = PtyManager::new();
        let id = mgr.create(test_config()).unwrap();
        let pid = mgr.get_pid(id).unwrap();
        assert!(pid.is_some());
        assert!(pid.unwrap() > 0);
        mgr.close(id).unwrap();
    }
}
