use std::collections::HashMap;
use std::ffi::{c_char, CStr};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::manager::{PtyConfig, PtyManager};
use crate::pty;

/// Opaque handle for FFI consumers. Mutex protects against concurrent FFI calls.
pub struct PtyManagerHandle {
    manager: Mutex<PtyManager>,
}

/// Create a new PtyManager. Returns an opaque handle.
///
/// # Safety
/// Caller must eventually call `pty_manager_destroy` to free the handle.
#[no_mangle]
pub extern "C" fn pty_manager_create() -> *mut PtyManagerHandle {
    let handle = Box::new(PtyManagerHandle {
        manager: Mutex::new(PtyManager::new()),
    });
    Box::into_raw(handle)
}

/// Create a new PTY session. Returns the PaneId (>0) on success, 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `pty_manager_create`.
/// - `shell` must be a valid null-terminated C string, or null for default.
/// - `cwd` must be a valid null-terminated C string, or null for default.
#[no_mangle]
pub unsafe extern "C" fn pty_create(
    handle: *mut PtyManagerHandle,
    shell: *const c_char,
    cwd: *const c_char,
    rows: u16,
    cols: u16,
) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };

    if rows == 0 || cols == 0 {
        tracing::error!("pty_create: rows and cols must be > 0");
        return 0;
    }

    let shell = if shell.is_null() {
        pty::default_shell()
    } else {
        unsafe { CStr::from_ptr(shell) }
            .to_str()
            .unwrap_or("/bin/sh")
            .to_string()
    };

    let cwd = if cwd.is_null() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    } else {
        PathBuf::from(
            unsafe { CStr::from_ptr(cwd) }
                .to_str()
                .unwrap_or("/"),
        )
    };

    let config = PtyConfig {
        shell,
        env: HashMap::new(),
        cwd,
        rows,
        cols,
    };

    let mut mgr = handle.manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.create(config) {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create PTY session");
            0
        }
    }
}

/// Read data from a PTY session. Returns bytes read, or -1 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `pty_manager_create`.
/// - `buf` must point to at least `buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn pty_read(
    handle: *mut PtyManagerHandle,
    pane_id: u64,
    buf: *mut u8,
    buf_len: usize,
) -> i64 {
    if handle.is_null() || buf.is_null() || buf_len == 0 {
        return -1;
    }
    let handle = unsafe { &*handle };
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, buf_len) };

    let mut mgr = handle.manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.read(pane_id, slice) {
        Ok(n) => n as i64,
        Err(e) => {
            tracing::error!(pane_id, error = %e, "pty_read failed");
            -1
        }
    }
}

/// Write data to a PTY session. Returns bytes written, or -1 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `pty_manager_create`.
/// - `data` must point to `data_len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn pty_write(
    handle: *mut PtyManagerHandle,
    pane_id: u64,
    data: *const u8,
    data_len: usize,
) -> i64 {
    if handle.is_null() || data.is_null() {
        return -1;
    }
    let handle = unsafe { &*handle };
    let slice = unsafe { std::slice::from_raw_parts(data, data_len) };

    let mut mgr = handle.manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.write(pane_id, slice) {
        Ok(n) => n as i64,
        Err(e) => {
            tracing::error!(pane_id, error = %e, "pty_write failed");
            -1
        }
    }
}

/// Resize a PTY session. Returns 1 on success, 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `pty_manager_create`.
#[no_mangle]
pub unsafe extern "C" fn pty_resize(
    handle: *mut PtyManagerHandle,
    pane_id: u64,
    rows: u16,
    cols: u16,
) -> i32 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };

    let mut mgr = handle.manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.resize(pane_id, rows, cols) {
        Ok(()) => 1,
        Err(e) => {
            tracing::error!(pane_id, error = %e, "pty_resize failed");
            0
        }
    }
}

/// Close and remove a PTY session. Returns 1 on success, 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `pty_manager_create`.
#[no_mangle]
pub unsafe extern "C" fn pty_close(
    handle: *mut PtyManagerHandle,
    pane_id: u64,
) -> i32 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };

    let mut mgr = handle.manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.close(pane_id) {
        Ok(()) => 1,
        Err(e) => {
            tracing::error!(pane_id, error = %e, "pty_close failed");
            0
        }
    }
}

/// Get the child process ID for a PTY session. Returns PID or 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `pty_manager_create`.
#[no_mangle]
pub unsafe extern "C" fn pty_get_pid(
    handle: *mut PtyManagerHandle,
    pane_id: u64,
) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };

    let mgr = handle.manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.get_pid(pane_id) {
        Ok(Some(pid)) => pid,
        _ => 0,
    }
}

/// Check if a child process has exited. Returns 1 if exited, 0 if still running, -1 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `pty_manager_create`.
#[no_mangle]
pub unsafe extern "C" fn pty_wait(
    handle: *mut PtyManagerHandle,
    pane_id: u64,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let handle = unsafe { &*handle };

    let mut mgr = handle.manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.try_wait(pane_id) {
        Ok(Some(_)) => 1,
        Ok(None) => 0,
        Err(_) => -1,
    }
}

/// Get the number of active PTY sessions.
///
/// # Safety
/// - `handle` must be a valid pointer from `pty_manager_create`.
#[no_mangle]
pub unsafe extern "C" fn pty_count(handle: *mut PtyManagerHandle) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };
    let mgr = handle.manager.lock().unwrap_or_else(|e| e.into_inner());
    mgr.count() as u64
}

/// Destroy a PtyManager handle, killing all child processes and freeing memory.
///
/// # Safety
/// - `handle` must be a valid pointer from `pty_manager_create`, or null (no-op).
/// - Must not be called more than once for the same handle.
#[no_mangle]
pub unsafe extern "C" fn pty_manager_destroy(handle: *mut PtyManagerHandle) {
    if !handle.is_null() {
        // SAFETY: handle is valid and not previously freed per caller contract
        let boxed = unsafe { Box::from_raw(handle) };
        // close_all is called by PtyManager's Drop impl, which kills all children
        drop(boxed);
    }
}
