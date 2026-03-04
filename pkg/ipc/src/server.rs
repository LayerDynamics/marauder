//! IPC server listening on a Unix domain socket.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::net::UnixListener;
use tokio::sync::{broadcast, Semaphore};

use crate::error::IpcError;
use crate::framing::{FrameReader, FrameWriter};
use crate::message::{IpcMessage, IpcPayload, IpcRequest};

/// Maximum number of concurrent connections.
const MAX_CONNECTIONS: usize = 256;

/// Callback type for handling incoming requests.
/// Returns an `IpcMessage` response for the given request.
pub type RequestHandler = Arc<dyn Fn(IpcRequest) -> IpcMessage + Send + Sync>;

/// Unix domain socket IPC server.
pub struct IpcServer {
    socket_path: PathBuf,
    shutdown_tx: broadcast::Sender<()>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl IpcServer {
    /// Create and start an IPC server.
    ///
    /// Binds to the given Unix socket path and spawns a background task
    /// that accepts connections and dispatches requests to the handler.
    pub async fn bind(
        socket_path: impl AsRef<Path>,
        handler: RequestHandler,
    ) -> Result<Self, IpcError> {
        let socket_path = socket_path.as_ref().to_path_buf();

        // Remove stale socket file if it exists (TOCTOU-safe)
        match std::fs::remove_file(&socket_path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(IpcError::Io(e)),
        }

        // Ensure parent directory exists
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        tracing::info!(path = %socket_path.display(), "IPC server listening");

        let (shutdown_tx, _) = broadcast::channel(1);
        let mut shutdown_rx = shutdown_tx.subscribe();
        let conn_shutdown_tx = shutdown_tx.clone();

        let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, _addr)) => {
                                let handler = Arc::clone(&handler);
                                let permit = match semaphore.clone().acquire_owned().await {
                                    Ok(permit) => permit,
                                    Err(_) => break, // semaphore closed
                                };
                                let mut conn_shutdown_rx = conn_shutdown_tx.subscribe();
                                tokio::spawn(async move {
                                    let _permit = permit;
                                    let (read_half, write_half) = stream.into_split();
                                    let mut reader = FrameReader::new(read_half);
                                    let mut writer = FrameWriter::new(write_half);

                                    loop {
                                        tokio::select! {
                                            read_result = reader.read_message() => {
                                                match read_result {
                                                    Ok(Some(msg)) => {
                                                        let response = match msg.payload {
                                                            IpcPayload::Request(req) => {
                                                                let handler = Arc::clone(&handler);
                                                                let msg_id = msg.id;
                                                                match tokio::task::spawn_blocking(move || {
                                                                    let mut resp = handler(req);
                                                                    resp.id = msg_id;
                                                                    resp
                                                                }).await {
                                                                    Ok(resp) => resp,
                                                                    Err(e) => {
                                                                        tracing::error!(error = %e, "handler task panicked");
                                                                        IpcMessage::error(msg.id, "internal error")
                                                                    }
                                                                }
                                                            }
                                                            IpcPayload::Response(_) => {
                                                                IpcMessage::error(msg.id, "unexpected response from client")
                                                            }
                                                        };
                                                        if let Err(e) = writer.write_message(&response).await {
                                                            tracing::debug!(error = %e, "failed to write response");
                                                            break;
                                                        }
                                                    }
                                                    Ok(None) => break, // connection closed
                                                    Err(e) => {
                                                        tracing::debug!(error = %e, "client read error");
                                                        break;
                                                    }
                                                }
                                            }
                                            _ = conn_shutdown_rx.recv() => {
                                                tracing::debug!("connection task received shutdown signal");
                                                break;
                                            }
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "accept error");
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        tracing::info!("IPC server shutting down");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            socket_path,
            shutdown_tx,
            handle: Some(handle),
        })
    }

    /// Get the socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Shut down the server gracefully.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        // Clean up socket file
        let _ = std::fs::remove_file(&self.socket_path);
        tracing::info!(path = %self.socket_path.display(), "IPC server stopped");
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // Best-effort cleanup if not shut down gracefully
        let _ = self.shutdown_tx.send(());
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_server_bind_and_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");

        let handler: RequestHandler = Arc::new(|_req| {
            IpcMessage::ok(0, None)
        });

        let server = IpcServer::bind(&sock, handler).await.unwrap();
        assert!(sock.exists());

        server.shutdown().await;
        assert!(!sock.exists());
    }
}
