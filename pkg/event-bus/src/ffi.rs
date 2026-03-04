use std::ffi::c_void;
use std::sync::Arc;

use crate::bus::{EventBus, SharedEventBus, SubscriberId};
use crate::events::{Event, EventType};
use crate::interceptor::{Interceptor, InterceptorAction};

/// Opaque handle for FFI consumers.
pub struct EventBusHandle {
    bus: SharedEventBus,
}

/// Create a new EventBus. Returns an opaque handle.
///
/// # Safety
/// Caller must eventually call `event_bus_destroy` to free the handle.
#[no_mangle]
pub extern "C" fn event_bus_create() -> *mut EventBusHandle {
    let bus = Arc::new(EventBus::new());
    let handle = Box::new(EventBusHandle { bus });
    Box::into_raw(handle)
}

/// Subscribe to an event type with a C callback.
///
/// Returns a subscriber ID (>0) on success, or 0 on error (null handle, invalid event type).
///
/// Callback signature: fn(event_json: *const u8, event_json_len: usize, user_data: *mut c_void)
///
/// # Safety
/// - `handle` must be a valid pointer from `event_bus_create`, or null (returns 0).
/// - `callback` must be a valid function pointer for the lifetime of the subscription.
/// - `user_data` must be valid for the lifetime of the subscription.
#[no_mangle]
pub unsafe extern "C" fn event_bus_subscribe(
    handle: *mut EventBusHandle,
    event_type: u32,
    callback: extern "C" fn(*const u8, usize, *mut c_void),
    user_data: *mut c_void,
) -> u64 {
    // SAFETY: null check before dereference
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };

    let event_type = match EventType::from_u32(event_type) {
        Ok(et) => et,
        Err(_) => return 0,
    };

    let user_data = UserDataWrapper { p: user_data };
    let callback = CallbackWrapper { f: callback };

    let id = handle.bus.subscribe(event_type, move |event| {
        if let Ok(json) = serde_json::to_vec(event) {
            callback.call(json.as_ptr(), json.len(), user_data.ptr());
        }
    });

    id.0
}

/// Unsubscribe a previously registered callback.
///
/// Returns 1 on success, 0 on error (null handle, invalid event type).
///
/// # Safety
/// - `handle` must be a valid pointer from `event_bus_create`, or null (returns 0).
#[no_mangle]
pub unsafe extern "C" fn event_bus_unsubscribe(
    handle: *mut EventBusHandle,
    event_type: u32,
    subscriber_id: u64,
) -> i32 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };

    let event_type = match EventType::from_u32(event_type) {
        Ok(et) => et,
        Err(_) => return 0,
    };

    handle.bus.unsubscribe(event_type, SubscriberId(subscriber_id));
    1
}

/// Publish an event with a JSON payload.
///
/// Returns 1 on success, 0 on error (null handle, invalid event type).
///
/// # Safety
/// - `handle` must be a valid pointer from `event_bus_create`, or null (returns 0).
/// - `payload` must point to `payload_len` valid bytes of JSON.
#[no_mangle]
pub unsafe extern "C" fn event_bus_publish(
    handle: *mut EventBusHandle,
    event_type: u32,
    payload: *const u8,
    payload_len: usize,
) -> i32 {
    // SAFETY: null check before dereference
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };

    let event_type = match EventType::from_u32(event_type) {
        Ok(et) => et,
        Err(_) => return 0,
    };

    // SAFETY: payload valid per caller contract
    let payload_slice = if payload.is_null() || payload_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(payload, payload_len) }
    };

    let event = Event {
        event_type,
        payload: payload_slice.to_vec(),
        timestamp_us: Event::now_us(),
        source: None,
    };

    handle.bus.publish(event);
    1
}

