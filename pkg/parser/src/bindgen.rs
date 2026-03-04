//! High-level deno_bindgen bindings for the VT parser.

use deno_bindgen::deno_bindgen;
use std::sync::{Arc, Mutex};

use marauder_event_bus::HandleRegistry;

use crate::performer::MarauderParser;

static REGISTRY: HandleRegistry<Arc<Mutex<MarauderParser>>> = HandleRegistry::new();

fn get_parser(handle_id: u32) -> Option<Arc<Mutex<MarauderParser>>> {
    REGISTRY.get_clone(handle_id)
}

/// Create a new parser. Returns a handle ID (0 on failure).
#[deno_bindgen]
fn parser_bindgen_create() -> u32 {
    REGISTRY.allocate(Arc::new(Mutex::new(MarauderParser::new())))
}

/// Feed input bytes and return parsed actions as a JSON array string.
#[deno_bindgen]
fn parser_bindgen_feed(handle_id: u32, input: &str) -> String {
    let parser = match get_parser(handle_id) {
        Some(p) => p,
        None => return "[]".to_string(),
    };
    let mut parser = parser.lock().unwrap_or_else(|e| e.into_inner());
    let mut actions = Vec::new();
    parser.feed(input.as_bytes(), |action| {
        actions.push(action);
    });
    serde_json::to_string(&actions).unwrap_or_else(|_| "[]".to_string())
}

/// Reset the parser to initial state.
#[deno_bindgen]
fn parser_bindgen_reset(handle_id: u32) {
    let parser = match get_parser(handle_id) {
        Some(p) => p,
        None => return,
    };
    let mut parser = parser.lock().unwrap_or_else(|e| e.into_inner());
    *parser = MarauderParser::new();
}

/// Destroy a parser handle.
#[deno_bindgen]
fn parser_bindgen_destroy(handle_id: u32) {
    REGISTRY.remove(handle_id);
}
