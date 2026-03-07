//! The Marauder daemon — headless multiplexer process.
//!
//! Manages sessions and handles IPC requests from clients.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use marauder_event_bus::lock_or_log;
use marauder_ipc::message::{IpcMessage, IpcRequest};
use marauder_ipc::server::{IpcServer, RequestHandler};
use tokio::sync::broadcast;

use crate::error::DaemonError;
use crate::session::{Session, SessionId};

/// Default maximum number of concurrent sessions.
const DEFAULT_MAX_SESSIONS: usize = 256;

/// Default socket path.
///
/// Prefers `XDG_RUNTIME_DIR` (typically `0700`), then `$HOME/.marauder/`,
/// and falls back to a per-user directory under `/tmp`. Callers must use
/// [`ensure_socket_dir_secure`] before binding to enforce restrictive permissions.
pub fn default_socket_path() -> PathBuf {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("marauder/daemon.sock");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".marauder/daemon.sock");
    }
    // Use the username from USER env var or PID as fallback
    let user = std::env::var("USER").unwrap_or_else(|_| format!("pid-{}", std::process::id()));
    PathBuf::from(format!("/tmp/marauder-{}/daemon.sock", user))
}

/// Ensure the parent directory of the socket path exists with restrictive
/// permissions (`0700`) suitable for a control socket.
#[cfg(unix)]
pub fn ensure_socket_dir_secure(socket_path: &Path) -> Result<(), DaemonError> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = socket_path.parent() {
        fs::create_dir_all(dir)?;
        let meta = fs::metadata(dir)?;
        // Verify the directory is owned by the current user
        let my_uid = unsafe { libc::getuid() };
        if meta.uid() != my_uid {
            return Err(DaemonError::Other(format!(
                "socket directory {} is owned by uid {} but current uid is {}",
                dir.display(),
                meta.uid(),
                my_uid,
            )));
        }
        let mut perms = meta.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(dir, perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn ensure_socket_dir_secure(socket_path: &Path) -> Result<(), DaemonError> {
    if let Some(dir) = socket_path.parent() {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// Validate that a shell path is absolute and exists on disk.
fn validate_shell_path(shell: &str) -> Result<(), String> {
    let path = Path::new(shell);
    if !path.is_absolute() {
        return Err(format!("shell path must be absolute: {shell}"));
    }
    if !path.exists() {
        return Err(format!("shell path does not exist: {shell}"));
    }
    Ok(())
}

/// The Marauder daemon.
pub struct MarauderDaemon {
    sessions: Arc<std::sync::Mutex<HashMap<SessionId, Session>>>,
    socket_path: PathBuf,
    server: Option<IpcServer>,
    max_sessions: usize,
    shutdown_tx: broadcast::Sender<()>,
    shutdown_rx: broadcast::Receiver<()>,
}

impl MarauderDaemon {
    /// Create a new daemon (not yet started).
    pub fn new() -> Self {
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        Self {
            sessions: Arc::new(std::sync::Mutex::new(HashMap::new())),
            socket_path: default_socket_path(),
            server: None,
            max_sessions: DEFAULT_MAX_SESSIONS,
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Set a custom socket path.
    pub fn with_socket_path(mut self, path: impl AsRef<Path>) -> Self {
        self.socket_path = path.as_ref().to_path_buf();
        self
    }

    /// Set the maximum number of concurrent sessions.
    pub fn with_max_sessions(mut self, max: usize) -> Self {
        self.max_sessions = max;
        self
    }

    /// Get a receiver that fires when shutdown is requested via IPC.
    pub fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Wait for a shutdown signal (from IPC or other source).
    pub async fn wait_for_shutdown(&mut self) {
        let _ = self.shutdown_rx.recv().await;
    }

    /// Get a clone of the sessions map for external use (e.g., marauder-server).
    pub fn sessions(&self) -> Arc<std::sync::Mutex<HashMap<SessionId, Session>>> {
        Arc::clone(&self.sessions)
    }

    /// Start the daemon, binding the IPC server.
    pub async fn start(&mut self) -> Result<(), DaemonError> {
        if self.server.is_some() {
            return Err(DaemonError::AlreadyRunning);
        }

        // Ensure socket directory exists with restrictive permissions (0700)
        ensure_socket_dir_secure(&self.socket_path)?;

        let sessions = Arc::clone(&self.sessions);
        let max_sessions = self.max_sessions;
        let shutdown_tx = self.shutdown_tx.clone();
        let handler: RequestHandler = Arc::new(move |req| {
            Self::handle_request(&sessions, max_sessions, &shutdown_tx, req)
        });

        let server = IpcServer::bind(&self.socket_path, handler).await?;
        self.server = Some(server);

        tracing::info!(path = %self.socket_path.display(), "Marauder daemon started");
        Ok(())
    }

    /// Shut down the daemon.
    pub async fn shutdown(mut self) {
        if let Some(server) = self.server.take() {
            server.shutdown().await;
        }
        // Clean up sessions — kill all PTY processes
        let mut sessions = lock_or_log(&self.sessions, "daemon::shutdown");
        for (_, session) in sessions.iter_mut() {
            session.kill();
        }
        sessions.clear();
        tracing::info!("Marauder daemon shut down");
    }

    /// Get the socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Handle an incoming IPC request.
    fn handle_request(
        sessions: &Arc<std::sync::Mutex<HashMap<SessionId, Session>>>,
        max_sessions: usize,
        shutdown_tx: &broadcast::Sender<()>,
        request: IpcRequest,
    ) -> IpcMessage {
        match request {
            IpcRequest::Ping => {
                IpcMessage::ok(0, Some(serde_json::json!("pong")))
            }

            IpcRequest::CreateSession { shell, rows, cols } => {
                let shell = shell.unwrap_or_else(|| {
                    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
                });

                // Validate the shell path is absolute and exists
                if let Err(msg) = validate_shell_path(&shell) {
                    return IpcMessage::error(0, msg);
                }

                let rows = rows.unwrap_or(24);
                let cols = cols.unwrap_or(80);

                let mut locked = lock_or_log(sessions, "daemon::create_session");

                // Enforce session count limit
                if locked.len() >= max_sessions {
                    return IpcMessage::error(
                        0,
                        format!("session limit reached: max {max_sessions}"),
                    );
                }

                // Spawn a real PTY session
                match Session::create(shell, rows, cols) {
                    Ok(session) => {
                        let info = session.info();
                        let id = session.id;
                        locked.insert(id, session);
                        tracing::info!(session_id = id, "Session created with live PTY");
                        IpcMessage::ok(0, Some(serde_json::to_value(info).unwrap()))
                    }
                    Err(e) => {
                        tracing::error!("Failed to create session: {e}");
                        IpcMessage::error(0, format!("failed to spawn PTY: {e}"))
                    }
                }
            }

            IpcRequest::ListSessions => {
                let sessions = lock_or_log(sessions, "daemon::list_sessions");
                let infos: Vec<_> = sessions.values().map(|s| s.info()).collect();
                IpcMessage::ok(0, Some(serde_json::to_value(infos).unwrap()))
            }

            IpcRequest::AttachSession { session_id } => {
                let mut sessions = lock_or_log(sessions, "daemon::attach_session");
                match sessions.get_mut(&session_id) {
                    Some(session) => {
                        session.attach();
                        IpcMessage::ok(0, Some(serde_json::to_value(session.info()).unwrap()))
                    }
                    None => IpcMessage::error(0, format!("session not found: {session_id}")),
                }
            }

            IpcRequest::DetachSession { session_id } => {
                let mut sessions = lock_or_log(sessions, "daemon::detach_session");
                match sessions.get_mut(&session_id) {
                    Some(session) => {
                        session.detach();
                        IpcMessage::ok(0, None)
                    }
                    None => IpcMessage::error(0, format!("session not found: {session_id}")),
                }
            }

            IpcRequest::KillSession { session_id } => {
                let mut sessions = lock_or_log(sessions, "daemon::kill_session");
                match sessions.get_mut(&session_id) {
                    Some(session) => {
                        session.kill();
                        sessions.remove(&session_id);
                        tracing::info!(session_id, "Session killed");
                        IpcMessage::ok(0, None)
                    }
                    None => IpcMessage::error(0, format!("session not found: {session_id}")),
                }
            }

            IpcRequest::Resize { session_id, rows, cols } => {
                let mut sessions = lock_or_log(sessions, "daemon::resize");
                match sessions.get_mut(&session_id) {
                    Some(session) => {
                        match session.resize(rows, cols) {
                            Ok(()) => IpcMessage::ok(0, None),
                            Err(e) => IpcMessage::error(0, format!("resize failed: {e}")),
                        }
                    }
                    None => IpcMessage::error(0, format!("session not found: {session_id}")),
                }
            }

            IpcRequest::Write { session_id, data } => {
                let mut sessions = lock_or_log(sessions, "daemon::write");
                match sessions.get_mut(&session_id) {
                    Some(session) => {
                        match session.write_to_pty(&data) {
                            Ok(n) => {
                                tracing::trace!(session_id, bytes = n, "wrote to PTY");
                                IpcMessage::ok(0, None)
                            }
                            Err(e) => IpcMessage::error(0, format!("write failed: {e}")),
                        }
                    }
                    None => IpcMessage::error(0, format!("session not found: {session_id}")),
                }
            }

            IpcRequest::Shutdown => {
                tracing::info!("Shutdown requested via IPC");
                let _ = shutdown_tx.send(());
                IpcMessage::ok(0, Some(serde_json::json!("shutting down")))
            }
        }
    }
}

impl Default for MarauderDaemon {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marauder_ipc::message::IpcResponse;

    fn make_sessions() -> Arc<std::sync::Mutex<HashMap<SessionId, Session>>> {
        Arc::new(std::sync::Mutex::new(HashMap::new()))
    }

    fn make_shutdown() -> broadcast::Sender<()> {
        broadcast::channel(1).0
    }

    #[tokio::test]
    async fn test_daemon_start_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");

        let mut daemon = MarauderDaemon::new().with_socket_path(&sock);
        daemon.start().await.unwrap();
        assert!(sock.exists());

        daemon.shutdown().await;
    }

    #[tokio::test]
    async fn test_daemon_double_start_fails() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");

        let mut daemon = MarauderDaemon::new().with_socket_path(&sock);
        daemon.start().await.unwrap();
        let result = daemon.start().await;
        assert!(result.is_err());

        daemon.shutdown().await;
    }

    #[test]
    fn test_handle_ping() {
        let sessions = make_sessions();
        let shutdown_tx = make_shutdown();
        let resp = MarauderDaemon::handle_request(&sessions, DEFAULT_MAX_SESSIONS, &shutdown_tx, IpcRequest::Ping);
        assert!(matches!(
            resp.payload,
            marauder_ipc::message::IpcPayload::Response(IpcResponse::Ok { .. })
        ));
    }

    #[test]
    fn test_handle_create_and_list_sessions() {
        let sessions = make_sessions();
        let shutdown_tx = make_shutdown();

        // Create a session with a real shell
        let resp = MarauderDaemon::handle_request(
            &sessions,
            DEFAULT_MAX_SESSIONS,
            &shutdown_tx,
            IpcRequest::CreateSession {
                shell: Some("/bin/sh".into()),
                rows: Some(24),
                cols: Some(80),
            },
        );
        assert!(matches!(
            resp.payload,
            marauder_ipc::message::IpcPayload::Response(IpcResponse::Ok { .. })
        ));

        // List
        let resp = MarauderDaemon::handle_request(&sessions, DEFAULT_MAX_SESSIONS, &shutdown_tx, IpcRequest::ListSessions);
        if let marauder_ipc::message::IpcPayload::Response(IpcResponse::Ok { data }) = resp.payload {
            let arr = data.unwrap();
            assert_eq!(arr.as_array().unwrap().len(), 1);
        } else {
            panic!("expected Ok response");
        }
    }

    #[test]
    fn test_handle_kill_nonexistent() {
        let sessions = make_sessions();
        let shutdown_tx = make_shutdown();
        let resp = MarauderDaemon::handle_request(
            &sessions,
            DEFAULT_MAX_SESSIONS,
            &shutdown_tx,
            IpcRequest::KillSession { session_id: 999 },
        );
        assert!(matches!(
            resp.payload,
            marauder_ipc::message::IpcPayload::Response(IpcResponse::Error { .. })
        ));
    }

    #[test]
    fn test_default_socket_path_not_empty() {
        let path = default_socket_path();
        assert!(!path.as_os_str().is_empty());
    }

    #[test]
    fn test_session_limit_enforced() {
        let sessions = make_sessions();
        let shutdown_tx = make_shutdown();
        let max = 2;

        // Create 2 sessions (at limit)
        for _ in 0..2 {
            let resp = MarauderDaemon::handle_request(
                &sessions,
                max,
                &shutdown_tx,
                IpcRequest::CreateSession {
                    shell: Some("/bin/sh".into()),
                    rows: Some(24),
                    cols: Some(80),
                },
            );
            assert!(matches!(
                resp.payload,
                marauder_ipc::message::IpcPayload::Response(IpcResponse::Ok { .. })
            ));
        }

        // Third should fail
        let resp = MarauderDaemon::handle_request(
            &sessions,
            max,
            &shutdown_tx,
            IpcRequest::CreateSession {
                shell: Some("/bin/sh".into()),
                rows: Some(24),
                cols: Some(80),
            },
        );
        assert!(matches!(
            resp.payload,
            marauder_ipc::message::IpcPayload::Response(IpcResponse::Error { .. })
        ));
    }

    #[test]
    fn test_create_session_rejects_relative_shell() {
        let sessions = make_sessions();
        let shutdown_tx = make_shutdown();
        let resp = MarauderDaemon::handle_request(
            &sessions,
            DEFAULT_MAX_SESSIONS,
            &shutdown_tx,
            IpcRequest::CreateSession {
                shell: Some("sh".into()),
                rows: Some(24),
                cols: Some(80),
            },
        );
        assert!(matches!(
            resp.payload,
            marauder_ipc::message::IpcPayload::Response(IpcResponse::Error { .. })
        ));
    }

    #[test]
    fn test_create_session_rejects_nonexistent_shell() {
        let sessions = make_sessions();
        let shutdown_tx = make_shutdown();
        let resp = MarauderDaemon::handle_request(
            &sessions,
            DEFAULT_MAX_SESSIONS,
            &shutdown_tx,
            IpcRequest::CreateSession {
                shell: Some("/nonexistent/shell".into()),
                rows: Some(24),
                cols: Some(80),
            },
        );
        assert!(matches!(
            resp.payload,
            marauder_ipc::message::IpcPayload::Response(IpcResponse::Error { .. })
        ));
    }

    #[test]
    fn test_shutdown_sends_signal() {
        let sessions = make_sessions();
        let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);
        MarauderDaemon::handle_request(
            &sessions,
            DEFAULT_MAX_SESSIONS,
            &shutdown_tx,
            IpcRequest::Shutdown,
        );
        // Should have received the shutdown signal
        assert!(shutdown_rx.try_recv().is_ok());
    }

    #[test]
    fn test_write_to_session() {
        let sessions = make_sessions();
        let shutdown_tx = make_shutdown();

        // Create a session
        let resp = MarauderDaemon::handle_request(
            &sessions,
            DEFAULT_MAX_SESSIONS,
            &shutdown_tx,
            IpcRequest::CreateSession {
                shell: Some("/bin/sh".into()),
                rows: Some(24),
                cols: Some(80),
            },
        );
        // Extract session ID from the response
        let session_id = if let marauder_ipc::message::IpcPayload::Response(IpcResponse::Ok { data }) = &resp.payload {
            let info: serde_json::Value = data.clone().unwrap();
            info["id"].as_u64().unwrap()
        } else {
            panic!("expected Ok response");
        };

        // Write to the session — should succeed
        let resp = MarauderDaemon::handle_request(
            &sessions,
            DEFAULT_MAX_SESSIONS,
            &shutdown_tx,
            IpcRequest::Write {
                session_id,
                data: b"echo hello\n".to_vec(),
            },
        );
        assert!(matches!(
            resp.payload,
            marauder_ipc::message::IpcPayload::Response(IpcResponse::Ok { .. })
        ));
    }

    #[test]
    fn test_resize_session() {
        let sessions = make_sessions();
        let shutdown_tx = make_shutdown();

        // Create a session
        let resp = MarauderDaemon::handle_request(
            &sessions,
            DEFAULT_MAX_SESSIONS,
            &shutdown_tx,
            IpcRequest::CreateSession {
                shell: Some("/bin/sh".into()),
                rows: Some(24),
                cols: Some(80),
            },
        );
        let session_id = if let marauder_ipc::message::IpcPayload::Response(IpcResponse::Ok { data }) = &resp.payload {
            data.clone().unwrap()["id"].as_u64().unwrap()
        } else {
            panic!("expected Ok response");
        };

        // Resize — should succeed and update PTY + grid
        let resp = MarauderDaemon::handle_request(
            &sessions,
            DEFAULT_MAX_SESSIONS,
            &shutdown_tx,
            IpcRequest::Resize {
                session_id,
                rows: 48,
                cols: 120,
            },
        );
        assert!(matches!(
            resp.payload,
            marauder_ipc::message::IpcPayload::Response(IpcResponse::Ok { .. })
        ));
    }
}
