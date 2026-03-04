//! High-level deno_bindgen bindings for the config store.

use deno_bindgen::deno_bindgen;
use std::sync::{Arc, RwLock};

use marauder_event_bus::HandleRegistry;

use crate::store::ConfigStore;

static REGISTRY: HandleRegistry<Arc<RwLock<ConfigStore>>> = HandleRegistry::new();

fn get_store(handle_id: u32) -> Option<Arc<RwLock<ConfigStore>>> {
    REGISTRY.get_clone(handle_id)
}

/// Create a new ConfigStore with defaults. Returns a handle ID (0 on failure).
#[deno_bindgen]
fn config_store_bindgen_create() -> u32 {
    REGISTRY.allocate(Arc::new(RwLock::new(ConfigStore::new())))
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
    REGISTRY.remove(handle_id);
}
