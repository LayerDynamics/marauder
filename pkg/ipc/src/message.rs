//! IPC message types exchanged between daemon and clients.

use serde::{Deserialize, Serialize};

/// Top-level IPC message envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcMessage {
    /// Unique message ID for request/response correlation.
    pub id: u64,
    /// The message payload.
    pub payload: IpcPayload,
}

/// Message payload — either a request or response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcPayload {
    Request(IpcRequest),
    Response(IpcResponse),
}

/// Requests from client → daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum IpcRequest {
    /// Ping the daemon (health check).
    Ping,

    /// Create a new session with optional config.
    CreateSession {
        #[serde(default)]
        shell: Option<String>,
        #[serde(default)]
        rows: Option<u16>,
        #[serde(default)]
        cols: Option<u16>,
    },

    /// Attach to an existing session.
    AttachSession { session_id: u64 },

    /// Detach from the current session.
    DetachSession { session_id: u64 },

    /// List all active sessions.
    ListSessions,

    /// Write data to a session's PTY.
    Write {
        session_id: u64,
        #[serde(with = "hex_bytes")]
        data: Vec<u8>,
    },

    /// Resize a session's PTY.
    Resize {
        session_id: u64,
        rows: u16,
        cols: u16,
    },

    /// Kill a session.
    KillSession { session_id: u64 },

    /// Shut down the daemon.
    Shutdown,
}

/// Responses from daemon → client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum IpcResponse {
    /// Success with optional data.
    Ok {
        #[serde(default)]
        data: Option<serde_json::Value>,
    },

    /// Error with message.
    Error { message: String },
}

impl IpcMessage {
    /// Create a new request message.
    pub fn request(id: u64, request: IpcRequest) -> Self {
        Self {
            id,
            payload: IpcPayload::Request(request),
        }
    }

    /// Create a new response message.
    pub fn response(id: u64, response: IpcResponse) -> Self {
        Self {
            id,
            payload: IpcPayload::Response(response),
        }
    }

    /// Create an OK response.
    pub fn ok(id: u64, data: Option<serde_json::Value>) -> Self {
        Self::response(id, IpcResponse::Ok { data })
    }

    /// Create an error response.
    pub fn error(id: u64, message: impl Into<String>) -> Self {
        Self::response(id, IpcResponse::Error { message: message.into() })
    }
}

/// Hex encoding for byte data in JSON.
mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};
    use serde::de::Error;

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Serialize;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        hex.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() % 2 != 0 {
            return Err(D::Error::custom("odd-length hex string"));
        }
        (0..s.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&s[i..i + 2], 16).map_err(D::Error::custom)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping_roundtrip() {
        let msg = IpcMessage::request(1, IpcRequest::Ping);
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: IpcMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
        assert!(matches!(parsed.payload, IpcPayload::Request(IpcRequest::Ping)));
    }

    #[test]
    fn test_ok_response() {
        let msg = IpcMessage::ok(42, Some(serde_json::json!({"sessions": [1, 2]})));
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: IpcMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 42);
        assert!(matches!(parsed.payload, IpcPayload::Response(IpcResponse::Ok { .. })));
    }

    #[test]
    fn test_error_response() {
        let msg = IpcMessage::error(5, "not found");
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: IpcMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed.payload,
            IpcPayload::Response(IpcResponse::Error { .. })
        ));
    }

    #[test]
    fn test_write_with_bytes() {
        let msg = IpcMessage::request(
            3,
            IpcRequest::Write {
                session_id: 1,
                data: b"hello".to_vec(),
            },
        );
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: IpcMessage = serde_json::from_str(&json).unwrap();
        if let IpcPayload::Request(IpcRequest::Write { data, .. }) = parsed.payload {
            assert_eq!(data, b"hello");
        } else {
            panic!("wrong variant");
        }
    }
}
