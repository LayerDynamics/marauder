use std::sync::Mutex;

use marauder_event_bus::lock_or_log;

use crate::engine::ComputeEngine;
use crate::types::*;
use marauder_grid::ffi::GridHandle;

/// Opaque handle for FFI consumers.
pub struct ComputeHandle {
    engine: Mutex<ComputeEngine>,
}

/// Create a standalone compute engine (creates its own wgpu device).
/// Returns null on failure.
///
/// # Safety
/// Caller must eventually call `compute_destroy` to free the handle.
#[no_mangle]
pub extern "C" fn compute_create() -> *mut ComputeHandle {
    match ComputeEngine::new_standalone() {
        Ok(engine) => Box::into_raw(Box::new(ComputeHandle {
            engine: Mutex::new(engine),
        })),
        Err(e) => {
            tracing::error!(error = %e, "compute_create: failed to create standalone engine");
            std::ptr::null_mut()
        }
    }
}

/// Create a compute engine sharing the renderer's device and queue.
///
/// # Safety
/// - `device_ptr` must be a valid pointer to an `Arc<wgpu::Device>` (not a raw Device).
/// - `queue_ptr` must be a valid pointer to an `Arc<wgpu::Queue>` (not a raw Queue).
/// - Both pointees must outlive the returned handle.
/// - Caller must eventually call `compute_destroy` to free the handle.
#[no_mangle]
pub unsafe extern "C" fn compute_create_shared(
    device_ptr: *const std::ffi::c_void,
    queue_ptr: *const std::ffi::c_void,
) -> *mut ComputeHandle {
    if device_ptr.is_null() || queue_ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: Caller guarantees pointers point to valid Arc<Device> / Arc<Queue> instances.
    let engine = unsafe {
        ComputeEngine::new_borrowed(
            device_ptr as *const std::sync::Arc<wgpu::Device>,
            queue_ptr as *const std::sync::Arc<wgpu::Queue>,
        )
    };
    Box::into_raw(Box::new(ComputeHandle {
        engine: Mutex::new(engine),
    }))
}

/// Upload cell data from a JSON array of GpuCell objects.
/// Returns 1 on success, 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `compute_create`.
/// - `json_ptr` must point to `json_len` valid bytes of JSON.
#[no_mangle]
pub unsafe extern "C" fn compute_upload_cells(
    handle: *mut ComputeHandle,
    json_ptr: *const u8,
    json_len: usize,
    rows: u32,
    cols: u32,
) -> i32 {
    if handle.is_null() || json_ptr.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };
    let json = unsafe { std::slice::from_raw_parts(json_ptr, json_len) };

    let cells: Vec<GpuCell> = match serde_json::from_slice(json) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "compute_upload_cells: JSON parse error");
            return 0;
        }
    };

    let mut engine = lock_or_log(&handle.engine, "compute::ffi");
    engine.upload_cells_raw(&cells, rows, cols);
    1
}

/// Upload cell data directly from a Grid handle.
/// Returns 1 on success, 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `compute_create`.
/// - `grid_handle` must be a valid pointer from `grid_create`.
#[no_mangle]
pub unsafe extern "C" fn compute_upload_from_grid(
    handle: *mut ComputeHandle,
    grid_handle: *const GridHandle,
) -> i32 {
    if handle.is_null() || grid_handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };
    let grid_handle = unsafe { &*grid_handle };

    let mut engine = lock_or_log(&handle.engine, "compute::ffi");
    grid_handle.with_grid(|grid| engine.upload_cells(grid));
    1
}

/// Search for a pattern. Writes JSON results to `out_buf`. Returns bytes written, 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `compute_create`.
/// - `pattern_ptr` must point to `pattern_len` valid UTF-8 bytes.
/// - `out_buf` must point to `out_buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn compute_search(
    handle: *mut ComputeHandle,
    pattern_ptr: *const u8,
    pattern_len: usize,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> usize {
    if handle.is_null() || pattern_ptr.is_null() || out_buf.is_null() {
        return INTERNAL_ERROR;
    }
    let handle = unsafe { &*handle };
    let pattern_bytes = unsafe { std::slice::from_raw_parts(pattern_ptr, pattern_len) };
    let pattern = match std::str::from_utf8(pattern_bytes) {
        Ok(s) => s,
        Err(_) => return INTERNAL_ERROR,
    };

    let engine = lock_or_log(&handle.engine, "compute::ffi");
    let results = match engine.search(pattern) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "compute_search failed");
            return INTERNAL_ERROR;
        }
    };

    write_json_to_buf(&results, out_buf, out_buf_len)
}

