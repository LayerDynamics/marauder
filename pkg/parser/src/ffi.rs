use std::ffi::c_void;
use std::sync::Mutex;

use crate::performer::MarauderParser;

/// Opaque handle for FFI consumers.
pub struct ParserHandle {
    parser: Mutex<MarauderParser>,
}

/// Create a new parser. Returns an opaque handle.
///
/// # Safety
/// Caller must eventually call `parser_destroy` to free the handle.
#[no_mangle]
pub extern "C" fn parser_create() -> *mut ParserHandle {
    let handle = Box::new(ParserHandle {
        parser: Mutex::new(MarauderParser::new()),
    });
    Box::into_raw(handle)
}

/// Feed bytes into the parser. The callback is invoked for each parsed action.
///
/// Callback signature: `fn(action_type: u32, data: *const u8, data_len: usize, user_data: *mut c_void)`
///
/// The `data` pointer contains JSON-serialized action data. The `action_type` is a discriminant
/// for the action kind (see `TerminalAction` variants).
///
/// # Safety
/// - `handle` must be a valid pointer from `parser_create`.
/// - `input` must point to `input_len` valid bytes.
/// - `callback` must be a valid function pointer.
/// - `user_data` must be valid for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn parser_feed(
    handle: *mut ParserHandle,
    input: *const u8,
    input_len: usize,
    callback: extern "C" fn(action_json: *const u8, action_json_len: usize, user_data: *mut c_void),
    user_data: *mut c_void,
) {
    if handle.is_null() || input.is_null() {
        return;
    }
    let handle = unsafe { &*handle };
    let bytes = unsafe { std::slice::from_raw_parts(input, input_len) };

    let mut parser = handle.parser.lock().unwrap_or_else(|e| e.into_inner());
    parser.feed(bytes, |action| {
        if let Ok(json) = serde_json::to_vec(&action) {
            callback(json.as_ptr(), json.len(), user_data);
        }
    });
}

/// Reset the parser state.
///
/// # Safety
/// - `handle` must be a valid pointer from `parser_create`.
#[no_mangle]
pub unsafe extern "C" fn parser_reset(handle: *mut ParserHandle) {
    if handle.is_null() {
        return;
    }
    let handle = unsafe { &*handle };
    let mut parser = handle.parser.lock().unwrap_or_else(|e| e.into_inner());
    *parser = MarauderParser::new();
}

/// Destroy a parser handle, freeing its memory.
///
/// # Safety
/// - `handle` must be a valid pointer from `parser_create`, or null (no-op).
/// - Must not be called more than once for the same handle.
#[no_mangle]
pub unsafe extern "C" fn parser_destroy(handle: *mut ParserHandle) {
    if !handle.is_null() {
        // SAFETY: handle is valid and not previously freed per caller contract
        let _ = unsafe { Box::from_raw(handle) };
    }
}
