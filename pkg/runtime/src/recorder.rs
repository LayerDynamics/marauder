//! Session recording — captures PTY I/O with timestamps for replay and export.
//!
//! Records terminal sessions in a format compatible with asciinema v2 (.cast files).
//! The recorder captures raw PTY output bytes and input events with microsecond-precision
//! timestamps relative to session start.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// A single recorded event in the session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordEvent {
    /// Time offset from session start in seconds (float).
    pub time: f64,
    /// Event type: "o" for output, "i" for input.
    pub event_type: String,
    /// Event data (raw bytes as UTF-8 string, may contain escape sequences).
    pub data: String,
}

/// Session metadata for the recording header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingHeader {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub timestamp: Option<u64>,
    pub title: Option<String>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

impl Default for RecordingHeader {
    fn default() -> Self {
        Self {
            version: 2,
            width: 80,
            height: 24,
            timestamp: None,
            title: None,
            env: None,
        }
    }
}

/// Recording state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderState {
    Idle,
    Recording,
    Paused,
}

/// Session recorder that captures PTY I/O events with timestamps.
pub struct SessionRecorder {
    state: RecorderState,
    start_time: Option<Instant>,
    events: Vec<RecordEvent>,
    header: RecordingHeader,
    /// Maximum number of events before auto-flush (0 = unlimited).
    max_events: usize,
}

