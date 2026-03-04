use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EventError {
    #[error("failed to serialize event payload: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("invalid event type discriminant: {0}")]
    InvalidEventType(u32),
}

/// Categories of events flowing through the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum EventType {
    // Input layer
    KeyInput = 0,
    MouseInput = 1,
    PasteInput = 2,

    // PTY layer
    PtyOutput = 3,
    PtyExit = 4,
    PtyError = 5,

    // Parser layer
    ParserAction = 6,

    // Grid layer
    GridUpdated = 7,
    GridResized = 8,
    GridScrolled = 9,
    SelectionChanged = 10,

    // Shell layer
    ShellPromptDetected = 11,
    ShellCommandStarted = 12,
    ShellCommandFinished = 13,
    ShellCwdChanged = 14,

    // Render layer
    RenderFrameRequested = 15,
    RenderFrameCompleted = 16,
    OverlayChanged = 17,

    // Config layer
    ConfigChanged = 18,
    ConfigError = 19,

    // Lifecycle
    SessionCreated = 20,
    SessionClosed = 21,
    PaneCreated = 22,
    PaneClosed = 23,
    PaneFocused = 24,
    TabCreated = 25,
    TabClosed = 26,
    TabFocused = 27,

    // Extension layer
    ExtensionLoaded = 28,
    ExtensionUnloaded = 29,
    ExtensionMessage = 30,
}

impl EventType {
    /// Maximum valid discriminant value.
    pub const MAX_DISCRIMINANT: u32 = 30;

    /// Try to convert a u32 discriminant to an EventType.
    pub fn from_u32(value: u32) -> Result<Self, EventError> {
        if value > Self::MAX_DISCRIMINANT {
            return Err(EventError::InvalidEventType(value));
        }
        // SAFETY: value is within the valid range of the #[repr(u32)] enum
        Ok(unsafe { std::mem::transmute(value) })
    }

    /// Convert to u32 discriminant.
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

/// A typed event carrying serialized payload data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_type: EventType,
    /// JSON-serialized payload. Consumers deserialize to the expected type.
    pub payload: Vec<u8>,
    /// Monotonic timestamp in microseconds.
    pub timestamp_us: u64,
    /// Optional source identifier (e.g., pane ID, extension name).
    pub source: Option<String>,
}

impl Event {
    /// Create a new event with the given type and payload.
    /// Returns an error if the payload cannot be serialized.
    pub fn try_new(event_type: EventType, payload: impl Serialize) -> Result<Self, EventError> {
        let payload_bytes = serde_json::to_vec(&payload)?;
        Ok(Self {
            event_type,
            payload: payload_bytes,
            timestamp_us: Self::now_us(),
            source: None,
        })
    }

    /// Create a new event, logging serialization failures.
    /// Falls back to empty payload on error (for convenience in non-critical paths).
    pub fn new(event_type: EventType, payload: impl Serialize) -> Self {
        match Self::try_new(event_type, payload) {
            Ok(event) => event,
            Err(e) => {
                tracing::warn!(event_type = ?event_type, error = %e, "Failed to serialize event payload");
                Self {
                    event_type,
                    payload: Vec::new(),
                    timestamp_us: Self::now_us(),
                    source: None,
                }
            }
        }
    }

    /// Create a new event with a source identifier.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Deserialize the payload into the expected type.
    pub fn payload_as<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.payload)
    }

    /// Get the current timestamp in microseconds.
    pub fn now_us() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }
}
