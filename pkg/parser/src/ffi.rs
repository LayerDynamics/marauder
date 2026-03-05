use std::ffi::c_void;
use std::sync::Mutex;

use marauder_event_bus::lock_or_log;

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
/// Callback signature: `fn(action_json: *const u8, action_json_len: usize, user_data: *mut c_void)`
///
/// The `action_json` pointer contains a JSON-serialized `TerminalAction` (tagged enum).
/// The action kind is encoded in the JSON as a `"type"` field — there is no separate discriminant.
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

    let mut parser = lock_or_log(&handle.parser, "parser::ffi_feed");
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
    let mut parser = lock_or_log(&handle.parser, "parser::ffi_reset");
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
