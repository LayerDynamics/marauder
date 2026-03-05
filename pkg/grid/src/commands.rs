//! Tauri command wrappers for grid operations.
//!
//! Grid commands take a `pane_id` since the webview knows pane IDs.
//! The `PaneGridMap` is shared state mapping pane IDs to grids.

use serde::{Deserialize, Serialize};

use marauder_event_bus::lock_or_log;

use crate::cell::Cell;
use crate::{PaneId, SharedGrid, PaneGridMap};

/// Grid dimensions returned to the webview.
#[derive(Debug, Serialize, Deserialize)]
pub struct GridDimensions {
    pub rows: usize,
    pub cols: usize,
}

/// Cursor position returned to the webview.
#[derive(Debug, Serialize, Deserialize)]
pub struct CursorPosition {
    pub row: usize,
    pub col: usize,
}

/// Full screen snapshot returned in a single IPC round-trip.
/// Each row is a Vec of Cells matching the grid's column count.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScreenSnapshot {
    pub rows: usize,
    pub cols: usize,
    pub cursor: CursorPosition,
    pub cells: Vec<Vec<Cell>>,
}

fn get_grid(
    state: &PaneGridMap,
    pane_id: PaneId,
) -> Result<SharedGrid, String> {
    let map = lock_or_log(state, "grid::cmd_get_grid");
    map.get(&pane_id)
        .cloned()
        .ok_or_else(|| format!("Pane {pane_id} not found"))
}

#[tauri::command]
pub fn grid_cmd_get_cursor(
    state: tauri::State<'_, PaneGridMap>,
    pane_id: PaneId,
) -> Result<CursorPosition, String> {
    let grid = get_grid(state.inner(), pane_id)?;
    let g = lock_or_log(&grid, "grid::cmd_get_cursor");
    Ok(CursorPosition {
        row: g.cursor.row,
        col: g.cursor.col,
    })
}

#[tauri::command]
pub fn grid_cmd_get_cell(
    state: tauri::State<'_, PaneGridMap>,
    pane_id: PaneId,
    row: usize,
    col: usize,
) -> Result<Cell, String> {
    let grid = get_grid(state.inner(), pane_id)?;
    let g = lock_or_log(&grid, "grid::cmd_get_cell");
    let screen = g.active_screen();
    if row >= screen.rows.len() || col >= screen.cols {
        return Err(format!("Cell ({row}, {col}) out of bounds"));
    }
    Ok(screen.rows[row][col])
}

#[tauri::command]
pub fn grid_cmd_get_selection_text(
    state: tauri::State<'_, PaneGridMap>,
    pane_id: PaneId,
) -> Result<Option<String>, String> {
    let grid = get_grid(state.inner(), pane_id)?;
    let g = lock_or_log(&grid, "grid::cmd_get_selection_text");
    Ok(g.get_selection_text())
}

#[tauri::command]
pub fn grid_cmd_set_selection(
    state: tauri::State<'_, PaneGridMap>,
    pane_id: PaneId,
    start_row: usize,
    start_col: usize,
    end_row: usize,
    end_col: usize,
) -> Result<(), String> {
    let grid = get_grid(state.inner(), pane_id)?;
    let mut g = lock_or_log(&grid, "grid::cmd_set_selection");
    g.set_selection(start_row, start_col, end_row, end_col);
    Ok(())
}

#[tauri::command]
pub fn grid_cmd_clear_selection(
    state: tauri::State<'_, PaneGridMap>,
    pane_id: PaneId,
) -> Result<(), String> {
    let grid = get_grid(state.inner(), pane_id)?;
    let mut g = lock_or_log(&grid, "grid::cmd_clear_selection");
    g.clear_selection();
    Ok(())
}

#[tauri::command]
pub fn grid_cmd_scroll_viewport(
    state: tauri::State<'_, PaneGridMap>,
    pane_id: PaneId,
    offset: usize,
) -> Result<(), String> {
    let grid = get_grid(state.inner(), pane_id)?;
    let mut g = lock_or_log(&grid, "grid::cmd_scroll_viewport");
    g.scroll_viewport(offset);
    Ok(())
}

#[tauri::command]
pub fn grid_cmd_scroll_viewport_by(
    state: tauri::State<'_, PaneGridMap>,
    pane_id: PaneId,
    delta: i64,
) -> Result<(), String> {
    let grid = get_grid(state.inner(), pane_id)?;
    let mut g = lock_or_log(&grid, "grid::cmd_scroll_viewport_by");
    g.scroll_viewport_by(delta);
    Ok(())
}

#[tauri::command]
pub fn grid_cmd_get_screen_snapshot(
    state: tauri::State<'_, PaneGridMap>,
    pane_id: PaneId,
) -> Result<ScreenSnapshot, String> {
    let grid = get_grid(state.inner(), pane_id)?;
    let g = lock_or_log(&grid, "grid::cmd_get_screen_snapshot");
    let screen = g.active_screen();
    let cells: Vec<Vec<Cell>> = screen.rows.iter().map(|row| row.to_vec()).collect();
    Ok(ScreenSnapshot {
        rows: screen.rows.len(),
        cols: screen.cols,
        cursor: CursorPosition {
            row: g.cursor.row,
            col: g.cursor.col,
        },
        cells,
    })
}

#[tauri::command]
pub fn grid_cmd_get_dimensions(
    state: tauri::State<'_, PaneGridMap>,
    pane_id: PaneId,
) -> Result<GridDimensions, String> {
    let grid = get_grid(state.inner(), pane_id)?;
    let g = lock_or_log(&grid, "grid::cmd_get_dimensions");
    Ok(GridDimensions {
        rows: g.rows(),
        cols: g.cols(),
    })
}
