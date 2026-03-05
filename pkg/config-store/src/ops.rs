//! deno_core #[op2] ops for the config store in embedded mode.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use deno_core::op2;
use deno_core::OpState;
use serde_json::Value;

use crate::store::{ConfigStore, SharedConfigStore};

/// Error type for config store ops.
#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[error("{0}")]
pub struct ConfigOpError(String);

impl From<crate::layer::ConfigError> for ConfigOpError {
    fn from(e: crate::layer::ConfigError) -> Self {
        Self(e.to_string())
    }
}

type ConfigMap = Arc<Mutex<HashMap<u32, ConfigStore>>>;
type NextConfigId = Arc<Mutex<u32>>;

/// Map of handle → SharedConfigStore for live-shared config stores from the runtime.
type SharedConfigMap = Arc<Mutex<HashMap<u32, SharedConfigStore>>>;

fn init_config_state(state: &mut OpState) {
    state.put::<ConfigMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<NextConfigId>(Arc::new(Mutex::new(1)));
    state.put::<SharedConfigMap>(Arc::new(Mutex::new(HashMap::new())));
}

/// Inject a shared config store from the real runtime.
/// Pre-registers it at the given handle.
pub fn inject_shared_config_store(state: &mut OpState, handle: u32, store: SharedConfigStore) {
    let shared = state.borrow::<SharedConfigMap>().clone();
    shared.lock().unwrap_or_else(|e| e.into_inner()).insert(handle, store);
}

fn with_config<R>(
    state: &mut OpState,
    handle: u32,
    f: impl FnOnce(&mut ConfigStore) -> R,
) -> Result<R, ConfigOpError> {
    // Check shared config stores first (live runtime stores)
    let shared = state.borrow::<SharedConfigMap>().clone();
    let shared_map = shared.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(store_arc) = shared_map.get(&handle) {
        let store_arc = store_arc.clone();
        drop(shared_map);
        let mut store = store_arc.write().unwrap_or_else(|e| e.into_inner());
        return Ok(f(&mut store));
    }
    drop(shared_map);

    // Fall back to local config map
    let map = state.borrow::<ConfigMap>().clone();
    let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
    let store = map
        .get_mut(&handle)
        .ok_or_else(|| ConfigOpError(format!("invalid config handle: {handle}")))?;
    Ok(f(store))
}

/// Create a new config store, returns handle ID.
#[op2(fast)]
#[smi]
pub fn op_config_create(state: &mut OpState) -> Result<u32, ConfigOpError> {
    let id_rc = state.borrow::<NextConfigId>().clone();
    let mut id = id_rc.lock().unwrap_or_else(|e| e.into_inner());
    let handle = *id;
    *id = id.checked_add(1).ok_or_else(|| ConfigOpError("config handle ID overflow".to_string()))?;
    drop(id);

    let map = state.borrow::<ConfigMap>().clone();
    map.lock().unwrap_or_else(|e| e.into_inner()).insert(handle, ConfigStore::new());
    Ok(handle)
}

/// Load config from file paths. Pass empty string to skip a layer.
/// Async because it performs file I/O that could block the event loop.
#[op2(async)]
pub async fn op_config_load(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
    #[string] system: String,
    #[string] user: String,
    #[string] project: String,
) -> Result<(), ConfigOpError> {
    // Check shared stores first (live runtime stores)
    let shared = {
        let st = state.borrow();
        st.borrow::<SharedConfigMap>().clone()
    };
    let shared_map = shared.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(store_arc) = shared_map.get(&handle) {
        let store_arc = store_arc.clone();
        drop(shared_map);

        let sys = if system.is_empty() { None } else { Some(std::path::PathBuf::from(&system)) };
        let usr = if user.is_empty() { None } else { Some(std::path::PathBuf::from(&user)) };
        let prj = if project.is_empty() { None } else { Some(std::path::PathBuf::from(&project)) };

        return tokio::task::spawn_blocking(move || {
            let mut store = store_arc.write().unwrap_or_else(|e| e.into_inner());
            store.load(sys.as_deref(), usr.as_deref(), prj.as_deref())
                .map_err(|e| ConfigOpError(e.to_string()))
        }).await.map_err(|e| ConfigOpError(e.to_string()))?;
    }
    drop(shared_map);

    // Fall back to local map: remove store, do I/O, re-insert
    let map = {
        let st = state.borrow();
        st.borrow::<ConfigMap>().clone()
    };
    let mut store = map.lock().unwrap_or_else(|e| e.into_inner())
        .remove(&handle)
        .ok_or_else(|| ConfigOpError(format!("invalid config handle: {handle}")))?;

    let sys = if system.is_empty() { None } else { Some(std::path::PathBuf::from(&system)) };
    let usr = if user.is_empty() { None } else { Some(std::path::PathBuf::from(&user)) };
    let prj = if project.is_empty() { None } else { Some(std::path::PathBuf::from(&project)) };

    let result = tokio::task::spawn_blocking(move || {
        let r = store.load(sys.as_deref(), usr.as_deref(), prj.as_deref())
            .map_err(|e| ConfigOpError(e.to_string()));
        (store, r)
    }).await.map_err(|e| ConfigOpError(e.to_string()))?;

    let (store, r) = result;
    map.lock().unwrap_or_else(|e| e.into_inner()).insert(handle, store);
    r
}

