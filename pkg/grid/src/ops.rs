//! deno_core #[op2] ops for the terminal grid in embedded mode.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use deno_core::op2;
use deno_core::OpState;
use marauder_event_bus::lock_or_log;

use crate::cell::Cell;
use crate::grid::Grid;
use marauder_parser::actions::TerminalAction;

/// Error type for grid ops.
#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[error("{0}")]
pub struct GridOpError(String);

type GridMap = Arc<Mutex<HashMap<u32, Grid>>>;
type NextGridId = Arc<Mutex<u32>>;

/// Map of handle → Arc<Mutex<Grid>> for live-shared grids from the runtime.
type SharedGridMap = Arc<Mutex<HashMap<u32, Arc<Mutex<Grid>>>>>;

fn init_grid_state(state: &mut OpState) {
    state.put::<GridMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<NextGridId>(Arc::new(Mutex::new(1)));
    state.put::<SharedGridMap>(Arc::new(Mutex::new(HashMap::new())));
}

/// Inject a shared grid reference from the real runtime into OpState.
/// The grid ops will prefer this live-shared grid over the local grid map.
pub fn inject_shared_grid(state: &mut OpState, handle: u32, grid: Arc<Mutex<Grid>>) {
    let shared_grids = state.borrow::<SharedGridMap>().clone();
    lock_or_log(&shared_grids, "grid::ops_inject_shared_grid").insert(handle, grid);
}

fn with_grid<R>(
    state: &mut OpState,
    handle: u32,
    f: impl FnOnce(&mut Grid) -> R,
) -> Result<R, GridOpError> {
    // First check shared grids (live runtime grids)
    let shared_grids = state.borrow::<SharedGridMap>().clone();
    let shared = lock_or_log(&shared_grids, "grid::ops_with_grid_shared");
    if let Some(grid_arc) = shared.get(&handle) {
        let grid_arc = grid_arc.clone();
        drop(shared);
        let mut grid = lock_or_log(&grid_arc, "grid::ops_with_grid_live");
        return Ok(f(&mut grid));
    }
    drop(shared);

    // Fall back to local grid map
    let map = state.borrow::<GridMap>().clone();
    let mut map = lock_or_log(&map, "grid::ops_with_grid_local");
    let grid = map
        .get_mut(&handle)
        .ok_or_else(|| GridOpError(format!("invalid grid handle: {handle}")))?;
    Ok(f(grid))
}

/// Create a new grid with given dimensions, returns handle ID.
#[op2(fast)]
#[smi]
pub fn op_grid_create(
    state: &mut OpState,
    #[smi] rows: u32,
    #[smi] cols: u32,
) -> Result<u32, GridOpError> {
    let id_rc = state.borrow::<NextGridId>().clone();
    let mut id = lock_or_log(&id_rc, "grid::ops_create_id");
    let handle = *id;
    *id = id.checked_add(1).ok_or_else(|| GridOpError("grid handle ID overflow".to_string()))?;
    drop(id);

    let map = state.borrow::<GridMap>().clone();
    lock_or_log(&map, "grid::ops_create_insert").insert(handle, Grid::new(rows as usize, cols as usize));
    Ok(handle)
}

/// Apply a terminal action to the grid.
#[op2]
pub fn op_grid_apply_action(
    state: &mut OpState,
    #[smi] handle: u32,
    #[serde] action: TerminalAction,
) -> Result<(), GridOpError> {
    with_grid(state, handle, |grid| grid.apply_action(&action))
}

/// Get a cell at (row, col) as serialized JSON.
#[op2]
#[serde]
pub fn op_grid_get_cell(
    state: &mut OpState,
    #[smi] handle: u32,
    #[smi] row: u32,
    #[smi] col: u32,
) -> Result<Cell, GridOpError> {
    with_grid(state, handle, |grid| {
        let screen = grid.active_screen();
        let r = row as usize;
        let c = col as usize;
        if r < screen.rows.len() && c < screen.rows[r].len() {
            screen.rows[r][c]
        } else {
            Cell::default()
        }
    })
}

