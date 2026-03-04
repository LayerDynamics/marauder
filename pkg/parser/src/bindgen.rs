//! High-level deno_bindgen bindings for the VT parser.

use deno_bindgen::deno_bindgen;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::performer::MarauderParser;

static HANDLES: OnceLock<Mutex<HashMap<u32, Arc<Mutex<MarauderParser>>>>> = OnceLock::new();
static NEXT_ID: OnceLock<Mutex<u32>> = OnceLock::new();

fn handles() -> &'static Mutex<HashMap<u32, Arc<Mutex<MarauderParser>>>> {
    HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u32 {
    let mut id = NEXT_ID.get_or_init(|| Mutex::new(1)).lock().unwrap();
    let val = *id;
    *id += 1;
    val
}

fn get_parser(handle_id: u32) -> Option<Arc<Mutex<MarauderParser>>> {
    handles().lock().unwrap().get(&handle_id).cloned()
}

/// Create a new parser. Returns a handle ID.
#[deno_bindgen]
fn parser_bindgen_create() -> u32 {
    let id = next_id();
    handles().lock().unwrap().insert(id, Arc::new(Mutex::new(MarauderParser::new())));
    id
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
    handles().lock().unwrap().remove(&handle_id);
}
