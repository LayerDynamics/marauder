//! C ABI exports for the renderer, consumed by `ffi/renderer/mod.ts`.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use marauder_grid::ffi::GridHandle;

use crate::renderer::Renderer;
use crate::types::{CursorStyle, RendererConfig, ThemeColors};

/// Error code returned when the renderer mutex is poisoned (prior panic).
///
/// GPU state is likely corrupt after a panic — continuing would risk GPU hangs,
/// validation errors, or rendering garbage. Callers should treat this as fatal
/// and destroy + recreate the renderer.
const ERR_POISONED: i32 = -99;

/// Lock the renderer mutex, returning `ERR_POISONED` (via early return) if poisoned.
///
/// Usage: `let r = lock_or_err!(handle_ref, mutable);` or `let r = lock_or_err!(handle_ref);`
macro_rules! lock_or_err {
    ($h:expr, mutable) => {
        match $h.renderer.lock() {
            Ok(guard) => guard,
            Err(_) => {
                tracing::error!("renderer mutex poisoned — GPU state may be corrupt");
                return ERR_POISONED;
            }
        }
    };
    ($h:expr) => {
        match $h.renderer.lock() {
            Ok(guard) => guard,
            Err(_) => {
                tracing::error!("renderer mutex poisoned — GPU state may be corrupt");
                return ERR_POISONED;
            }
        }
    };
}

/// Variant for functions that return a pointer (null on poison).
macro_rules! lock_or_null {
    ($h:expr) => {
        match $h.renderer.lock() {
            Ok(guard) => guard,
            Err(_) => {
                tracing::error!("renderer mutex poisoned — GPU state may be corrupt");
                return std::ptr::null();
            }
        }
    };
}

/// Opaque handle for FFI consumers.
pub struct RendererHandle {
    renderer: Mutex<Renderer>,
}

// ---------------------------------------------------------------------------
// Create / Destroy
// ---------------------------------------------------------------------------

/// Create a headless renderer (no surface, no present).
///
/// `config_json` is a JSON-encoded `RendererConfig`. Pass null + 0 for defaults.
///
/// Returns null on failure.
///
/// # Safety
/// - If `config_json` is non-null, it must point to `config_json_len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn renderer_create(
    width: u32,
    height: u32,
    scale_factor: f32,
    config_json: *const u8,
    config_json_len: usize,
) -> *mut RendererHandle {
    let config = if config_json.is_null() || config_json_len == 0 {
        RendererConfig::default()
    } else {
        let slice = std::slice::from_raw_parts(config_json, config_json_len);
        match serde_json::from_slice::<RendererConfig>(slice) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("renderer_create: bad config JSON: {e}");
                return std::ptr::null_mut();
            }
        }
    };

    match Renderer::new_headless(width, height, scale_factor, config) {
        Ok(renderer) => Box::into_raw(Box::new(RendererHandle {
            renderer: Mutex::new(renderer),
        })),
        Err(e) => {
            tracing::error!("renderer_create: {e}");
            std::ptr::null_mut()
        }
    }
}

/// Destroy the renderer handle.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`, or null (no-op).
#[no_mangle]
pub unsafe extern "C" fn renderer_destroy(handle: *mut RendererHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}

// ---------------------------------------------------------------------------
// Font / Theme / Cursor
// ---------------------------------------------------------------------------

/// Set font family, size, and line height. Rebuilds the glyph atlas.
///
/// Pass null/0 for `family_ptr`/`family_len` to keep the current family.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
/// - If `family_ptr` is non-null, it must point to `family_len` valid UTF-8 bytes.
#[no_mangle]
pub unsafe extern "C" fn renderer_set_font(
    handle: *mut RendererHandle,
    family_ptr: *const u8,
    family_len: usize,
    size: f32,
    line_height: f32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let family = if family_ptr.is_null() || family_len == 0 {
        ""
    } else {
        let bytes = std::slice::from_raw_parts(family_ptr, family_len);
        match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("renderer_set_font: invalid UTF-8 family: {e}");
                return -1;
            }
        }
    };
    let h = &*handle;
    let mut r = lock_or_err!(h, mutable);
    r.set_font(family, size, line_height);
    0
}

/// Set theme colors from a JSON-encoded `ThemeColors`.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
/// - `theme_json` must point to `len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn renderer_set_theme(
    handle: *mut RendererHandle,
    theme_json: *const u8,
    len: usize,
) -> i32 {
    if handle.is_null() || theme_json.is_null() {
        return -1;
    }
    let h = &*handle;
    let slice = std::slice::from_raw_parts(theme_json, len);
    let theme: ThemeColors = match serde_json::from_slice(slice) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("renderer_set_theme: {e}");
            return -1;
        }
    };
    let mut r = lock_or_err!(h, mutable);
    r.set_theme(theme);
    0
}

