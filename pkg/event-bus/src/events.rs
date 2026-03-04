use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Epoch anchor: the Instant at process start, paired with the corresponding SystemTime.
/// This lets us derive monotonic timestamps relative to a known epoch.
static EPOCH_ANCHOR: OnceLock<(Instant, u64)> = OnceLock::new();

fn epoch_anchor() -> &'static (Instant, u64) {
    EPOCH_ANCHOR.get_or_init(|| {
        let now_system = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        (Instant::now(), now_system)
    })
}

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
        match value {
            0 => Ok(Self::KeyInput),
            1 => Ok(Self::MouseInput),
            2 => Ok(Self::PasteInput),
            3 => Ok(Self::PtyOutput),
            4 => Ok(Self::PtyExit),
            5 => Ok(Self::PtyError),
            6 => Ok(Self::ParserAction),
            7 => Ok(Self::GridUpdated),
            8 => Ok(Self::GridResized),
            9 => Ok(Self::GridScrolled),
            10 => Ok(Self::SelectionChanged),
            11 => Ok(Self::ShellPromptDetected),
            12 => Ok(Self::ShellCommandStarted),
            13 => Ok(Self::ShellCommandFinished),
            14 => Ok(Self::ShellCwdChanged),
            15 => Ok(Self::RenderFrameRequested),
            16 => Ok(Self::RenderFrameCompleted),
            17 => Ok(Self::OverlayChanged),
            18 => Ok(Self::ConfigChanged),
            19 => Ok(Self::ConfigError),
            20 => Ok(Self::SessionCreated),
            21 => Ok(Self::SessionClosed),
            22 => Ok(Self::PaneCreated),
            23 => Ok(Self::PaneClosed),
            24 => Ok(Self::PaneFocused),
            25 => Ok(Self::TabCreated),
            26 => Ok(Self::TabClosed),
            27 => Ok(Self::TabFocused),
            28 => Ok(Self::ExtensionLoaded),
            29 => Ok(Self::ExtensionUnloaded),
            30 => Ok(Self::ExtensionMessage),
            _ => Err(EventError::InvalidEventType(value)),
        }
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

    /// Get a monotonic timestamp in microseconds (anchored to UNIX epoch at process start).
    pub fn now_us() -> u64 {
        let (anchor_instant, anchor_us) = epoch_anchor();
        let elapsed = anchor_instant.elapsed().as_micros() as u64;
        anchor_us + elapsed
    }
}
