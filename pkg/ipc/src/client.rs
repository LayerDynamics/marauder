//! IPC client connecting to the daemon over a Unix domain socket.

#![cfg(unix)]

use std::path::Path;

use tokio::net::UnixStream;

use crate::error::IpcError;
use crate::framing::{FrameReader, FrameWriter};
use crate::message::{IpcMessage, IpcPayload, IpcRequest, IpcResponse};

/// Unix domain socket IPC client.
pub struct IpcClient {
    reader: FrameReader<tokio::net::unix::OwnedReadHalf>,
    writer: FrameWriter<tokio::net::unix::OwnedWriteHalf>,
    next_id: u64,
}

impl IpcClient {
    /// Connect to a daemon's Unix socket.
    pub async fn connect(socket_path: impl AsRef<Path>) -> Result<Self, IpcError> {
        let path = socket_path.as_ref();

        let stream = UnixStream::connect(path).await.map_err(|err| {
            match err.kind() {
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                    IpcError::ServerNotRunning
                }
                _ => IpcError::from(err),
            }
        })?;
        let (read_half, write_half) = stream.into_split();

        Ok(Self {
            reader: FrameReader::new(read_half),
            writer: FrameWriter::new(write_half),
            next_id: 1,
        })
    }

    /// Send a request and wait for the response.
    pub async fn request(&mut self, request: IpcRequest) -> Result<IpcResponse, IpcError> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = IpcMessage::request(id, request);

        self.writer.write_message(&msg).await?;

        match self.reader.read_message().await? {
            Some(response) => {
                if response.id != id {
                    return Err(IpcError::ResponseIdMismatch {
                        expected: id,
                        got: response.id,
                    });
                }
                match response.payload {
                    IpcPayload::Response(resp) => Ok(resp),
                    IpcPayload::Request(_) => Err(IpcError::ProtocolViolation),
                }
            }
            None => Err(IpcError::ConnectionClosed),
        }
    }

    /// Convenience: send a ping.
    pub async fn ping(&mut self) -> Result<bool, IpcError> {
        match self.request(IpcRequest::Ping).await? {
            IpcResponse::Ok { .. } => Ok(true),
            IpcResponse::Error { .. } => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{IpcServer, RequestHandler};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_client_server_ping() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");

        let handler: RequestHandler = Arc::new(|req| match req {
            IpcRequest::Ping => IpcMessage::ok(0, Some(serde_json::json!("pong"))),
            _ => IpcMessage::error(0, "unhandled"),
        });

        let server = IpcServer::bind(&sock, handler).await.unwrap();

        // Give server a moment to start accepting
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = IpcClient::connect(&sock).await.unwrap();
        let pong = client.ping().await.unwrap();
        assert!(pong);

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_client_connect_no_server() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("nonexistent.sock");
        let result = IpcClient::connect(&sock).await;
        assert!(matches!(result, Err(IpcError::ServerNotRunning)));
    }
}
