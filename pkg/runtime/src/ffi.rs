//! C ABI exports for the runtime.
//!
//! # Thread Safety
//! `RuntimeHandle` wraps a `Mutex<MarauderRuntime>`. All FFI functions lock
//! the mutex before accessing the runtime. The tokio runtime is created
//! per-handle for async operations.
//!
//! # Pane ID Convention
//! Pane IDs are always > 0. The value 0 is used as an error sentinel in
//! `runtime_create_pane`. This is guaranteed by `PtyManager` which starts
//! its counter at 1.

use std::sync::Mutex;

use crate::config::RuntimeConfig;
use crate::lifecycle::MarauderRuntime;

/// Helper to lock a mutex, logging a warning if it was poisoned.
fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, label: &str) -> std::sync::MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|e| {
        tracing::warn!("{label} mutex was poisoned, recovering");
        e.into_inner()
    })
}

/// Opaque FFI handle wrapping the runtime + a tokio runtime for async ops.
pub struct RuntimeHandle {
    runtime: Mutex<MarauderRuntime>,
    tokio_rt: tokio::runtime::Runtime,
}

/// Create a new runtime with default config. Returns an opaque handle.
///
/// # Safety
/// Caller must eventually call `runtime_destroy` to free the handle.
#[no_mangle]
pub extern "C" fn runtime_create() -> *mut RuntimeHandle {
    let tokio_rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return std::ptr::null_mut(),
    };
    let handle = Box::new(RuntimeHandle {
        runtime: Mutex::new(MarauderRuntime::new(RuntimeConfig::default())),
        tokio_rt,
    });
    Box::into_raw(handle)
}

/// Boot the runtime (async — blocks until boot completes).
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid pointer from `runtime_create`.
///
/// # Deadlock Prevention
/// The runtime mutex is acquired inside the `block_on` future (not before)
/// to prevent deadlocks if async code spawned by boot needs the mutex.
#[no_mangle]
pub unsafe extern "C" fn runtime_boot(handle: *mut RuntimeHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let handle = unsafe { &*handle };
    // Acquire mutex INSIDE block_on to prevent deadlock if boot() spawns
    // tasks that need the runtime mutex.
    match handle.tokio_rt.block_on(async {
        let mut rt = lock_or_recover(&handle.runtime, "runtime");
        rt.boot().await
    }) {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!(error = %e, "runtime_boot failed");
            -1
        }
    }
}

/// Create a new pane. Returns the pane ID (always > 0), or 0 on error.
///
/// # Safety
/// `handle` must be a valid pointer from `runtime_create`.
#[no_mangle]
pub unsafe extern "C" fn runtime_create_pane(handle: *mut RuntimeHandle) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };
    let mut rt = lock_or_recover(&handle.runtime, "runtime");
    match rt.create_pane() {
        Ok(id) => {
            debug_assert!(id > 0, "PtyManager should never assign pane_id 0");
            id
        }
        Err(e) => {
            tracing::error!(error = %e, "runtime_create_pane failed");
            0
        }
    }
}

/// Close a pane. Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid pointer from `runtime_create`.
#[no_mangle]
pub unsafe extern "C" fn runtime_close_pane(handle: *mut RuntimeHandle, pane_id: u64) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let handle = unsafe { &*handle };
    let mut rt = lock_or_recover(&handle.runtime, "runtime");
    match rt.close_pane(pane_id) {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!(error = %e, "runtime_close_pane failed");
            -1
        }
    }
}

/// Write data to a pane's PTY. Returns bytes written, or -1 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `runtime_create`.
/// - `data` must point to `data_len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn runtime_write(
    handle: *mut RuntimeHandle,
    pane_id: u64,
    data: *const u8,
    data_len: usize,
) -> i32 {
    if handle.is_null() || data.is_null() {
        return -1;
    }
    let handle = unsafe { &*handle };
    let bytes = unsafe { std::slice::from_raw_parts(data, data_len) };
    let rt = lock_or_recover(&handle.runtime, "runtime");
    match rt.write_to_pane(pane_id, bytes) {
        Ok(n) => n.min(i32::MAX as usize) as i32,
        Err(e) => {
            tracing::error!(error = %e, "runtime_write failed");
            -1
        }
    }
}

