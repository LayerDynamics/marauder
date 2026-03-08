//! TCP IPC client — connects to the daemon over TCP.
//!
//! Same framing protocol as the Unix socket client, enabling remote
//! connections including from browser-based terminals.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::TcpStream;

use crate::error::IpcError;
use crate::framing::{FrameReader, FrameWriter};
use crate::message::{IpcMessage, IpcPayload, IpcRequest, IpcResponse};

/// TCP IPC client for connecting to the daemon over the network.
pub struct TcpIpcClient {
    frame_reader: FrameReader<tokio::net::tcp::OwnedReadHalf>,
    frame_writer: FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    next_id: AtomicU64,
}

impl TcpIpcClient {
    /// Connect to the daemon via TCP.
    pub async fn connect(addr: SocketAddr) -> Result<Self, IpcError> {
        let stream = TcpStream::connect(addr).await?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            frame_reader: FrameReader::new(reader),
            frame_writer: FrameWriter::new(writer),
            next_id: AtomicU64::new(1),
        })
    }

    /// Send a request and wait for the response.
    pub async fn request(&mut self, request: IpcRequest) -> Result<IpcResponse, IpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = IpcMessage::request(id, request);
        self.frame_writer.write_message(&msg).await?;

        let response = self
            .frame_reader
            .read_message()
            .await?
            .ok_or(IpcError::ConnectionClosed)?;

        match response.payload {
            IpcPayload::Response(resp) => Ok(resp),
            _ => Err(IpcError::ProtocolViolation),
        }
    }
}
