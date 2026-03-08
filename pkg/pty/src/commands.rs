//! Tauri command wrappers for PTY operations.
//!
//! These commands bridge the webview to the PTY manager via Tauri's IPC.
//! The PtyManager is stored as Tauri managed state behind a Mutex.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::manager::{PaneId, PtyConfig, PtyManager};
use crate::pty;

/// Shared PTY manager type (same as runtime uses internally).
pub type SharedPtyManager = Arc<Mutex<PtyManager>>;

/// Tauri managed state wrapping the PtyManager.
///
/// Uses late-injection pattern: starts empty and receives the runtime's
/// real PTY manager after boot via `inject()`. This ensures Tauri commands
/// operate on the same PTY sessions as the runtime pipeline.
pub struct TauriPtyManager {
    pub inner: RwLock<Option<SharedPtyManager>>,
}

impl TauriPtyManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }

    /// Inject the runtime's real PTY manager after boot.
    pub fn inject(&self, mgr: SharedPtyManager) {
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = Some(mgr);
    }
}

impl Default for TauriPtyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to run a closure with the shared PTY manager.
fn with_pty_manager<F, T>(state: &TauriPtyManager, f: F) -> Result<T, String>
where
    F: FnOnce(&mut PtyManager) -> Result<T, String>,
{
    let guard = state.inner.read().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(mgr) => {
            let mut mgr = mgr.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut mgr)
        }
        None => Err("PTY manager not initialized yet (runtime still booting)".into()),
    }
}

/// Request to create a PTY session.
#[derive(Debug, Deserialize)]
pub struct CreatePtyRequest {
    pub shell: Option<String>,
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub rows: u16,
    pub cols: u16,
}

/// Info about a PTY session returned to the webview.
#[derive(Debug, Serialize)]
pub struct PtyInfo {
    pub pane_id: PaneId,
    pub pid: Option<u32>,
    pub shell: String,
    pub rows: u16,
    pub cols: u16,
}

#[tauri::command]
pub fn pty_cmd_create(
    state: tauri::State<'_, TauriPtyManager>,
    request: CreatePtyRequest,
) -> Result<PtyInfo, String> {
    with_pty_manager(&state, |mgr| {
        let config = PtyConfig {
            shell: request.shell.clone().unwrap_or_else(pty::default_shell),
            env: request.env.clone().unwrap_or_default(),
            cwd: request.cwd.as_ref().map(|s| PathBuf::from(s)).unwrap_or_else(|| {
                std::env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
            }),
            rows: request.rows,
            cols: request.cols,
        };

        let shell = config.shell.clone();
        let id = mgr.create(config).map_err(|e| e.to_string())?;
        let pid = mgr.get_pid(id).ok().flatten();

        Ok(PtyInfo {
            pane_id: id,
            pid,
            shell,
            rows: request.rows,
            cols: request.cols,
        })
    })
}

#[tauri::command]
pub fn pty_cmd_write(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
    data: Vec<u8>,
) -> Result<usize, String> {
    tracing::debug!(
        pane_id,
        bytes = data.len(),
        preview = %String::from_utf8_lossy(&data[..data.len().min(64)]),
        "pty_cmd_write: keyboard input → PTY"
    );
    with_pty_manager(&state, |mgr| {
        mgr.write(pane_id, &data).map_err(|e| {
            tracing::error!(pane_id, error = %e, "pty_cmd_write failed");
            e.to_string()
        })
    })
}

/// Maximum allowed read buffer size (64 KiB) to prevent OOM from malicious requests.
const MAX_READ_BYTES: usize = 65_536;

#[tauri::command]
pub fn pty_cmd_read(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let capped = max_bytes.min(MAX_READ_BYTES);
    with_pty_manager(&state, |mgr| {
        let mut buf = vec![0u8; capped];
        let n = mgr.read(pane_id, &mut buf).map_err(|e| e.to_string())?;
        buf.truncate(n);
        Ok(buf)
    })
}

#[tauri::command]
pub fn pty_cmd_resize(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    with_pty_manager(&state, |mgr| {
        mgr.resize(pane_id, rows, cols).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn pty_cmd_close(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
) -> Result<(), String> {
    with_pty_manager(&state, |mgr| {
        mgr.close(pane_id).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn pty_cmd_get_pid(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
) -> Result<Option<u32>, String> {
    with_pty_manager(&state, |mgr| {
        mgr.get_pid(pane_id).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn pty_cmd_wait(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
) -> Result<bool, String> {
    with_pty_manager(&state, |mgr| {
        let status = mgr.try_wait(pane_id).map_err(|e| e.to_string())?;
        Ok(status.is_some())
    })
}

#[tauri::command]
pub fn pty_cmd_list(
    state: tauri::State<'_, TauriPtyManager>,
) -> Result<Vec<PaneId>, String> {
    with_pty_manager(&state, |mgr| {
        Ok(mgr.list())
    })
}
