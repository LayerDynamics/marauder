//! Collaborative terminal support — multi-client session broadcasting
//! and user presence tracking.
//!
//! When multiple clients are attached to the same session, grid updates
//! and PTY output are broadcast to all of them. Each client is assigned
//! a unique color for cursor/presence indicators.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::session::SessionId;

/// Unique client identifier within a collaborative session.
pub type ClientId = u64;

/// Client presence information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientPresence {
    pub client_id: ClientId,
    pub display_name: String,
    /// RGBA cursor color for this user's presence indicator.
    pub cursor_color: [f32; 4],
    /// Last known cursor position (row, col).
    pub cursor_pos: Option<(u32, u32)>,
    /// True if this client is actively typing.
    pub is_active: bool,
}

/// Events broadcast to all clients in a collaborative session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CollabEvent {
    /// PTY output bytes (forwarded to all attached clients).
    PtyOutput(Vec<u8>),
    /// A client's cursor position changed.
    CursorMoved {
        client_id: ClientId,
        row: u32,
        col: u32,
    },
    /// A client joined the session.
    ClientJoined(ClientPresence),
    /// A client left the session.
    ClientLeft { client_id: ClientId },
    /// Grid was resized.
    GridResized { rows: u16, cols: u16 },
}

/// Predefined cursor colors for collaborative users (up to 8).
const COLLAB_COLORS: [[f32; 4]; 8] = [
    [0.537, 0.706, 0.980, 0.8],  // Blue
    [0.647, 0.824, 0.494, 0.8],  // Green
    [0.980, 0.706, 0.537, 0.8],  // Orange
    [0.816, 0.537, 0.980, 0.8],  // Purple
    [0.980, 0.537, 0.537, 0.8],  // Red
    [0.537, 0.980, 0.886, 0.8],  // Teal
    [0.980, 0.980, 0.537, 0.8],  // Yellow
    [0.980, 0.537, 0.816, 0.8],  // Pink
];

/// Manages collaborative state for a single session.
pub struct SessionCollaborator {
    pub session_id: SessionId,
    clients: HashMap<ClientId, ClientPresence>,
    next_client_id: ClientId,
    broadcast_tx: broadcast::Sender<CollabEvent>,
}

impl SessionCollaborator {
    /// Create a new collaborator for a session.
    pub fn new(session_id: SessionId) -> Self {
        let (broadcast_tx, _) = broadcast::channel(256);
        Self {
            session_id,
            clients: HashMap::new(),
            next_client_id: 1,
            broadcast_tx,
        }
    }

    /// Add a client to the session. Returns the client ID and a broadcast receiver.
    pub fn add_client(
        &mut self,
        display_name: String,
    ) -> (ClientId, broadcast::Receiver<CollabEvent>) {
        let client_id = self.next_client_id;
        self.next_client_id += 1;

        let color_idx = (client_id as usize - 1) % COLLAB_COLORS.len();
        let presence = ClientPresence {
            client_id,
            display_name,
            cursor_color: COLLAB_COLORS[color_idx],
            cursor_pos: None,
            is_active: true,
        };

        self.clients.insert(client_id, presence.clone());
        let rx = self.broadcast_tx.subscribe();

        // Notify existing clients
        let _ = self.broadcast_tx.send(CollabEvent::ClientJoined(presence));

        (client_id, rx)
    }

    /// Remove a client from the session.
    pub fn remove_client(&mut self, client_id: ClientId) {
        self.clients.remove(&client_id);
        let _ = self
            .broadcast_tx
            .send(CollabEvent::ClientLeft { client_id });
    }

    /// Update a client's cursor position.
    pub fn update_cursor(&mut self, client_id: ClientId, row: u32, col: u32) {
        if let Some(presence) = self.clients.get_mut(&client_id) {
            presence.cursor_pos = Some((row, col));
            let _ = self.broadcast_tx.send(CollabEvent::CursorMoved {
                client_id,
                row,
                col,
            });
        }
    }

    /// Broadcast PTY output to all clients.
    pub fn broadcast_output(&self, data: Vec<u8>) {
        let _ = self.broadcast_tx.send(CollabEvent::PtyOutput(data));
    }

    /// Broadcast a grid resize event.
    pub fn broadcast_resize(&self, rows: u16, cols: u16) {
        let _ = self
            .broadcast_tx
            .send(CollabEvent::GridResized { rows, cols });
    }

    /// Get all current client presences.
    pub fn clients(&self) -> Vec<ClientPresence> {
        self.clients.values().cloned().collect()
    }

    /// Get the number of connected clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Subscribe to broadcast events (for new connections that need a receiver).
    pub fn subscribe(&self) -> broadcast::Receiver<CollabEvent> {
        self.broadcast_tx.subscribe()
    }
}

/// Manages collaborative sessions across the daemon.
pub struct CollabManager {
    sessions: HashMap<SessionId, SessionCollaborator>,
}

impl CollabManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Get or create a collaborator for a session.
    pub fn get_or_create(&mut self, session_id: SessionId) -> &mut SessionCollaborator {
        self.sessions
            .entry(session_id)
            .or_insert_with(|| SessionCollaborator::new(session_id))
    }

    /// Remove a session's collaborator (when session is killed).
    pub fn remove_session(&mut self, session_id: SessionId) {
        self.sessions.remove(&session_id);
    }

    /// Get a session's collaborator if it exists.
    pub fn get(&self, session_id: SessionId) -> Option<&SessionCollaborator> {
        self.sessions.get(&session_id)
    }

    /// Get a mutable reference to a session's collaborator.
    pub fn get_mut(&mut self, session_id: SessionId) -> Option<&mut SessionCollaborator> {
        self.sessions.get_mut(&session_id)
    }
}

impl Default for CollabManager {
    fn default() -> Self {
        Self::new()
    }
}
