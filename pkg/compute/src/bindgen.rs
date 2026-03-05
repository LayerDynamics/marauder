//! High-level deno_bindgen bindings for the GPU compute engine.

use deno_bindgen::deno_bindgen;
use std::sync::{Arc, Mutex};

use marauder_event_bus::{lock_or_log, HandleRegistry};

use crate::engine::ComputeEngine;
use crate::types::GpuCell;

static REGISTRY: HandleRegistry<Arc<Mutex<ComputeEngine>>> = HandleRegistry::new();

fn get_engine(handle_id: u32) -> Option<Arc<Mutex<ComputeEngine>>> {
    REGISTRY.get_clone(handle_id)
}

/// Create a standalone compute engine. Returns a handle ID (0 on failure).
#[deno_bindgen]
fn compute_bindgen_create() -> u32 {
    match ComputeEngine::new_standalone() {
        Ok(engine) => REGISTRY.allocate(Arc::new(Mutex::new(engine))),
        Err(_) => 0,
    }
}

/// Upload cell data as JSON array of GpuCell objects. Returns 1 on success, 0 on error.
#[deno_bindgen]
fn compute_bindgen_upload_cells(handle_id: u32, cells_json: &str, rows: u32, cols: u32) -> u8 {
    let engine = match get_engine(handle_id) {
        Some(e) => e,
        None => return 0,
    };
    let cells: Vec<GpuCell> = match serde_json::from_str(cells_json) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let mut engine = lock_or_log(&engine, "compute::bindgen");
    engine.upload_cells_raw(&cells, rows, cols);
    1
}

/// Search for a pattern. Returns JSON array of SearchResult.
#[deno_bindgen]
fn compute_bindgen_search(handle_id: u32, pattern: &str) -> String {
    let engine = match get_engine(handle_id) {
        Some(e) => e,
        None => return "[]".to_string(),
    };
    let engine = lock_or_log(&engine, "compute::bindgen");
    match engine.search(pattern) {
        Ok(results) => serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()),
        Err(_) => "[]".to_string(),
    }
}

/// Detect URLs in a row range. Returns JSON array of UrlMatch.
#[deno_bindgen]
fn compute_bindgen_detect_urls(handle_id: u32, row_start: u32, row_end: u32) -> String {
    let engine = match get_engine(handle_id) {
        Some(e) => e,
        None => return "[]".to_string(),
    };
    let engine = lock_or_log(&engine, "compute::bindgen");
    match engine.detect_urls(row_start, row_end) {
        Ok(results) => serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()),
        Err(_) => "[]".to_string(),
    }
}

/// Classify cells for highlighting. Returns JSON array of HighlightResult.
#[deno_bindgen]
fn compute_bindgen_highlight_cells(handle_id: u32) -> String {
    let engine = match get_engine(handle_id) {
        Some(e) => e,
        None => return "[]".to_string(),
    };
    let engine = lock_or_log(&engine, "compute::bindgen");
    match engine.highlight_cells() {
        Ok(results) => serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()),
        Err(_) => "[]".to_string(),
    }
}

/// Extract selection text from a range.
#[deno_bindgen]
fn compute_bindgen_extract_selection(
    handle_id: u32,
    start_row: u32,
    start_col: u32,
    end_row: u32,
    end_col: u32,
) -> String {
    let engine = match get_engine(handle_id) {
        Some(e) => e,
        None => return String::new(),
    };
    let engine = lock_or_log(&engine, "compute::bindgen");
    match engine.extract_selection(start_row, start_col, end_row, end_col) {
        Ok(text) => text,
        Err(_) => String::new(),
    }
}

/// Destroy a compute engine handle.
#[deno_bindgen]
fn compute_bindgen_destroy(handle_id: u32) {
    REGISTRY.remove(handle_id);
}
