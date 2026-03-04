//! High-level deno_bindgen bindings for PTY management.

use deno_bindgen::deno_bindgen;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::manager::{PtyConfig, PtyManager};
use crate::pty;

static HANDLES: OnceLock<Mutex<HashMap<u32, Arc<Mutex<PtyManager>>>>> = OnceLock::new();
static NEXT_ID: OnceLock<Mutex<u32>> = OnceLock::new();

fn handles() -> &'static Mutex<HashMap<u32, Arc<Mutex<PtyManager>>>> {
    HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u32 {
    let mut id = NEXT_ID.get_or_init(|| Mutex::new(1)).lock().unwrap_or_else(|e| e.into_inner());
    let val = *id;
    match val.checked_add(1) {
        Some(next) => { *id = next; val }
        None => {
            tracing::error!("bindgen handle ID counter overflow");
            0
        }
    }
}

fn get_mgr(handle_id: u32) -> Option<Arc<Mutex<PtyManager>>> {
    handles().lock().unwrap_or_else(|e| e.into_inner()).get(&handle_id).cloned()
}

/// Create a new PtyManager. Returns a handle ID.
#[deno_bindgen]
fn pty_bindgen_create() -> u32 {
    let id = next_id();
    handles().lock().unwrap_or_else(|e| e.into_inner()).insert(id, Arc::new(Mutex::new(PtyManager::new())));
    id
}

/// Create a PTY session. Returns pane ID (>0) on success, 0 on error.
#[deno_bindgen]
fn pty_bindgen_open(handle_id: u32, shell: &str, cwd: &str, rows: u16, cols: u16) -> u64 {
    let mgr = match get_mgr(handle_id) {
        Some(m) => m,
        None => return 0,
    };
    if rows == 0 || cols == 0 { return 0; }
    let shell = if shell.is_empty() { pty::default_shell() } else { shell.to_string() };
    let cwd = if cwd.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    } else {
        PathBuf::from(cwd)
    };
    let config = PtyConfig { shell, env: HashMap::new(), cwd, rows, cols };
    let mut mgr = mgr.lock().unwrap_or_else(|e| e.into_inner());
    mgr.create(config).unwrap_or(0)
}

/// Write string data to a PTY. Returns bytes written, or -1 on error.
#[deno_bindgen]
fn pty_bindgen_write(handle_id: u32, pane_id: u64, data: &str) -> i32 {
    let mgr = match get_mgr(handle_id) {
        Some(m) => m,
        None => return -1,
    };
    let mut mgr = mgr.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.write(pane_id, data.as_bytes()) {
        Ok(n) => n as i32,
        Err(_) => -1,
    }
}

/// Resize a PTY. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn pty_bindgen_resize(handle_id: u32, pane_id: u64, rows: u16, cols: u16) -> u8 {
    let mgr = match get_mgr(handle_id) {
        Some(m) => m,
        None => return 0,
    };
    let mut mgr = mgr.lock().unwrap_or_else(|e| e.into_inner());
    if mgr.resize(pane_id, rows, cols).is_ok() { 1 } else { 0 }
}

/// Close a PTY session. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn pty_bindgen_close(handle_id: u32, pane_id: u64) -> u8 {
    let mgr = match get_mgr(handle_id) {
        Some(m) => m,
        None => return 0,
    };
    let mut mgr = mgr.lock().unwrap_or_else(|e| e.into_inner());
    if mgr.close(pane_id).is_ok() { 1 } else { 0 }
}

/// Get child PID. Returns 0 on error.
#[deno_bindgen]
fn pty_bindgen_get_pid(handle_id: u32, pane_id: u64) -> u32 {
    let mgr = match get_mgr(handle_id) {
        Some(m) => m,
        None => return 0,
    };
    let mgr = mgr.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.get_pid(pane_id) {
        Ok(Some(pid)) => pid,
        _ => 0,
    }
}

/// Destroy a PtyManager handle.
#[deno_bindgen]
fn pty_bindgen_destroy(handle_id: u32) {
    handles().lock().unwrap_or_else(|e| e.into_inner()).remove(&handle_id);
}
