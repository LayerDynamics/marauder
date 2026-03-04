//! High-level deno_bindgen bindings for the runtime.
//!
//! # Pane ID Convention
//! Pane IDs are always > 0. The value 0 is used as an error sentinel.
//! `PaneId` (u64) may exceed u32::MAX; callers should check for 0 (error).

use deno_bindgen::deno_bindgen;
use std::sync::{Arc, Mutex};

use marauder_event_bus::HandleRegistry;

use crate::config::RuntimeConfig;
use crate::lifecycle::MarauderRuntime;
use crate::util::lock_or_recover;

struct RuntimeEntry {
    runtime: Arc<Mutex<MarauderRuntime>>,
    tokio_rt: Arc<tokio::runtime::Runtime>,
}

static REGISTRY: HandleRegistry<RuntimeEntry> = HandleRegistry::new();

fn get_entry(handle_id: u32) -> Option<(Arc<Mutex<MarauderRuntime>>, Arc<tokio::runtime::Runtime>)> {
    REGISTRY.get(handle_id, |e| (Arc::clone(&e.runtime), Arc::clone(&e.tokio_rt)))
}

/// Create a new runtime with default config. Returns a handle ID, or 0 on error.
#[deno_bindgen]
fn runtime_bindgen_create() -> u32 {
    let tokio_rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return 0,
    };
    let entry = RuntimeEntry {
        runtime: Arc::new(Mutex::new(MarauderRuntime::new(RuntimeConfig::default()))),
        tokio_rt: Arc::new(tokio_rt),
    };
    REGISTRY.allocate(entry)
}

/// Boot the runtime. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn runtime_bindgen_boot(handle_id: u32) -> u8 {
    let (runtime, tokio_rt) = match get_entry(handle_id) {
        Some(e) => e,
        None => return 0,
    };
    // SAFETY: lock_or_recover acquires a std::sync::Mutex inside block_on.
    // This is safe because block_on runs synchronously on the calling thread,
    // and boot() operates on &mut MarauderRuntime directly without
    // re-acquiring this mutex. No spawned tasks access RuntimeHandle.runtime.
    match tokio_rt.block_on(async {
        let mut rt = lock_or_recover(&runtime, "runtime");
        rt.boot().await
    }) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Create a new pane. Returns the pane ID as string (to preserve u64 range), or empty on error.
#[deno_bindgen]
fn runtime_bindgen_create_pane(handle_id: u32) -> String {
    let (runtime, _tokio_rt) = match get_entry(handle_id) {
        Some(e) => e,
        None => return String::new(),
    };
    let mut rt = lock_or_recover(&runtime, "runtime");
    match rt.create_pane() {
        Ok(id) => id.to_string(),
        Err(_) => String::new(),
    }
}

/// Close a pane. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn runtime_bindgen_close_pane(handle_id: u32, pane_id: &str) -> u8 {
    let pane_id: u64 = match pane_id.parse() {
        Ok(id) => id,
        Err(_) => return 0,
    };
    let (runtime, _tokio_rt) = match get_entry(handle_id) {
        Some(e) => e,
        None => return 0,
    };
    let mut rt = lock_or_recover(&runtime, "runtime");
    match rt.close_pane(pane_id) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Write data to a pane. Returns bytes written as string, or empty on error.
#[deno_bindgen]
fn runtime_bindgen_write(handle_id: u32, pane_id: &str, data: &str) -> String {
    let pane_id: u64 = match pane_id.parse() {
        Ok(id) => id,
        Err(_) => return String::new(),
    };
    let (runtime, _tokio_rt) = match get_entry(handle_id) {
        Some(e) => e,
        None => return String::new(),
    };
    let rt = lock_or_recover(&runtime, "runtime");
    match rt.write_to_pane(pane_id, data.as_bytes()) {
        Ok(n) => n.to_string(),
        Err(_) => String::new(),
    }
}

/// Resize a pane. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn runtime_bindgen_resize_pane(handle_id: u32, pane_id: &str, rows: u16, cols: u16) -> u8 {
    let pane_id: u64 = match pane_id.parse() {
        Ok(id) => id,
        Err(_) => return 0,
    };
    let (runtime, _tokio_rt) = match get_entry(handle_id) {
        Some(e) => e,
        None => return 0,
    };
    let mut rt = lock_or_recover(&runtime, "runtime");
    match rt.resize_pane(pane_id, rows, cols) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Get the number of active panes.
#[deno_bindgen]
fn runtime_bindgen_pane_count(handle_id: u32) -> u32 {
    let (runtime, _tokio_rt) = match get_entry(handle_id) {
        Some(e) => e,
        None => return 0,
    };
    let rt = lock_or_recover(&runtime, "runtime");
    rt.pane_ids().len() as u32
}

/// Shutdown the runtime. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn runtime_bindgen_shutdown(handle_id: u32) -> u8 {
    let (runtime, tokio_rt) = match get_entry(handle_id) {
        Some(e) => e,
        None => return 0,
    };
    // SAFETY: lock_or_recover acquires a std::sync::Mutex inside block_on.
    // This is safe because block_on runs synchronously on the calling thread,
    // and shutdown() operates on &mut MarauderRuntime directly without
    // re-acquiring this mutex. No spawned tasks access RuntimeHandle.runtime.
    match tokio_rt.block_on(async {
        let mut rt = lock_or_recover(&runtime, "runtime");
        rt.shutdown().await
    }) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Destroy a runtime handle. Callers should call shutdown first.
#[deno_bindgen]
fn runtime_bindgen_destroy(handle_id: u32) {
    REGISTRY.remove(handle_id);
}