/// Resize a pane. Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid pointer from `runtime_create`.
#[no_mangle]
pub unsafe extern "C" fn runtime_resize_pane(
    handle: *mut RuntimeHandle,
    pane_id: u64,
    rows: u16,
    cols: u16,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let handle = unsafe { &*handle };
    let mut rt = lock_or_recover(&handle.runtime, "runtime");
    match rt.resize_pane(pane_id, rows, cols) {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!(error = %e, "runtime_resize_pane failed");
            -1
        }
    }
}

/// Get the number of active panes.
///
/// # Safety
/// `handle` must be a valid pointer from `runtime_create`.
#[no_mangle]
pub unsafe extern "C" fn runtime_pane_count(handle: *mut RuntimeHandle) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };
    let rt = lock_or_recover(&handle.runtime, "runtime");
    rt.pane_ids().len() as u32
}

/// Shutdown the runtime (async — blocks until shutdown completes).
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid pointer from `runtime_create`.
///
/// # Deadlock Prevention
/// The runtime mutex is acquired inside the `block_on` future (not before).
#[no_mangle]
pub unsafe extern "C" fn runtime_shutdown(handle: *mut RuntimeHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let handle = unsafe { &*handle };
    match handle.tokio_rt.block_on(async {
        let mut rt = lock_or_recover(&handle.runtime, "runtime");
        rt.shutdown().await
    }) {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!(error = %e, "runtime_shutdown failed");
            -1
        }
    }
}

/// Destroy a runtime handle, freeing its memory.
///
/// Callers should call `runtime_shutdown` before this. If the runtime is
/// still running, the `Drop` impl will perform synchronous cleanup.
///
/// # Safety
/// - `handle` must be a valid pointer from `runtime_create`, or null (no-op).
/// - Must not be called more than once for the same handle.
#[no_mangle]
pub unsafe extern "C" fn runtime_destroy(handle: *mut RuntimeHandle) {
    if !handle.is_null() {
        // SAFETY: handle is valid and not previously freed per caller contract
        let _ = unsafe { Box::from_raw(handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_destroy() {
        let handle = runtime_create();
        assert!(!handle.is_null());
        unsafe { runtime_destroy(handle) };
    }

    #[test]
    fn test_destroy_null_is_noop() {
        unsafe { runtime_destroy(std::ptr::null_mut()) };
    }

    #[test]
    fn test_boot_null_returns_error() {
        let result = unsafe { runtime_boot(std::ptr::null_mut()) };
        assert_eq!(result, -1);
    }

    #[test]
    fn test_create_pane_null_returns_zero() {
        let result = unsafe { runtime_create_pane(std::ptr::null_mut()) };
        assert_eq!(result, 0);
    }

    #[test]
    fn test_write_null_handle_returns_error() {
        let data = b"test";
        let result = unsafe { runtime_write(std::ptr::null_mut(), 1, data.as_ptr(), data.len()) };
        assert_eq!(result, -1);
    }

    #[test]
    fn test_write_null_data_returns_error() {
        let handle = runtime_create();
        let result = unsafe { runtime_write(handle, 1, std::ptr::null(), 0) };
        assert_eq!(result, -1);
        unsafe { runtime_destroy(handle) };
    }

    #[test]
    fn test_boot_shutdown_lifecycle() {
        let handle = runtime_create();
        assert!(!handle.is_null());
        let boot_result = unsafe { runtime_boot(handle) };
        assert_eq!(boot_result, 0);
        let shutdown_result = unsafe { runtime_shutdown(handle) };
        assert_eq!(shutdown_result, 0);
        unsafe { runtime_destroy(handle) };
    }

    #[test]
    fn test_pane_count_null_returns_zero() {
        let result = unsafe { runtime_pane_count(std::ptr::null_mut()) };
        assert_eq!(result, 0);
    }
}