/// Set cursor style and blink.
///
/// `style`: 0 = Block, 1 = Underline, 2 = Bar.
/// `blink`: 0 = no blink, 1 = blink.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
#[no_mangle]
pub unsafe extern "C" fn renderer_set_cursor_style(
    handle: *mut RendererHandle,
    style: u32,
    blink: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let h = &*handle;
    let cursor_style = match style {
        0 => CursorStyle::Block,
        1 => CursorStyle::Underline,
        2 => CursorStyle::Bar,
        _ => return -1,
    };
    let mut r = lock_or_err!(h, mutable);
    r.set_cursor_style(cursor_style, blink != 0);
    0
}

// ---------------------------------------------------------------------------
// Cell update / render
// ---------------------------------------------------------------------------

/// Update instance buffers from grid state.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
/// - `grid_handle` must be a valid pointer from `grid_create`.
#[no_mangle]
pub unsafe extern "C" fn renderer_update_cells(
    handle: *mut RendererHandle,
    grid_handle: *mut GridHandle,
) -> i32 {
    if handle.is_null() || grid_handle.is_null() {
        return -1;
    }
    let h = &*handle;
    let gh = &*grid_handle;
    let mut r = lock_or_err!(h, mutable);
    gh.with_grid(|grid| {
        let (bg, text, crow, ccol) = r.build_instances_from(grid);
        r.upload_instances(&bg, &text, crow, ccol);
    });
    // Re-upload atlas if build_instances_from rasterized new glyphs
    r.flush_atlas_if_dirty();
    0
}