/// Detect URLs in row range. Writes JSON results to `out_buf`. Returns bytes written.
///
/// # Safety
/// - `handle` must be a valid pointer from `compute_create`.
/// - `out_buf` must point to `out_buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn compute_detect_urls(
    handle: *mut ComputeHandle,
    row_start: u32,
    row_end: u32,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> usize {
    if handle.is_null() || out_buf.is_null() {
        return INTERNAL_ERROR;
    }
    let handle = unsafe { &*handle };
    let engine = lock_or_log(&handle.engine, "compute::ffi");
    let results = match engine.detect_urls(row_start, row_end) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "compute_detect_urls failed");
            return INTERNAL_ERROR;
        }
    };
    write_json_to_buf(&results, out_buf, out_buf_len)
}

/// Classify cells for highlighting. Writes JSON results to `out_buf`. Returns bytes written.
///
/// # Safety
/// - `handle` must be a valid pointer from `compute_create`.
/// - `out_buf` must point to `out_buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn compute_highlight_cells(
    handle: *mut ComputeHandle,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> usize {
    if handle.is_null() || out_buf.is_null() {
        return INTERNAL_ERROR;
    }
    let handle = unsafe { &*handle };
    let engine = lock_or_log(&handle.engine, "compute::ffi");
    let results = match engine.highlight_cells() {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "compute_highlight_cells failed");
            return INTERNAL_ERROR;
        }
    };
    write_json_to_buf(&results, out_buf, out_buf_len)
}

/// Extract selection text. Writes UTF-8 to `out_buf`. Returns bytes written.
///
/// # Safety
/// - `handle` must be a valid pointer from `compute_create`.
/// - `out_buf` must point to `out_buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn compute_extract_selection(
    handle: *mut ComputeHandle,
    start_row: u32,
    start_col: u32,
    end_row: u32,
    end_col: u32,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> usize {
    if handle.is_null() || out_buf.is_null() {
        return INTERNAL_ERROR;
    }
    let handle = unsafe { &*handle };
    let engine = lock_or_log(&handle.engine, "compute::ffi");
    let text = match engine.extract_selection(start_row, start_col, end_row, end_col) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "compute_extract_selection failed");
            return INTERNAL_ERROR;
        }
    };

    let bytes = text.as_bytes();
    if bytes.len() > out_buf_len {
        tracing::warn!(
            needed = bytes.len(),
            available = out_buf_len,
            "compute_extract_selection: buffer too small, caller should retry"
        );
        return BUFFER_TOO_SMALL;
    }
    let out = unsafe { std::slice::from_raw_parts_mut(out_buf, bytes.len()) };
    out.copy_from_slice(bytes);
    bytes.len()
}

/// Destroy a compute handle, freeing its memory.
///
/// # Safety
/// - `handle` must be a valid pointer from `compute_create`, or null (no-op).
/// - Must not be called more than once for the same handle.
#[no_mangle]
pub unsafe extern "C" fn compute_destroy(handle: *mut ComputeHandle) {
    if !handle.is_null() {
        // SAFETY: handle is valid and not previously freed per caller contract
        let _ = unsafe { Box::from_raw(handle) };
    }
}

/// Sentinel return value indicating the output buffer was too small.
/// The TypeScript FFI layer must check for this and retry with a larger buffer.
const BUFFER_TOO_SMALL: usize = usize::MAX;

/// Sentinel return value indicating an internal error (UTF-8, GPU, etc.).
/// Distinct from 0 (which means "no results") and BUFFER_TOO_SMALL.
const INTERNAL_ERROR: usize = usize::MAX - 1;

/// Helper: serialize to JSON and write to output buffer.
/// Returns bytes written on success, 0 on serialization error,
/// or `BUFFER_TOO_SMALL` (`usize::MAX`) if the buffer is too small.
unsafe fn write_json_to_buf<T: serde::Serialize>(
    value: &T,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> usize {
    let json = match serde_json::to_vec(value) {
        Ok(j) => j,
        Err(_) => return INTERNAL_ERROR,
    };
    if json.len() > out_buf_len {
        tracing::warn!(
            needed = json.len(),
            available = out_buf_len,
            "write_json_to_buf: buffer too small, caller should retry"
        );
        return BUFFER_TOO_SMALL;
    }
    let out = unsafe { std::slice::from_raw_parts_mut(out_buf, json.len()) };
    out.copy_from_slice(&json);
    json.len()
}
