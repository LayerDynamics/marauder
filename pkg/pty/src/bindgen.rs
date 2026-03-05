//! High-level deno_bindgen bindings for PTY management.

use deno_bindgen::deno_bindgen;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use marauder_event_bus::lock_or_log;
use marauder_event_bus::HandleRegistry;

use crate::manager::{PtyConfig, PtyManager};
use crate::pty;

static REGISTRY: HandleRegistry<Arc<Mutex<PtyManager>>> = HandleRegistry::new();

fn get_mgr(handle_id: u32) -> Option<Arc<Mutex<PtyManager>>> {
    REGISTRY.get_clone(handle_id)
}

/// Create a new PtyManager. Returns a handle ID (0 on failure).
#[deno_bindgen]
fn pty_bindgen_create() -> u32 {
    REGISTRY.allocate(Arc::new(Mutex::new(PtyManager::new())))
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
    let mut mgr = lock_or_log(&mgr, "pty::bindgen_open");
    mgr.create(config).unwrap_or(0)
}

/// Write string data to a PTY. Returns bytes written, or -1 on error.
#[deno_bindgen]
fn pty_bindgen_write(handle_id: u32, pane_id: u64, data: &str) -> i32 {
    let mgr = match get_mgr(handle_id) {
        Some(m) => m,
        None => return -1,
    };
    let mut mgr = lock_or_log(&mgr, "pty::bindgen_write");
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
    let mut mgr = lock_or_log(&mgr, "pty::bindgen_resize");
    if mgr.resize(pane_id, rows, cols).is_ok() { 1 } else { 0 }
}

/// Close a PTY session. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn pty_bindgen_close(handle_id: u32, pane_id: u64) -> u8 {
    let mgr = match get_mgr(handle_id) {
        Some(m) => m,
        None => return 0,
    };
    let mut mgr = lock_or_log(&mgr, "pty::bindgen_close");
    if mgr.close(pane_id).is_ok() { 1 } else { 0 }
}

/// Get child PID. Returns 0 on error.
#[deno_bindgen]
fn pty_bindgen_get_pid(handle_id: u32, pane_id: u64) -> u32 {
    let mgr = match get_mgr(handle_id) {
        Some(m) => m,
        None => return 0,
    };
    let mgr = lock_or_log(&mgr, "pty::bindgen_get_pid");
    match mgr.get_pid(pane_id) {
        Ok(Some(pid)) => pid,
        _ => 0,
    }
}

/// Destroy a PtyManager handle.
#[deno_bindgen]
fn pty_bindgen_destroy(handle_id: u32) {
    REGISTRY.remove(handle_id);
}
