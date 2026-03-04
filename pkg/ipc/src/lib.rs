//! Unix socket IPC transport for the Marauder multiplexer daemon.
//!
//! Provides framed message passing over Unix domain sockets with
//! serde JSON serialization. Used by `pkg/daemon` for client ↔ server
//! communication.
//!
//! Phase 1: Types, framing, and basic server/client skeletons.
//! Full implementation (reconnection, multiplexing, auth) comes later.

pub mod error;
pub mod message;
pub mod framing;
#[cfg(unix)]
pub mod server;
#[cfg(unix)]
pub mod client;

pub use error::IpcError;
pub use message::{IpcMessage, IpcRequest, IpcResponse};
pub use framing::{FrameReader, FrameWriter};
#[cfg(unix)]
pub use server::IpcServer;
#[cfg(unix)]
pub use client::IpcClient;
