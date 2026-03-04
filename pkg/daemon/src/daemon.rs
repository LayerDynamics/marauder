//! The Marauder daemon — headless multiplexer process.
//!
//! Manages sessions and handles IPC requests from clients.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
        // Clean up sessions
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
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

                let mut locked = sessions.lock().unwrap_or_else(|e| e.into_inner());

                // Enforce session count limit
                if locked.len() >= max_sessions {
                    return IpcMessage::error(
                        0,
                        format!("session limit reached: max {max_sessions}"),
                    );
                }

                let session = Session::new(shell, rows, cols);
                let info = session.info();
                let id = session.id;

                locked.insert(id, session);
                tracing::info!(session_id = id, "Session created");

                IpcMessage::ok(0, Some(serde_json::to_value(info).unwrap()))
            }

            IpcRequest::ListSessions => {
                let sessions = sessions.lock().unwrap_or_else(|e| e.into_inner());
                let infos: Vec<_> = sessions.values().map(|s| s.info()).collect();
                IpcMessage::ok(0, Some(serde_json::to_value(infos).unwrap()))
            }

            IpcRequest::AttachSession { session_id } => {
                let mut sessions = sessions.lock().unwrap_or_else(|e| e.into_inner());
                match sessions.get_mut(&session_id) {
                    Some(session) => {
                        session.attach();
                        IpcMessage::ok(0, Some(serde_json::to_value(session.info()).unwrap()))
                    }
                    None => IpcMessage::error(0, format!("session not found: {session_id}")),
                }
            }

            IpcRequest::DetachSession { session_id } => {
                let mut sessions = sessions.lock().unwrap_or_else(|e| e.into_inner());
                match sessions.get_mut(&session_id) {
                    Some(session) => {
                        session.detach();
                        IpcMessage::ok(0, None)
                    }
                    None => IpcMessage::error(0, format!("session not found: {session_id}")),
                }
            }

            IpcRequest::KillSession { session_id } => {
                let mut sessions = sessions.lock().unwrap_or_else(|e| e.into_inner());
                match sessions.remove(&session_id) {
                    Some(_) => {
                        tracing::info!(session_id, "Session killed");
                        IpcMessage::ok(0, None)
                    }
                    None => IpcMessage::error(0, format!("session not found: {session_id}")),
                }
            }

            IpcRequest::Resize { session_id, rows, cols } => {
                let mut sessions = sessions.lock().unwrap_or_else(|e| e.into_inner());
                match sessions.get_mut(&session_id) {
                    Some(session) => {
                        session.rows = rows;
                        session.cols = cols;
                        IpcMessage::ok(0, None)
                    }
                    None => IpcMessage::error(0, format!("session not found: {session_id}")),
                }
            }

            IpcRequest::Write { session_id, data: _ } => {
                // Phase 1 skeleton: acknowledge but don't actually write to PTY yet
                let sessions = sessions.lock().unwrap_or_else(|e| e.into_inner());
                if sessions.contains_key(&session_id) {
                    IpcMessage::ok(0, None)
                } else {
                    IpcMessage::error(0, format!("session not found: {session_id}"))
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

        // Create
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
}