impl SessionRecorder {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            state: RecorderState::Idle,
            start_time: None,
            events: Vec::with_capacity(4096),
            header: RecordingHeader {
                width,
                height,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs()),
                ..Default::default()
            },
            max_events: 0,
        }
    }

    /// Start recording.
    pub fn start(&mut self) {
        self.state = RecorderState::Recording;
        self.start_time = Some(Instant::now());
        self.events.clear();
        tracing::info!("Session recording started");
    }

    /// Stop recording and return the event count.
    pub fn stop(&mut self) -> usize {
        let count = self.events.len();
        self.state = RecorderState::Idle;
        tracing::info!("Session recording stopped ({} events)", count);
        count
    }

    /// Pause recording (events are discarded while paused).
    pub fn pause(&mut self) {
        if self.state == RecorderState::Recording {
            self.state = RecorderState::Paused;
        }
    }

    /// Resume recording after pause.
    pub fn resume(&mut self) {
        if self.state == RecorderState::Paused {
            self.state = RecorderState::Recording;
        }
    }

    pub fn state(&self) -> RecorderState {
        self.state
    }

    /// Record an output event (PTY → terminal).
    pub fn record_output(&mut self, data: &[u8]) {
        if self.state != RecorderState::Recording {
            return;
        }
        let time = self.elapsed();
        let text = String::from_utf8_lossy(data).into_owned();
        self.events.push(RecordEvent {
            time,
            event_type: "o".into(),
            data: text,
        });
    }

    /// Record an input event (user → PTY).
    pub fn record_input(&mut self, data: &[u8]) {
        if self.state != RecorderState::Recording {
            return;
        }
        let time = self.elapsed();
        let text = String::from_utf8_lossy(data).into_owned();
        self.events.push(RecordEvent {
            time,
            event_type: "i".into(),
            data: text,
        });
    }

    /// Export to asciinema v2 format (.cast).
    pub fn export_asciinema(&self, path: &PathBuf) -> anyhow::Result<()> {
        let mut file = std::fs::File::create(path)?;

        // Write header line (JSON object)
        let header_json = serde_json::to_string(&self.header)?;
        writeln!(file, "{}", header_json)?;

        // Write event lines: [time, event_type, data]
        for event in &self.events {
            let line = serde_json::to_string(&[
                serde_json::Value::from(event.time),
                serde_json::Value::from(event.event_type.as_str()),
                serde_json::Value::from(event.data.as_str()),
            ])?;
            writeln!(file, "{}", line)?;
        }

        tracing::info!("Exported {} events to {}", self.events.len(), path.display());
        Ok(())
    }

    /// Get all recorded events (for replay).
    pub fn events(&self) -> &[RecordEvent] {
        &self.events
    }

    /// Search recorded events for a pattern.
    pub fn search(&self, pattern: &str) -> Vec<(usize, &RecordEvent)> {
        self.events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.data.contains(pattern))
            .collect()
    }

    /// Get recording duration in seconds.
    pub fn duration(&self) -> f64 {
        self.events.last().map(|e| e.time).unwrap_or(0.0)
    }

    fn elapsed(&self) -> f64 {
        self.start_time
            .map(|s| s.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    /// Set the recording title.
    pub fn set_title(&mut self, title: String) {
        self.header.title = Some(title);
    }

    /// Set max events limit.
    pub fn set_max_events(&mut self, max: usize) {
        self.max_events = max;
    }
}

/// Thread-safe shared recorder handle.
pub type SharedRecorder = Arc<Mutex<SessionRecorder>>;

/// Create a new shared recorder.
pub fn create_shared_recorder(width: u32, height: u32) -> SharedRecorder {
    Arc::new(Mutex::new(SessionRecorder::new(width, height)))
}

// ─── C ABI exports ───────────────────────────────────────────────────────

/// Opaque handle for FFI.
pub struct RecorderHandle {
    inner: SharedRecorder,
}

/// Create a new session recorder.
///
/// # Safety
/// Returns a heap-allocated handle. Caller must free with `recorder_destroy`.
#[no_mangle]
pub extern "C" fn recorder_create(width: u32, height: u32) -> *mut RecorderHandle {
    let handle = Box::new(RecorderHandle {
        inner: create_shared_recorder(width, height),
    });
    Box::into_raw(handle)
}

/// Start recording.
///
/// # Safety
/// `handle` must be a valid pointer from `recorder_create`.
#[no_mangle]
pub unsafe extern "C" fn recorder_start(handle: *mut RecorderHandle) {
    // SAFETY: caller guarantees handle is valid
    let h = unsafe { &*handle };
    h.inner.lock().unwrap_or_else(|e| e.into_inner()).start();
}

/// Stop recording. Returns event count.
///
/// # Safety
/// `handle` must be a valid pointer from `recorder_create`.
#[no_mangle]
pub unsafe extern "C" fn recorder_stop(handle: *mut RecorderHandle) -> u32 {
    // SAFETY: caller guarantees handle is valid
    let h = unsafe { &*handle };
    h.inner.lock().unwrap_or_else(|e| e.into_inner()).stop() as u32
}

/// Record output data.
///
/// # Safety
/// `handle` must be valid. `data` must point to `len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn recorder_record_output(
    handle: *mut RecorderHandle,
    data: *const u8,
    len: usize,
) {
    // SAFETY: caller guarantees handle and data are valid
    let h = unsafe { &*handle };
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    h.inner.lock().unwrap_or_else(|e| e.into_inner()).record_output(bytes);
}

/// Record input data.
///
/// # Safety
/// `handle` must be valid. `data` must point to `len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn recorder_record_input(
    handle: *mut RecorderHandle,
    data: *const u8,
    len: usize,
) {
    // SAFETY: caller guarantees handle and data are valid
    let h = unsafe { &*handle };
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    h.inner.lock().unwrap_or_else(|e| e.into_inner()).record_input(bytes);
}

/// Export recording to asciinema format.
///
/// # Safety
/// `handle` must be valid. `path` must be a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn recorder_export(
    handle: *mut RecorderHandle,
    path: *const std::ffi::c_char,
) -> i32 {
    // SAFETY: caller guarantees handle and path are valid
    let h = unsafe { &*handle };
    let c_str = unsafe { std::ffi::CStr::from_ptr(path) };
    let path_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let path = PathBuf::from(path_str);
    match h.inner.lock().unwrap_or_else(|e| e.into_inner()).export_asciinema(&path) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

/// Destroy the recorder handle.
///
/// # Safety
/// `handle` must be a valid pointer from `recorder_create`. Must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn recorder_destroy(handle: *mut RecorderHandle) {
    if !handle.is_null() {
        // SAFETY: caller guarantees handle is valid and will not be reused
        let _ = unsafe { Box::from_raw(handle) };
    }
}
