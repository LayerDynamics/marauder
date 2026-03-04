use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::Path;
use std::ptr;
use std::sync::{Arc, RwLock};

use crate::store::ConfigStore;

/// Opaque handle for FFI consumers.
///
/// Thread-safety: This handle is safe to use from multiple threads concurrently.
/// Internally it uses `Arc<RwLock<ConfigStore>>`, so reads are concurrent and writes
/// are serialized. The watcher field (when present) is also thread-safe.
///
/// # Safety
/// The handle must be created via `config_store_create` and freed via `config_store_destroy`.
/// Do not use the handle after calling `config_store_destroy`.
pub struct ConfigStoreHandle {
    store: Arc<RwLock<ConfigStore>>,
    _watcher: Option<crate::watcher::ConfigWatcher>,
}

/// Create a new ConfigStore with defaults.
///
/// # Safety
/// Caller must eventually call `config_store_destroy` on the returned pointer.
#[no_mangle]
pub extern "C" fn config_store_create() -> *mut ConfigStoreHandle {
    let store = ConfigStore::new();
    let handle = ConfigStoreHandle {
        store: Arc::new(RwLock::new(store)),
        _watcher: None,
    };
    Box::into_raw(Box::new(handle))
}

/// Load config from file paths. Pass null for any path to skip that layer.
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid pointer from `config_store_create`.
/// Path pointers must be valid null-terminated UTF-8 strings or null.
#[no_mangle]
pub unsafe extern "C" fn config_store_load(
    handle: *mut ConfigStoreHandle,
    system: *const c_char,
    user: *const c_char,
    project: *const c_char,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    // SAFETY: handle verified non-null above, caller guarantees validity
    let handle = unsafe { &*handle };

    let system_path = unsafe { nullable_c_str_to_path(system) };
    let user_path = unsafe { nullable_c_str_to_path(user) };
    let project_path = unsafe { nullable_c_str_to_path(project) };

    match handle.store.write() {
        Ok(mut store) => {
            match store.load(
                system_path.as_deref(),
                user_path.as_deref(),
                project_path.as_deref(),
            ) {
                Ok(()) => 0,
                Err(e) => {
                    tracing::error!("config_store_load error: {e}");
                    -1
                }
            }
        }
        Err(_) => -1,
    }
}

/// Get a config value as JSON string. Writes to `out_buf` up to `out_buf_len` bytes.
///
/// Returns the **total** JSON byte length (even if truncated). If the return value exceeds
/// `out_buf_len`, the output was truncated — the caller should retry with a larger buffer.
/// Returns -1 on error (null pointers, key not found, lock poisoned).
///
/// # Safety
/// `handle` must be valid. `key` must be a valid null-terminated UTF-8 string.
/// `out_buf` must point to at least `out_buf_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn config_store_get(
    handle: *mut ConfigStoreHandle,
    key: *const c_char,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> i32 {
    if handle.is_null() || key.is_null() || out_buf.is_null() {
        return -1;
    }
    // SAFETY: pointers verified non-null, caller guarantees validity
    let handle = unsafe { &*handle };
    let key_str = match unsafe { CStr::from_ptr(key) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    match handle.store.read() {
        Ok(store) => match store.get(key_str) {
            Some(value) => {
                let json = serde_json::to_string(value).unwrap_or_default();
                let bytes = json.as_bytes();
                let total_len = bytes.len();
                let copy_len = total_len.min(out_buf_len);
                // SAFETY: out_buf valid for out_buf_len bytes per contract
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, copy_len);
                }
                // Return total length, capped at i32::MAX to prevent overflow
                (total_len as u64).min(i32::MAX as u64) as i32
            }
            None => -1,
        },
        Err(_) => -1,
    }
}

/// Set a config value from a JSON string in the CLI override layer.
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be valid. `key` and `value_json` must be valid null-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn config_store_set(
    handle: *mut ConfigStoreHandle,
    key: *const c_char,
    value_json: *const c_char,
) -> i32 {
    if handle.is_null() || key.is_null() || value_json.is_null() {
        return -1;
    }
    // SAFETY: pointers verified non-null, caller guarantees validity
    let handle = unsafe { &*handle };
    let key_str = match unsafe { CStr::from_ptr(key) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let value_str = match unsafe { CStr::from_ptr(value_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let value: serde_json::Value = match serde_json::from_str(value_str) {
        Ok(v) => v,
        Err(_) => return -1,
    };

    match handle.store.write() {
        Ok(mut store) => {
            store.set(key_str, value);
            0
        }
        Err(_) => -1,
    }
}

/// Save the user config layer to a TOML file.
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be valid. `path` must be a valid null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn config_store_save(
    handle: *mut ConfigStoreHandle,
    path: *const c_char,
) -> i32 {
    if handle.is_null() || path.is_null() {
        return -1;
    }
    // SAFETY: pointers verified non-null, caller guarantees validity
    let handle = unsafe { &*handle };
    let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    match handle.store.read() {
        Ok(store) => match store.save_user_config(Path::new(path_str)) {
            Ok(()) => 0,
            Err(e) => {
                tracing::error!("config_store_save error: {e}");
                -1
            }
        },
        Err(_) => -1,
    }
}

/// Start watching loaded config file paths for changes. On change, the store reloads
/// automatically. Requires a tokio runtime to be active on the current thread.
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid pointer from `config_store_create`.
#[no_mangle]
pub unsafe extern "C" fn config_store_watch(handle: *mut ConfigStoreHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }
    // SAFETY: handle verified non-null, caller guarantees validity
    let handle = unsafe { &mut *handle };

    let paths = match handle.store.read() {
        Ok(store) => store.watched_paths(),
        Err(_) => return -1,
    };

    if paths.is_empty() {
        return 0; // Nothing to watch
    }

    match crate::watcher::ConfigWatcher::new(Arc::clone(&handle.store), paths) {
        Ok(watcher) => {
            handle._watcher = Some(watcher);
            0
        }
        Err(e) => {
            tracing::error!("config_store_watch error: {e}");
            -1
        }
    }
}

/// Stop watching config files. No-op if not currently watching.
/// Returns 0 on success.
///
/// # Safety
/// `handle` must be a valid pointer from `config_store_create`.
#[no_mangle]
pub unsafe extern "C" fn config_store_unwatch(handle: *mut ConfigStoreHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }
    // SAFETY: handle verified non-null, caller guarantees validity
    let handle = unsafe { &mut *handle };
    // Drop the watcher (stops the background task)
    handle._watcher = None;
    0
}

/// Destroy a ConfigStore handle, freeing its memory.
///
/// # Safety
/// `handle` must be a valid pointer from `config_store_create`, and must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn config_store_destroy(handle: *mut ConfigStoreHandle) {
    if !handle.is_null() {
        // SAFETY: handle is non-null and was created by Box::into_raw in config_store_create
        let _ = unsafe { Box::from_raw(handle) };
    }
}

/// Helper: convert a nullable C string to an Option<PathBuf>.
///
/// # Safety
/// If non-null, `ptr` must be a valid null-terminated UTF-8 C string.
unsafe fn nullable_c_str_to_path(ptr: *const c_char) -> Option<std::path::PathBuf> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees ptr is a valid null-terminated C string
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(std::path::PathBuf::from)
}
