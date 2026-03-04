//! High-level deno_bindgen bindings for the config store.

use deno_bindgen::deno_bindgen;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::store::ConfigStore;

static HANDLES: OnceLock<Mutex<HashMap<u32, Arc<RwLock<ConfigStore>>>>> = OnceLock::new();
static NEXT_ID: OnceLock<Mutex<u32>> = OnceLock::new();

fn handles() -> &'static Mutex<HashMap<u32, Arc<RwLock<ConfigStore>>>> {
    HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u32 {
    let mut id = NEXT_ID.get_or_init(|| Mutex::new(1)).lock().unwrap_or_else(|e| e.into_inner());
    let val = *id;
    match val.checked_add(1) {
        Some(next) => { *id = next; val }
        None => {
            tracing::error!("bindgen handle ID counter overflow");
            0
        }
    }
}

fn get_store(handle_id: u32) -> Option<Arc<RwLock<ConfigStore>>> {
    handles().lock().unwrap_or_else(|e| e.into_inner()).get(&handle_id).cloned()
}

/// Create a new ConfigStore with defaults. Returns a handle ID.
#[deno_bindgen]
fn config_store_bindgen_create() -> u32 {
    let id = next_id();
    if id == 0 {
        return 0; // overflow sentinel — caller must treat 0 as invalid
    }
    handles().lock().unwrap_or_else(|e| e.into_inner()).insert(id, Arc::new(RwLock::new(ConfigStore::new())));
    id
}

/// Load config from file paths. Pass empty string to skip a layer.
/// Returns 1 on success, 0 on error.
#[deno_bindgen]
fn config_store_bindgen_load(handle_id: u32, system: &str, user: &str, project: &str) -> u8 {
    let store = match get_store(handle_id) {
        Some(s) => s,
        None => return 0,
    };
    let system = if system.is_empty() { None } else { Some(std::path::Path::new(system)) };
    let user = if user.is_empty() { None } else { Some(std::path::Path::new(user)) };
    let project = if project.is_empty() { None } else { Some(std::path::Path::new(project)) };

    let result = match store.write() {
        Ok(mut s) => if s.load(system, user, project).is_ok() { 1 } else { 0 },
        Err(_) => 0,
    };
    result
}

/// Get a config value as JSON string. Returns empty string if not found.
#[deno_bindgen]
fn config_store_bindgen_get(handle_id: u32, key: &str) -> String {
    let store = match get_store(handle_id) {
        Some(s) => s,
        None => return String::new(),
    };
    let result = match store.read() {
        Ok(s) => match s.get(key) {
            Some(v) => serde_json::to_string(v).unwrap_or_default(),
            None => String::new(),
        },
        Err(_) => String::new(),
    };
    result
}

/// Set a config value from JSON. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn config_store_bindgen_set(handle_id: u32, key: &str, value_json: &str) -> u8 {
    let store = match get_store(handle_id) {
        Some(s) => s,
        None => return 0,
    };
    let value: serde_json::Value = match serde_json::from_str(value_json) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let result = match store.write() {
        Ok(mut s) => { s.set(key, value); 1 }
        Err(_) => 0,
    };
    result
}

/// Save user config to a TOML file. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn config_store_bindgen_save(handle_id: u32, path: &str) -> u8 {
    let store = match get_store(handle_id) {
        Some(s) => s,
        None => return 0,
    };
    let result = match store.read() {
        Ok(s) => if s.save_user_config(std::path::Path::new(path)).is_ok() { 1 } else { 0 },
        Err(_) => 0,
    };
    result
}

/// Reload config from disk. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn config_store_bindgen_reload(handle_id: u32) -> u8 {
    let store = match get_store(handle_id) {
        Some(s) => s,
        None => return 0,
    };
    let result = match store.write() {
        Ok(mut s) => if s.reload().is_ok() { 1 } else { 0 },
        Err(_) => 0,
    };
    result
}

/// Destroy a ConfigStore handle.
#[deno_bindgen]
fn config_store_bindgen_destroy(handle_id: u32) {
    handles().lock().unwrap_or_else(|e| e.into_inner()).remove(&handle_id);
}