/// Encode the render pass and present the frame.
///
/// Call after `renderer_update_cells` to submit GPU work. Returns 0 on success,
/// -1 on null handle, 1 if headless (no surface to present to).
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
#[no_mangle]
pub unsafe extern "C" fn renderer_render_frame(handle: *mut RendererHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let h = &*handle;
    let mut r = lock_or_err!(h, mutable);
    if !r.has_surface() {
        return 1; // headless — nothing to present
    }
    match r.submit_frame() {
        Ok(()) => 0,
        Err(wgpu::SurfaceError::Lost) => {
            tracing::warn!("renderer_render_frame: surface lost, reconfigure needed");
            -2
        }
        Err(wgpu::SurfaceError::OutOfMemory) => {
            tracing::error!("renderer_render_frame: out of GPU memory");
            -3
        }
        Err(e) => {
            tracing::warn!("renderer_render_frame: {e}");
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Surface resize
// ---------------------------------------------------------------------------

/// Resize the rendering surface.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
#[no_mangle]
pub unsafe extern "C" fn renderer_resize_surface(
    handle: *mut RendererHandle,
    width: u32,
    height: u32,
    scale_factor: f32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let h = &*handle;
    let mut r = lock_or_err!(h, mutable);
    r.resize_surface(width, height, scale_factor);
    0
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Get cell size in pixels.
///
/// Writes two little-endian f32 values (width, height) into the provided
/// byte buffers. Each buffer must be at least 4 bytes.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
/// - `out_width_buf` must point to at least 4 writable bytes.
/// - `out_height_buf` must point to at least 4 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn renderer_get_cell_size(
    handle: *mut RendererHandle,
    out_width_buf: *mut u8,
    out_height_buf: *mut u8,
) -> i32 {
    if handle.is_null() || out_width_buf.is_null() || out_height_buf.is_null() {
        return -1;
    }
    let h = &*handle;
    let r = lock_or_err!(h);
    let (w, h_val) = r.cell_size();
    // SAFETY: Caller guarantees at least 4 bytes in each buffer.
    std::ptr::copy_nonoverlapping(w.to_le_bytes().as_ptr(), out_width_buf, 4);
    std::ptr::copy_nonoverlapping(h_val.to_le_bytes().as_ptr(), out_height_buf, 4);
    0
}

/// Get grid dimensions (rows, cols) for current surface size.
///
/// Writes two little-endian u16 values (rows, cols) into the provided
/// byte buffers. Each buffer must be at least 2 bytes.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
/// - `out_rows_buf` must point to at least 2 writable bytes.
/// - `out_cols_buf` must point to at least 2 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn renderer_get_grid_dimensions(
    handle: *mut RendererHandle,
    out_rows_buf: *mut u8,
    out_cols_buf: *mut u8,
) -> i32 {
    if handle.is_null() || out_rows_buf.is_null() || out_cols_buf.is_null() {
        return -1;
    }
    let h = &*handle;
    let r = lock_or_err!(h);
    let (rows, cols) = r.grid_dimensions();
    // SAFETY: Caller guarantees at least 2 bytes in each buffer.
    std::ptr::copy_nonoverlapping(rows.to_le_bytes().as_ptr(), out_rows_buf, 2);
    std::ptr::copy_nonoverlapping(cols.to_le_bytes().as_ptr(), out_cols_buf, 2);
    0
}

/// Get a heap-allocated `Arc<wgpu::Device>` for sharing with compute.
///
/// Returns a pointer to a `Box<Arc<wgpu::Device>>`. The Arc clone keeps the
/// device alive even after the RendererHandle is destroyed. The caller MUST
/// free the returned pointer with `renderer_free_device_ptr` when done.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
#[no_mangle]
pub unsafe extern "C" fn renderer_get_device_ptr(handle: *mut RendererHandle) -> *const c_void {
    if handle.is_null() {
        return std::ptr::null();
    }
    let h = &*handle;
    let r = lock_or_null!(h);
    let arc = r.device_arc();
    // SAFETY: Heap-allocate the Arc so the pointer remains stable after MutexGuard drops.
    Box::into_raw(Box::new(arc)) as *const c_void
}

/// Get a heap-allocated `Arc<wgpu::Queue>` for sharing with compute.
///
/// Returns a pointer to a `Box<Arc<wgpu::Queue>>`. The Arc clone keeps the
/// queue alive even after the RendererHandle is destroyed. The caller MUST
/// free the returned pointer with `renderer_free_queue_ptr` when done.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
#[no_mangle]
pub unsafe extern "C" fn renderer_get_queue_ptr(handle: *mut RendererHandle) -> *const c_void {
    if handle.is_null() {
        return std::ptr::null();
    }
    let h = &*handle;
    let r = lock_or_null!(h);
    let arc = r.queue_arc();
    // SAFETY: Heap-allocate the Arc so the pointer remains stable after MutexGuard drops.
    Box::into_raw(Box::new(arc)) as *const c_void
}

/// Free a device pointer obtained from `renderer_get_device_ptr`.
///
/// # Safety
/// - `ptr` must be a valid pointer from `renderer_get_device_ptr`, or null (no-op).
/// - Must not be called more than once for the same pointer.
#[no_mangle]
pub unsafe extern "C" fn renderer_free_device_ptr(ptr: *const c_void) {
    if !ptr.is_null() {
        // SAFETY: ptr was created by Box::into_raw(Box::new(Arc<wgpu::Device>))
        drop(Box::from_raw(ptr as *mut Arc<wgpu::Device>));
    }
}

/// Free a queue pointer obtained from `renderer_get_queue_ptr`.
///
/// # Safety
/// - `ptr` must be a valid pointer from `renderer_get_queue_ptr`, or null (no-op).
/// - Must not be called more than once for the same pointer.
#[no_mangle]
pub unsafe extern "C" fn renderer_free_queue_ptr(ptr: *const c_void) {
    if !ptr.is_null() {
        // SAFETY: ptr was created by Box::into_raw(Box::new(Arc<wgpu::Queue>))
        drop(Box::from_raw(ptr as *mut Arc<wgpu::Queue>));
    }
}

// ---------------------------------------------------------------------------
// Overlay management
// ---------------------------------------------------------------------------

/// Add an overlay layer. `config_json` is a JSON-encoded `OverlayConfig`.
///
/// If an overlay with the same `layer_id` exists, it is replaced.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
/// - `config_json` must point to `config_json_len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn renderer_add_overlay(
    handle: *mut RendererHandle,
    config_json: *const u8,
    config_json_len: usize,
) -> i32 {
    if handle.is_null() || config_json.is_null() || config_json_len == 0 {
        return -1;
    }
    let h = &*handle;
    let slice = std::slice::from_raw_parts(config_json, config_json_len);
    let config: crate::renderer::OverlayConfig = match serde_json::from_slice(slice) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("renderer_add_overlay: bad JSON: {e}");
            return -1;
        }
    };
    let mut r = lock_or_err!(h, mutable);
    r.add_overlay(config);
    0
}

/// Remove an overlay layer by ID.
///
/// Returns 0 on success, -1 if the handle is null, -2 if no overlay with
/// that ID existed.
///
/// # Safety
/// - `handle` must be a valid pointer from `renderer_create`.
#[no_mangle]
pub unsafe extern "C" fn renderer_remove_overlay(
    handle: *mut RendererHandle,
    layer_id: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let h = &*handle;
    let mut r = lock_or_err!(h, mutable);
    if r.remove_overlay(layer_id) {
        0
    } else {
        -2
    }
}
