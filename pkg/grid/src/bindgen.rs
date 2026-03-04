//! High-level deno_bindgen bindings for the terminal grid.

use deno_bindgen::deno_bindgen;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::grid::Grid;
use marauder_parser::TerminalAction;

static HANDLES: OnceLock<Mutex<HashMap<u32, Arc<Mutex<Grid>>>>> = OnceLock::new();
static NEXT_ID: OnceLock<Mutex<u32>> = OnceLock::new();

fn handles() -> &'static Mutex<HashMap<u32, Arc<Mutex<Grid>>>> {
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

fn get_grid(handle_id: u32) -> Option<Arc<Mutex<Grid>>> {
    handles().lock().unwrap_or_else(|e| e.into_inner()).get(&handle_id).cloned()
}

/// Create a new grid. Returns a handle ID.
#[deno_bindgen]
fn grid_bindgen_create(rows: u32, cols: u32) -> u32 {
    let id = next_id();
    if id == 0 {
        return 0; // overflow sentinel — caller must treat 0 as invalid
    }
    handles().lock().unwrap_or_else(|e| e.into_inner()).insert(id, Arc::new(Mutex::new(Grid::new(rows as usize, cols as usize))));
    id
}

/// Apply a terminal action (JSON string). Returns 1 on success, 0 on error.
#[deno_bindgen]
fn grid_bindgen_apply_action(handle_id: u32, action_json: &str) -> u8 {
    let grid = match get_grid(handle_id) {
        Some(g) => g,
        None => return 0,
    };
    let action: TerminalAction = match serde_json::from_str(action_json) {
        Ok(a) => a,
        Err(_) => return 0,
    };
    let mut grid = grid.lock().unwrap_or_else(|e| e.into_inner());
    grid.apply_action(&action);
    1
}

/// Get cell as JSON string.
#[deno_bindgen]
fn grid_bindgen_get_cell(handle_id: u32, row: u32, col: u32) -> String {
    let grid = match get_grid(handle_id) {
        Some(g) => g,
        None => return String::new(),
    };
    let grid = grid.lock().unwrap_or_else(|e| e.into_inner());
    let screen = grid.active_screen();
    let (r, c) = (row as usize, col as usize);
    if r >= screen.rows.len() || c >= screen.cols {
        return String::new();
    }
    serde_json::to_string(&screen.rows[r][c]).unwrap_or_default()
}

/// Get cursor as "row,col".
#[deno_bindgen]
fn grid_bindgen_get_cursor(handle_id: u32) -> String {
    let grid = match get_grid(handle_id) {
        Some(g) => g,
        None => return String::new(),
    };
    let grid = grid.lock().unwrap_or_else(|e| e.into_inner());
    format!("{},{}", grid.cursor.row, grid.cursor.col)
}

/// Resize the grid.
#[deno_bindgen]
fn grid_bindgen_resize(handle_id: u32, rows: u32, cols: u32) {
    let grid = match get_grid(handle_id) {
        Some(g) => g,
        None => return,
    };
    let mut grid = grid.lock().unwrap_or_else(|e| e.into_inner());
    grid.resize(rows as usize, cols as usize);
}

/// Get selection text.
#[deno_bindgen]
fn grid_bindgen_get_selection_text(handle_id: u32) -> String {
    let grid = match get_grid(handle_id) {
        Some(g) => g,
        None => return String::new(),
    };
    let grid = grid.lock().unwrap_or_else(|e| e.into_inner());
    grid.get_selection_text().unwrap_or_default()
}

/// Set selection. Pass u32::MAX for all params to clear.
#[deno_bindgen]
fn grid_bindgen_select(handle_id: u32, start_row: u32, start_col: u32, end_row: u32, end_col: u32) {
    let grid = match get_grid(handle_id) {
        Some(g) => g,
        None => return,
    };
    let mut grid = grid.lock().unwrap_or_else(|e| e.into_inner());
    if start_row == u32::MAX && end_row == u32::MAX {
        grid.clear_selection();
    } else {
        grid.set_selection(
            start_row as usize, start_col as usize,
            end_row as usize, end_col as usize,
        );
    }
}

/// Destroy a grid handle.
#[deno_bindgen]
fn grid_bindgen_destroy(handle_id: u32) {
    handles().lock().unwrap_or_else(|e| e.into_inner()).remove(&handle_id);
}