/// Get a config value by key, returns JSON or null.
#[op2]
#[serde]
pub fn op_config_get(
    state: &mut OpState,
    #[smi] handle: u32,
    #[string] key: String,
) -> Result<Option<Value>, ConfigOpError> {
    with_config(state, handle, |store| store.get(&key).cloned())
}

/// Set a config value (CLI override layer).
#[op2]
pub fn op_config_set(
    state: &mut OpState,
    #[smi] handle: u32,
    #[string] key: String,
    #[serde] value: serde_json::Value,
) -> Result<(), ConfigOpError> {
    with_config(state, handle, |store| store.set(&key, value))
}

/// Save user config to a file path.
/// Async because it performs file I/O that could block the event loop.
#[op2(async)]
pub async fn op_config_save(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
    #[string] path: String,
) -> Result<(), ConfigOpError> {
    let shared = {
        let st = state.borrow();
        st.borrow::<SharedConfigMap>().clone()
    };
    let shared_map = shared.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(store_arc) = shared_map.get(&handle) {
        let store_arc = store_arc.clone();
        drop(shared_map);
        return tokio::task::spawn_blocking(move || {
            let store = store_arc.read().unwrap_or_else(|e| e.into_inner());
            store.save_user_config(std::path::Path::new(&path))
                .map_err(|e| ConfigOpError(e.to_string()))
        }).await.map_err(|e| ConfigOpError(e.to_string()))?;
    }
    drop(shared_map);

    let map = {
        let st = state.borrow();
        st.borrow::<ConfigMap>().clone()
    };
    let store = map.lock().unwrap_or_else(|e| e.into_inner())
        .remove(&handle)
        .ok_or_else(|| ConfigOpError(format!("invalid config handle: {handle}")))?;

    let result = tokio::task::spawn_blocking(move || {
        let r = store.save_user_config(std::path::Path::new(&path))
            .map_err(|e| ConfigOpError(e.to_string()));
        (store, r)
    }).await.map_err(|e| ConfigOpError(e.to_string()))?;

    let (store, r) = result;
    map.lock().unwrap_or_else(|e| e.into_inner()).insert(handle, store);
    r
}

/// Reload config files from disk.
/// Async because it performs file I/O that could block the event loop.
#[op2(async)]
pub async fn op_config_reload(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
) -> Result<(), ConfigOpError> {
    let shared = {
        let st = state.borrow();
        st.borrow::<SharedConfigMap>().clone()
    };
    let shared_map = shared.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(store_arc) = shared_map.get(&handle) {
        let store_arc = store_arc.clone();
        drop(shared_map);
        return tokio::task::spawn_blocking(move || {
            let mut store = store_arc.write().unwrap_or_else(|e| e.into_inner());
            store.reload().map_err(|e| ConfigOpError(e.to_string()))
        }).await.map_err(|e| ConfigOpError(e.to_string()))?;
    }
    drop(shared_map);

    let map = {
        let st = state.borrow();
        st.borrow::<ConfigMap>().clone()
    };
    let mut store = map.lock().unwrap_or_else(|e| e.into_inner())
        .remove(&handle)
        .ok_or_else(|| ConfigOpError(format!("invalid config handle: {handle}")))?;

    let result = tokio::task::spawn_blocking(move || {
        let r = store.reload().map_err(|e| ConfigOpError(e.to_string()));
        (store, r)
    }).await.map_err(|e| ConfigOpError(e.to_string()))?;

    let (store, r) = result;
    map.lock().unwrap_or_else(|e| e.into_inner()).insert(handle, store);
    r
}