/// Register an interceptor via FFI callback.
///
/// The callback receives the event JSON, its length, and user_data.
/// It must return: 0 = Pass, 1 = Suppress, 2 = Modify (write modified JSON to the output buffer).
///
/// For Modify: the callback writes modified JSON to `out_buf` (up to `out_buf_len`), and sets
/// `*out_written` to the number of bytes written. If out_buf is too small, falls back to Pass.
///
/// Returns 1 on success, 0 on error (null handle).
///
/// # Safety
/// - `handle` must be a valid pointer from `event_bus_create`, or null (returns 0).
/// - `callback`, `user_data` must be valid for the lifetime of the interceptor.
#[no_mangle]
pub unsafe extern "C" fn event_bus_intercept(
    handle: *mut EventBusHandle,
    priority: i32,
    callback: extern "C" fn(
        event_json: *const u8,
        event_json_len: usize,
        out_buf: *mut u8,
        out_buf_len: usize,
        out_written: *mut usize,
        user_data: *mut c_void,
    ) -> u32,
    user_data: *mut c_void,
) -> i32 {
    if handle.is_null() {
        return 0;
    }
    let handle = unsafe { &*handle };

    let user_data = UserDataWrapper { p: user_data };
    let callback = InterceptCallbackWrapper { f: callback };

    struct FfiInterceptor {
        priority: i32,
        callback: InterceptCallbackWrapper,
        user_data: UserDataWrapper,
    }

    impl Interceptor for FfiInterceptor {
        fn priority(&self) -> i32 {
            self.priority
        }

        fn intercept(&self, event: &Event) -> InterceptorAction {
            let json = match serde_json::to_vec(event) {
                Ok(j) => j,
                Err(_) => return InterceptorAction::Pass,
            };

            let mut out_buf = vec![0u8; json.len() * 2 + 1024]; // generous buffer
            let mut out_written: usize = 0;

            let result = (self.callback.f)(
                json.as_ptr(),
                json.len(),
                out_buf.as_mut_ptr(),
                out_buf.len(),
                &mut out_written as *mut usize,
                self.user_data.ptr(),
            );

            match result {
                0 => InterceptorAction::Pass,
                1 => InterceptorAction::Suppress,
                2 => {
                    if out_written > 0 && out_written <= out_buf.len() {
                        match serde_json::from_slice::<Event>(&out_buf[..out_written]) {
                            Ok(modified) => InterceptorAction::Modify(modified),
                            Err(_) => InterceptorAction::Pass,
                        }
                    } else {
                        InterceptorAction::Pass
                    }
                }
                _ => InterceptorAction::Pass,
            }
        }
    }

    handle.bus.add_interceptor(Box::new(FfiInterceptor {
        priority,
        callback,
        user_data,
    }));

    1
}

/// Destroy an EventBus handle, freeing its memory.
///
/// # Safety
/// - `handle` must be a valid pointer from `event_bus_create`, or null (no-op).
/// - Must not be called more than once for the same handle.
#[no_mangle]
pub unsafe extern "C" fn event_bus_destroy(handle: *mut EventBusHandle) {
    if !handle.is_null() {
        // SAFETY: handle is valid and not previously freed per caller contract
        let _ = unsafe { Box::from_raw(handle) };
    }
}

// --- Send+Sync wrappers for FFI function pointers and raw pointers ---

/// Wrapper for subscriber callbacks.
struct CallbackWrapper {
    f: extern "C" fn(*const u8, usize, *mut c_void),
}
unsafe impl Send for CallbackWrapper {}
unsafe impl Sync for CallbackWrapper {}

impl CallbackWrapper {
    fn call(&self, data: *const u8, len: usize, user_data: *mut c_void) {
        (self.f)(data, len, user_data);
    }
}

impl Clone for CallbackWrapper {
    fn clone(&self) -> Self {
        Self { f: self.f }
    }
}
impl Copy for CallbackWrapper {}

/// Wrapper for interceptor callbacks.
struct InterceptCallbackWrapper {
    f: extern "C" fn(*const u8, usize, *mut u8, usize, *mut usize, *mut c_void) -> u32,
}
unsafe impl Send for InterceptCallbackWrapper {}
unsafe impl Sync for InterceptCallbackWrapper {}

impl Clone for InterceptCallbackWrapper {
    fn clone(&self) -> Self {
        Self { f: self.f }
    }
}
impl Copy for InterceptCallbackWrapper {}

/// Wrapper to make *mut c_void Send+Sync.
/// SAFETY: Caller ensures user_data is valid for the subscription lifetime.
struct UserDataWrapper {
    p: *mut c_void,
}
unsafe impl Send for UserDataWrapper {}
unsafe impl Sync for UserDataWrapper {}

impl UserDataWrapper {
    fn ptr(&self) -> *mut c_void {
        self.p
    }
}

impl Clone for UserDataWrapper {
    fn clone(&self) -> Self {
        Self { p: self.p }
    }
}
impl Copy for UserDataWrapper {}
