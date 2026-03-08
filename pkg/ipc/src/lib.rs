//! IPC transport layer for the Marauder multiplexer daemon.
//!
//! Provides framed message passing over multiple transports:
//! - Unix domain sockets (primary, local-only)
//! - TCP (for remote clients and WebSocket bridges)
//!
//! All transports use the same framing protocol and message types.

pub mod error;
pub mod message;
pub mod framing;
#[cfg(unix)]
pub mod server;
#[cfg(unix)]
pub mod client;
pub mod tcp_server;
pub mod tcp_client;

pub use error::IpcError;
pub use message::{IpcMessage, IpcRequest, IpcResponse};
pub use framing::{FrameReader, FrameWriter};
#[cfg(unix)]
pub use server::IpcServer;
#[cfg(unix)]
pub use client::IpcClient;
pub use tcp_server::TcpIpcServer;
pub use tcp_client::TcpIpcClient;