/// Get all config keys.
#[op2]
#[serde]
pub fn op_config_keys(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<Vec<String>, ConfigOpError> {
    with_config(state, handle, |store| {
        store.keys().into_iter().map(|s| s.to_string()).collect()
    })
}

/// Destroy a config store instance.
#[op2(fast)]
pub fn op_config_destroy(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<(), ConfigOpError> {
    {
        let shared = state.borrow::<SharedConfigMap>().clone();
        shared.lock().unwrap_or_else(|e| e.into_inner()).remove(&handle);
    }
    let map = state.borrow::<ConfigMap>().clone();
    map.lock().unwrap_or_else(|e| e.into_inner()).remove(&handle);
    Ok(())
}

deno_core::extension!(
    marauder_config_store_ext,
    ops = [
        op_config_create,
        op_config_load,
        op_config_get,
        op_config_set,
        op_config_save,
        op_config_reload,
        op_config_keys,
        op_config_destroy,
    ],
    state = |state| init_config_state(state),
);

/// Build the deno_core Extension for config store ops.
pub fn config_store_extension() -> deno_core::Extension {
    marauder_config_store_ext::init()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deno_core::OpState;

    fn make_state() -> OpState {
        let mut state = OpState::new(None);
        init_config_state(&mut state);
        state
    }

    /// Create a new config store by directly manipulating the state maps,
    /// mirroring the logic in op_config_create.
    fn create_store(state: &mut OpState) -> Result<u32, ConfigOpError> {
        let id_rc = state.borrow::<NextConfigId>().clone();
        let mut id = id_rc.lock().unwrap_or_else(|e| e.into_inner());
        let handle = *id;
        *id = id.checked_add(1).ok_or_else(|| ConfigOpError("config handle ID overflow".to_string()))?;
        drop(id);
        let map = state.borrow::<ConfigMap>().clone();
        map.lock().unwrap_or_else(|e| e.into_inner()).insert(handle, ConfigStore::new());
        Ok(handle)
    }

    /// Get a value from a store by directly invoking with_config.
    fn get_value(state: &mut OpState, handle: u32, key: &str) -> Result<Option<Value>, ConfigOpError> {
        with_config(state, handle, |store| store.get(key).cloned())
    }

    /// Set a value in a store by directly invoking with_config.
    fn set_value(state: &mut OpState, handle: u32, key: &str, value: Value) -> Result<(), ConfigOpError> {
        with_config(state, handle, |store| store.set(key, value))
    }

    /// Get all keys from a store by directly invoking with_config.
    fn get_keys(state: &mut OpState, handle: u32) -> Result<Vec<String>, ConfigOpError> {
        with_config(state, handle, |store| {
            store.keys().into_iter().map(|s| s.to_string()).collect()
        })
    }

    /// Destroy a store by directly removing it from the config maps.
    fn destroy_store(state: &mut OpState, handle: u32) -> Result<(), ConfigOpError> {
        {
            let shared = state.borrow::<SharedConfigMap>().clone();
            shared.lock().unwrap_or_else(|e| e.into_inner()).remove(&handle);
        }
        let map = state.borrow::<ConfigMap>().clone();
        map.lock().unwrap_or_else(|e| e.into_inner()).remove(&handle);
        Ok(())
    }

    #[test]
    fn test_create_returns_incrementing_handles() {
        let mut state = make_state();
        let h1 = create_store(&mut state).expect("first create should succeed");
        let h2 = create_store(&mut state).expect("second create should succeed");
        assert_eq!(h1, 1);
        assert_eq!(h2, 2);
    }

    #[test]
    fn test_get_missing_key() {
        let mut state = make_state();
        let handle = create_store(&mut state).expect("create should succeed");
        let result = get_value(&mut state, handle, "nonexistent")
            .expect("get on missing key should return Ok");
        assert!(result.is_none());
    }

    #[test]
    fn test_set_and_get() {
        let mut state = make_state();
        let handle = create_store(&mut state).expect("create should succeed");
        let value = serde_json::Value::String("dark".to_string());
        set_value(&mut state, handle, "theme", value.clone())
            .expect("set should succeed");
        let got = get_value(&mut state, handle, "theme")
            .expect("get should succeed");
        assert_eq!(got, Some(value));
    }

    #[test]
    fn test_keys() {
        let mut state = make_state();
        let handle = create_store(&mut state).expect("create should succeed");
        set_value(&mut state, handle, "alpha", serde_json::Value::Bool(true))
            .expect("set alpha should succeed");
        set_value(&mut state, handle, "beta", serde_json::Value::Number(42.into()))
            .expect("set beta should succeed");
        let keys = get_keys(&mut state, handle).expect("keys should succeed");
        assert!(keys.contains(&"alpha".to_string()), "keys should contain 'alpha'");
        assert!(keys.contains(&"beta".to_string()), "keys should contain 'beta'");
    }

    #[test]
    fn test_invalid_handle() {
        let mut state = make_state();
        let result = get_value(&mut state, 9999, "key");
        assert!(result.is_err(), "get on invalid handle should return an error");
    }

    #[test]
    fn test_destroy_then_access() {
        let mut state = make_state();
        let handle = create_store(&mut state).expect("create should succeed");
        destroy_store(&mut state, handle).expect("destroy should succeed");
        let result = get_value(&mut state, handle, "key");
        assert!(result.is_err(), "get after destroy should return an error");
    }

    #[test]
    fn test_inject_shared_config_store() {
        let mut state = make_state();
        let shared_store: SharedConfigStore = Arc::new(std::sync::RwLock::new(ConfigStore::new()));
        let handle = 42u32;
        inject_shared_config_store(&mut state, handle, shared_store);

        let value = serde_json::Value::String("solarized".to_string());
        set_value(&mut state, handle, "colorscheme", value.clone())
            .expect("set on injected shared store should succeed");
        let got = get_value(&mut state, handle, "colorscheme")
            .expect("get on injected shared store should succeed");
        assert_eq!(got, Some(value));
    }

    #[test]
    fn test_handle_overflow() {
        let mut state = make_state();
        {
            let id_rc = state.borrow::<NextConfigId>().clone();
            let mut id = id_rc.lock().unwrap();
            *id = u32::MAX;
        }
        let result = create_store(&mut state);
        assert!(result.is_err(), "create at u32::MAX should return overflow error");
    }
}
