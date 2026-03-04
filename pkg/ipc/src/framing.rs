//! Length-prefixed frame encoding/decoding for IPC messages.
//!
//! Wire format: [4-byte big-endian length] [JSON payload]
//! Maximum frame size: 16 MiB (configurable).

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::IpcError;
use crate::message::IpcMessage;

/// Maximum frame size (16 MiB).
pub const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024;

/// Reads length-prefixed frames from an async reader.
pub struct FrameReader<R> {
    reader: R,
    max_frame_size: u32,
}

impl<R: AsyncReadExt + Unpin> FrameReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            max_frame_size: MAX_FRAME_SIZE,
        }
    }

    /// Set the maximum allowed frame size.
    pub fn with_max_frame_size(mut self, max: u32) -> Self {
        self.max_frame_size = max;
        self
    }

    /// Read the next message, or `None` if the connection is closed.
    pub async fn read_message(&mut self) -> Result<Option<IpcMessage>, IpcError> {
        // Read 4-byte length prefix
        let mut len_buf = [0u8; 4];
        match self.reader.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(IpcError::Io(e)),
        }

        let len = u32::from_be_bytes(len_buf);
        if len > self.max_frame_size {
            return Err(IpcError::FrameTooLarge {
                size: len,
                max: self.max_frame_size,
            });
        }

        // Read payload
        let mut payload = vec![0u8; len as usize];
        self.reader.read_exact(&mut payload).await?;

        let message = serde_json::from_slice(&payload)?;
        Ok(Some(message))
    }
}

/// Writes length-prefixed frames to an async writer.
pub struct FrameWriter<W> {
    writer: W,
    max_frame_size: u32,
}

impl<W: AsyncWriteExt + Unpin> FrameWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            max_frame_size: MAX_FRAME_SIZE,
        }
    }

    /// Set the maximum allowed frame size.
    pub fn with_max_frame_size(mut self, max: u32) -> Self {
        self.max_frame_size = max;
        self
    }

    /// Write a message as a length-prefixed frame.
    pub async fn write_message(&mut self, message: &IpcMessage) -> Result<(), IpcError> {
        let payload = serde_json::to_vec(message)?;
        let len = u32::try_from(payload.len()).map_err(|_| IpcError::FrameTooLarge {
            size: u32::MAX,
            max: self.max_frame_size,
        })?;

        if len > self.max_frame_size {
            return Err(IpcError::FrameTooLarge {
                size: len,
                max: self.max_frame_size,
            });
        }

        self.writer.write_all(&len.to_be_bytes()).await?;
        self.writer.write_all(&payload).await?;
        self.writer.flush().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_frame_roundtrip() {
        let msg = IpcMessage::request(1, crate::message::IpcRequest::Ping);

        // Write to buffer
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf);
        writer.write_message(&msg).await.unwrap();

        // Read back
        let mut reader = FrameReader::new(buf.as_slice());
        let parsed = reader.read_message().await.unwrap().unwrap();
        assert_eq!(parsed.id, 1);
    }

    #[tokio::test]
    async fn test_eof_returns_none() {
        let buf: &[u8] = &[];
        let mut reader = FrameReader::new(buf);
        let result = reader.read_message().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_frame_too_large() {
        // Craft a length prefix that exceeds max
        let len_bytes = (MAX_FRAME_SIZE + 1).to_be_bytes();
        let mut reader = FrameReader::new(len_bytes.as_slice());
        let result = reader.read_message().await;
        assert!(matches!(result, Err(IpcError::FrameTooLarge { .. })));
    }
}