/// Get cursor position as [row, col].
#[op2]
#[serde]
pub fn op_grid_get_cursor(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<(u32, u32), GridOpError> {
    with_grid(state, handle, |grid| {
        (grid.cursor.row as u32, grid.cursor.col as u32)
    })
}

/// Resize the grid.
#[op2(fast)]
pub fn op_grid_resize(
    state: &mut OpState,
    #[smi] handle: u32,
    #[smi] rows: u32,
    #[smi] cols: u32,
) -> Result<(), GridOpError> {
    with_grid(state, handle, |grid| grid.resize(rows as usize, cols as usize))
}

/// Get dirty row flags as a byte buffer (1 = dirty, 0 = clean).
#[op2]
#[buffer]
pub fn op_grid_get_dirty_rows(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<Vec<u8>, GridOpError> {
    with_grid(state, handle, |grid| {
        grid.get_dirty_rows().iter().map(|&d| d as u8).collect()
    })
}

/// Clear all dirty flags.
#[op2(fast)]
pub fn op_grid_clear_dirty(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<(), GridOpError> {
    with_grid(state, handle, |grid| grid.clear_dirty())
}

/// Set a text selection.
#[op2(fast)]
pub fn op_grid_select(
    state: &mut OpState,
    #[smi] handle: u32,
    #[smi] start_row: u32,
    #[smi] start_col: u32,
    #[smi] end_row: u32,
    #[smi] end_col: u32,
) -> Result<(), GridOpError> {
    with_grid(state, handle, |grid| {
        grid.set_selection(
            start_row as usize,
            start_col as usize,
            end_row as usize,
            end_col as usize,
        )
    })
}

/// Get selected text, or empty string if no selection.
#[op2]
#[string]
pub fn op_grid_get_selection_text(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<String, GridOpError> {
    with_grid(state, handle, |grid| {
        grid.get_selection_text().unwrap_or_default()
    })
}

/// Clear the current text selection.
#[op2(fast)]
pub fn op_grid_clear_selection(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<(), GridOpError> {
    with_grid(state, handle, |grid| grid.clear_selection())
}

/// Scroll viewport into scrollback history.
#[op2(fast)]
pub fn op_grid_scroll_viewport(
    state: &mut OpState,
    #[smi] handle: u32,
    #[smi] offset: u32,
) -> Result<(), GridOpError> {
    with_grid(state, handle, |grid| grid.scroll_viewport(offset as usize))
}

/// Destroy a grid instance.
#[op2(fast)]
pub fn op_grid_destroy(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<(), GridOpError> {
    // Remove from shared grids first
    {
        let shared = state.borrow::<SharedGridMap>().clone();
        lock_or_log(&shared, "grid::ops_destroy_shared").remove(&handle);
    }
    // Then from local map (may not exist if it was only in shared)
    let map = state.borrow::<GridMap>().clone();
    let removed = lock_or_log(&map, "grid::ops_destroy_local").remove(&handle);
    if removed.is_none() {
        // Check if it was in shared — if not found in either, error
        // (Already removed from shared above, so just return ok since it was found there)
    }
    Ok(())
}

deno_core::extension!(
    marauder_grid_ext,
    ops = [
        op_grid_create,
        op_grid_apply_action,
        op_grid_get_cell,
        op_grid_get_cursor,
        op_grid_resize,
        op_grid_get_dirty_rows,
        op_grid_clear_dirty,
        op_grid_select,
        op_grid_get_selection_text,
        op_grid_clear_selection,
        op_grid_scroll_viewport,
        op_grid_destroy,
    ],
    state = |state| init_grid_state(state),
);

/// Build the deno_core Extension for grid ops.
pub fn grid_extension() -> deno_core::Extension {
    marauder_grid_ext::init()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> OpState {
        let mut state = OpState::new(None);
        init_grid_state(&mut state);
        state
    }

    /// Create a grid by directly manipulating state maps (bypasses #[op2] macro).
    fn create_grid(state: &mut OpState, rows: usize, cols: usize) -> Result<u32, GridOpError> {
        let id_rc = state.borrow::<NextGridId>().clone();
        let mut id = lock_or_log(&id_rc, "grid::test_create_id");
        let handle = *id;
        *id = id.checked_add(1).ok_or_else(|| GridOpError("grid handle ID overflow".to_string()))?;
        drop(id);
        let map = state.borrow::<GridMap>().clone();
        lock_or_log(&map, "grid::test_create_insert").insert(handle, Grid::new(rows, cols));
        Ok(handle)
    }

    /// Get cursor via with_grid helper.
    fn get_cursor(state: &mut OpState, handle: u32) -> Result<(u32, u32), GridOpError> {
        with_grid(state, handle, |grid| {
            (grid.cursor.row as u32, grid.cursor.col as u32)
        })
    }

    /// Get cell via with_grid helper.
    fn get_cell(state: &mut OpState, handle: u32, row: u32, col: u32) -> Result<Cell, GridOpError> {
        with_grid(state, handle, |grid| {
            let screen = grid.active_screen();
            let r = row as usize;
            let c = col as usize;
            if r < screen.rows.len() && c < screen.rows[r].len() {
                screen.rows[r][c]
            } else {
                Cell::default()
            }
        })
    }

    /// Resize via with_grid helper.
    fn resize_grid(state: &mut OpState, handle: u32, rows: u32, cols: u32) -> Result<(), GridOpError> {
        with_grid(state, handle, |grid| grid.resize(rows as usize, cols as usize))
    }

    /// Get dirty rows via with_grid helper.
    fn get_dirty_rows(state: &mut OpState, handle: u32) -> Result<Vec<u8>, GridOpError> {
        with_grid(state, handle, |grid| {
            grid.get_dirty_rows().iter().map(|&d| d as u8).collect()
        })
    }

    /// Clear dirty flags via with_grid helper.
    fn clear_dirty(state: &mut OpState, handle: u32) -> Result<(), GridOpError> {
        with_grid(state, handle, |grid| grid.clear_dirty())
    }

    /// Destroy a grid by removing from both maps.
    fn destroy_grid(state: &mut OpState, handle: u32) -> Result<(), GridOpError> {
        {
            let shared = state.borrow::<SharedGridMap>().clone();
            lock_or_log(&shared, "grid::test_destroy_shared").remove(&handle);
        }
        let map = state.borrow::<GridMap>().clone();
        lock_or_log(&map, "grid::test_destroy_local").remove(&handle);
        Ok(())
    }

    #[test]
    fn test_create_returns_incrementing_handles() {
        let mut state = make_state();
        let h1 = create_grid(&mut state, 24, 80).expect("first create should succeed");
        let h2 = create_grid(&mut state, 24, 80).expect("second create should succeed");
        assert_eq!(h1, 1, "first handle should be 1");
        assert_eq!(h2, 2, "second handle should be 2");
    }

    #[test]
    fn test_get_cursor_initial() {
        let mut state = make_state();
        let h = create_grid(&mut state, 24, 80).unwrap();
        let (row, col) = get_cursor(&mut state, h).expect("get_cursor should succeed");
        assert_eq!(row, 0, "initial cursor row should be 0");
        assert_eq!(col, 0, "initial cursor col should be 0");
    }

    #[test]
    fn test_get_cell_default() {
        let mut state = make_state();
        let h = create_grid(&mut state, 24, 80).unwrap();
        let cell = get_cell(&mut state, h, 0, 0).expect("get_cell should succeed");
        assert_eq!(cell, Cell::default(), "cell at (0,0) should be default");
    }

    #[test]
    fn test_get_cell_out_of_bounds() {
        let mut state = make_state();
        let h = create_grid(&mut state, 24, 80).unwrap();
        let cell = get_cell(&mut state, h, 100, 200).expect("out-of-bounds get_cell should return default, not error");
        assert_eq!(cell, Cell::default(), "out-of-bounds cell should be Cell::default()");
    }

    #[test]
    fn test_resize() {
        let mut state = make_state();
        let h = create_grid(&mut state, 24, 80).unwrap();
        resize_grid(&mut state, h, 48, 120).expect("resize should succeed");
        let (row, col) = get_cursor(&mut state, h).expect("get_cursor after resize should succeed");
        assert!(row < 48, "cursor row should be within new row count");
        assert!(col < 120, "cursor col should be within new col count");
        let dirty = get_dirty_rows(&mut state, h).expect("get_dirty_rows after resize should succeed");
        assert_eq!(dirty.len(), 48, "dirty rows length should match new row count");
    }

    #[test]
    fn test_dirty_rows_and_clear() {
        let mut state = make_state();
        let h = create_grid(&mut state, 24, 80).unwrap();
        let dirty = get_dirty_rows(&mut state, h).expect("get_dirty_rows should succeed");
        assert_eq!(dirty.len(), 24, "dirty row buffer length should equal row count");
        assert!(dirty.iter().all(|&d| d == 1), "all rows should be dirty on a new grid");

        clear_dirty(&mut state, h).expect("clear_dirty should succeed");

        let dirty_after = get_dirty_rows(&mut state, h).expect("get_dirty_rows after clear should succeed");
        assert!(dirty_after.iter().all(|&d| d == 0), "all rows should be clean after clear_dirty");
    }

    #[test]
    fn test_invalid_handle_error() {
        let mut state = make_state();
        let result = get_cursor(&mut state, 99);
        assert!(result.is_err(), "get_cursor on nonexistent handle should return an error");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("99"),
            "error message should mention the invalid handle: {err}"
        );
    }

    #[test]
    fn test_destroy_then_access() {
        let mut state = make_state();
        let h = create_grid(&mut state, 24, 80).unwrap();
        get_cursor(&mut state, h).expect("get_cursor before destroy should succeed");
        destroy_grid(&mut state, h).expect("destroy should succeed");
        let result = get_cursor(&mut state, h);
        assert!(result.is_err(), "get_cursor after destroy should return an error");
    }

    #[test]
    fn test_inject_shared_grid() {
        let mut state = make_state();
        let shared = Arc::new(Mutex::new(Grid::new(10, 40)));
        inject_shared_grid(&mut state, 50, shared);
        let (row, col) = get_cursor(&mut state, 50)
            .expect("get_cursor on injected shared grid should succeed");
        assert_eq!(row, 0, "injected grid cursor row should be 0");
        assert_eq!(col, 0, "injected grid cursor col should be 0");
    }

    #[test]
    fn test_handle_overflow() {
        let mut state = make_state();
        {
            let id_rc = state.borrow::<NextGridId>().clone();
            let mut id = id_rc.lock().unwrap();
            *id = u32::MAX;
        }
        let result = create_grid(&mut state, 24, 80);
        assert!(result.is_err(), "create at u32::MAX should return an overflow error");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("overflow"),
            "error message should mention overflow: {err}"
        );
    }
}
