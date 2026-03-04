//! Tauri command wrappers for PTY operations.
//!
//! These commands bridge the webview to the PTY manager via Tauri's IPC.
//! The PtyManager is stored as Tauri managed state behind a Mutex.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::manager::{PaneId, PtyConfig, PtyManager};
use crate::pty;

/// Tauri managed state wrapping the PtyManager.
pub struct TauriPtyManager {
    pub inner: Mutex<PtyManager>,
}

impl TauriPtyManager {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(PtyManager::new()),
        }
    }

    pub fn with_manager(mgr: PtyManager) -> Self {
        Self {
            inner: Mutex::new(mgr),
        }
    }
}

impl Default for TauriPtyManager {
    fn default() -> Self {
        Self::new()
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
    let mut mgr = state.inner.lock().unwrap_or_else(|e| e.into_inner());

    let config = PtyConfig {
        shell: request.shell.unwrap_or_else(pty::default_shell),
        env: request.env.unwrap_or_default(),
        cwd: request.cwd.map(PathBuf::from).unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
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
}

#[tauri::command]
pub fn pty_cmd_write(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
    data: Vec<u8>,
) -> Result<usize, String> {
    let mut mgr = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    mgr.write(pane_id, &data).map_err(|e| e.to_string())
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
    let mut mgr = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    let mut buf = vec![0u8; capped];
    let n = mgr.read(pane_id, &mut buf).map_err(|e| e.to_string())?;
    buf.truncate(n);
    Ok(buf)
}

#[tauri::command]
pub fn pty_cmd_resize(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let mut mgr = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    mgr.resize(pane_id, rows, cols).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_cmd_close(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
) -> Result<(), String> {
    let mut mgr = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    mgr.close(pane_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_cmd_get_pid(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
) -> Result<Option<u32>, String> {
    let mgr = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    mgr.get_pid(pane_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_cmd_wait(
    state: tauri::State<'_, TauriPtyManager>,
    pane_id: PaneId,
) -> Result<bool, String> {
    let mut mgr = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    let status = mgr.try_wait(pane_id).map_err(|e| e.to_string())?;
    Ok(status.is_some())
}

#[tauri::command]
pub fn pty_cmd_list(
    state: tauri::State<'_, TauriPtyManager>,
) -> Result<Vec<PaneId>, String> {
    let mgr = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    Ok(mgr.list())
}
