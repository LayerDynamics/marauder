use std::sync::Mutex;

use crate::grid::Grid;
use marauder_parser::TerminalAction;

/// Opaque handle for FFI consumers.
pub struct GridHandle {
    grid: Mutex<Grid>,
}

/// Create a new grid. Returns an opaque handle.
///
/// # Safety
/// Caller must eventually call `grid_destroy` to free the handle.
#[no_mangle]
pub extern "C" fn grid_create(rows: u16, cols: u16) -> *mut GridHandle {
    if rows == 0 || cols == 0 {
        return std::ptr::null_mut();
    }
    let handle = Box::new(GridHandle {
        grid: Mutex::new(Grid::new(rows as usize, cols as usize)),
    });
    Box::into_raw(handle)
}

/// Apply a terminal action to the grid (from parser output).
///
/// `action_json` is a JSON-serialized `TerminalAction`.
/// Returns 1 on success, 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `grid_create`.
/// - `action_json` must point to `action_json_len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn grid_apply_action(
    handle: *mut GridHandle,
    action_json: *const u8,
    action_json_len: usize,
) -> i32 {
    if handle.is_null() || action_json.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };
    let json = unsafe { std::slice::from_raw_parts(action_json, action_json_len) };

    let action: TerminalAction = match serde_json::from_slice(json) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(error = %e, "grid_apply_action: failed to deserialize action");
            return 0;
        }
    };

    let mut grid = handle.grid.lock().unwrap_or_else(|e| e.into_inner());
    grid.apply_action(&action);
    1
}

/// Get a cell's data as JSON. Returns bytes written to `out_buf`, or 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `grid_create`.
/// - `out_buf` must point to at least `out_buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn grid_get_cell(
    handle: *mut GridHandle,
    row: usize,
    col: usize,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> usize {
    if handle.is_null() || out_buf.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };
    let grid = handle.grid.lock().unwrap_or_else(|e| e.into_inner());

    let screen = grid.active_screen();
    if row >= screen.rows.len() || col >= screen.cols {
        return 0;
    }

    let cell = &screen.rows[row][col];
    let json = match serde_json::to_vec(cell) {
        Ok(j) => j,
        Err(_) => return 0,
    };

    if json.len() > out_buf_len {
        return 0;
    }

    let out = unsafe { std::slice::from_raw_parts_mut(out_buf, json.len()) };
    out.copy_from_slice(&json);
    json.len()
}

/// Get cursor position. Returns row in high 32 bits, col in low 32 bits.
///
/// # Safety
/// - `handle` must be a valid pointer from `grid_create`.
#[no_mangle]
pub unsafe extern "C" fn grid_get_cursor(handle: *mut GridHandle) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };
    let grid = handle.grid.lock().unwrap_or_else(|e| e.into_inner());
    let cursor = &grid.cursor;
    ((cursor.row as u64) << 32) | (cursor.col as u64)
}

/// Resize the grid. Returns 1 on success, 0 on error.
///
/// # Safety
/// - `handle` must be a valid pointer from `grid_create`.
#[no_mangle]
pub unsafe extern "C" fn grid_resize(
    handle: *mut GridHandle,
    rows: u16,
    cols: u16,
) -> i32 {
    if handle.is_null() || rows == 0 || cols == 0 {
        return 0;
    }
    let handle = unsafe { &*handle };
    let mut grid = handle.grid.lock().unwrap_or_else(|e| e.into_inner());
    grid.resize(rows as usize, cols as usize);
    1
}

/// Set the viewport offset (0 = bottom/live, N = scrolled N lines up into scrollback).
///
/// # Safety
/// - `handle` must be a valid pointer from `grid_create`.
#[no_mangle]
pub unsafe extern "C" fn grid_scroll_viewport(handle: *mut GridHandle, offset: u32) {
    if handle.is_null() {
        return;
    }
    let handle = unsafe { &*handle };
    let mut grid = handle.grid.lock().unwrap_or_else(|e| e.into_inner());
    grid.scroll_viewport(offset as usize);
}

/// Set selection range. Pass start_row == end_row == u32::MAX to clear.
///
/// # Safety
/// - `handle` must be a valid pointer from `grid_create`.
#[no_mangle]
pub unsafe extern "C" fn grid_select(
    handle: *mut GridHandle,
    start_row: u32,
    start_col: u32,
    end_row: u32,
    end_col: u32,
) {
    if handle.is_null() {
        return;
    }
    let handle = unsafe { &*handle };
    let mut grid = handle.grid.lock().unwrap_or_else(|e| e.into_inner());

    if start_row == u32::MAX && end_row == u32::MAX {
        grid.clear_selection();
    } else {
        grid.set_selection(
            start_row as usize,
            start_col as usize,
            end_row as usize,
            end_col as usize,
        );
    }
}

/// Get selection text. Writes UTF-8 to `out_buf`. Returns bytes written, 0 if no selection.
///
/// # Safety
/// - `handle` must be a valid pointer from `grid_create`.
/// - `out_buf` must point to at least `out_buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn grid_get_selection_text(
    handle: *mut GridHandle,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> usize {
    if handle.is_null() || out_buf.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };
    let grid = handle.grid.lock().unwrap_or_else(|e| e.into_inner());

    let text = match grid.get_selection_text() {
        Some(t) => t,
        None => return 0,
    };
    let bytes = text.as_bytes();
    if bytes.len() > out_buf_len {
        return 0;
    }

    let out = unsafe { std::slice::from_raw_parts_mut(out_buf, bytes.len()) };
    out.copy_from_slice(bytes);
    bytes.len()
}

/// Destroy a grid handle, freeing its memory.
///
/// # Safety
/// - `handle` must be a valid pointer from `grid_create`, or null (no-op).
/// - Must not be called more than once for the same handle.
#[no_mangle]
pub unsafe extern "C" fn grid_destroy(handle: *mut GridHandle) {
    if !handle.is_null() {
        // SAFETY: handle is valid and not previously freed per caller contract
        let _ = unsafe { Box::from_raw(handle) };
    }
}
