//! TCP IPC server — same framing protocol as Unix socket, accessible over the network.
//!
//! Enables remote clients (including browser-based terminals via WebSocket upgrade)
//! to connect to the Marauder daemon. Uses the same message types and frame format
//! as the Unix socket transport.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{broadcast, Semaphore};

use crate::error::IpcError;
use crate::framing::{FrameReader, FrameWriter};
use crate::message::{IpcMessage, IpcPayload, IpcRequest};

/// Maximum number of concurrent TCP connections.
const MAX_TCP_CONNECTIONS: usize = 64;

/// Callback type for handling incoming requests.
pub type TcpRequestHandler = Arc<dyn Fn(IpcRequest) -> IpcMessage + Send + Sync>;

/// TCP IPC server.
pub struct TcpIpcServer {
    addr: SocketAddr,
    shutdown_tx: broadcast::Sender<()>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl TcpIpcServer {
    /// Start a TCP IPC server on the given address.
    pub async fn bind(
        addr: SocketAddr,
        handler: TcpRequestHandler,
    ) -> Result<Self, IpcError> {
        let listener = TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;
        tracing::info!(%actual_addr, "TCP IPC server listening");

        let (shutdown_tx, _) = broadcast::channel(1);
        let mut shutdown_rx = shutdown_tx.subscribe();

        let semaphore = Arc::new(Semaphore::new(MAX_TCP_CONNECTIONS));

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, peer_addr)) => {
                                let handler = Arc::clone(&handler);
                                let permit = match semaphore.clone().try_acquire_owned() {
                                    Ok(p) => p,
                                    Err(_) => {
                                        tracing::warn!(%peer_addr, "TCP connection limit reached, dropping");
                                        continue;
                                    }
                                };

                                tokio::spawn(async move {
                                    let _permit = permit;
                                    let (reader, writer) = stream.into_split();
                                    let mut frame_reader = FrameReader::new(reader);
                                    let mut frame_writer = FrameWriter::new(writer);

                                    loop {
                                        match frame_reader.read_message().await {
                                            Ok(Some(msg)) => {
                                                let response = match msg.payload {
                                                    IpcPayload::Request(req) => {
                                                        let resp = handler(req);
                                                        // Preserve the request's message ID in the response
                                                        IpcMessage { id: msg.id, payload: resp.payload }
                                                    }
                                                    _ => IpcMessage::error(msg.id, "expected request"),
                                                };

                                                if frame_writer.write_message(&response).await.is_err() {
                                                    break;
                                                }
                                            }
                                            Ok(None) => break, // connection closed
                                            Err(e) => {
                                                tracing::debug!(%peer_addr, "read error: {e}");
                                                break;
                                            }
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::error!("TCP accept error: {e}");
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        tracing::info!("TCP IPC server shutting down");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            addr: actual_addr,
            shutdown_tx,
            handle: Some(handle),
        })
    }

    /// Get the bound address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Shut down the server.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}
