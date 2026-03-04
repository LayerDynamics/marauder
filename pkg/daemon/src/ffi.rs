//! C ABI exports for the daemon crate.
//!
//! Provides opaque handle-based API for creating, starting, shutting down,
//! and destroying a `MarauderDaemon` instance from FFI consumers.

use crate::daemon::MarauderDaemon;

/// Opaque handle wrapping a `MarauderDaemon` and its tokio runtime.
pub struct DaemonHandle {
    daemon: Option<MarauderDaemon>,
    rt: tokio::runtime::Runtime,
}

/// Create a new daemon handle.
///
/// Returns a pointer to the handle, or null on failure.
///
/// # Safety
/// The caller must eventually call `daemon_destroy` to free the handle.
#[no_mangle]
pub extern "C" fn daemon_create() -> *mut DaemonHandle {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return std::ptr::null_mut(),
    };
    let daemon = MarauderDaemon::new();
    let handle = Box::new(DaemonHandle {
        daemon: Some(daemon),
        rt,
    });
    Box::into_raw(handle)
}

/// Start the daemon (binds IPC server).
///
/// Returns 0 on success, -1 on failure.
///
/// # Safety
/// `handle` must be a valid pointer returned by `daemon_create`.
#[no_mangle]
pub unsafe extern "C" fn daemon_start(handle: *mut DaemonHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }
    // SAFETY: caller guarantees handle is valid
    let h = unsafe { &mut *handle };
    match &mut h.daemon {
        Some(daemon) => {
            match h.rt.block_on(daemon.start()) {
                Ok(()) => 0,
                Err(_) => -1,
            }
        }
        None => -1,
    }
}

/// Shut down the daemon.
///
/// Returns 0 on success, -1 on failure.
///
/// # Safety
/// `handle` must be a valid pointer returned by `daemon_create`.
#[no_mangle]
pub unsafe extern "C" fn daemon_shutdown(handle: *mut DaemonHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }
    // SAFETY: caller guarantees handle is valid
    let h = unsafe { &mut *handle };
    match h.daemon.take() {
        Some(daemon) => {
            h.rt.block_on(daemon.shutdown());
            0
        }
        None => -1,
    }
}

/// Destroy the daemon handle, freeing all resources.
///
/// # Safety
/// `handle` must be a valid pointer returned by `daemon_create`, and must not
/// be used after this call.
#[no_mangle]
pub unsafe extern "C" fn daemon_destroy(handle: *mut DaemonHandle) {
    if !handle.is_null() {
        // SAFETY: caller guarantees handle is valid and will not be used again
        let h = unsafe { Box::from_raw(handle) };
        // If daemon wasn't shut down yet, do it now
        if let Some(daemon) = h.daemon {
            h.rt.block_on(daemon.shutdown());
        }
        // rt and daemon dropped here
    }
}
