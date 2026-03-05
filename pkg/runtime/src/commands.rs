//! Tauri command wrappers for runtime operations.

use std::sync::{Arc, Mutex, OnceLock};

use marauder_event_bus::lock_or_log;
use marauder_grid::PaneGridMap;

use crate::lifecycle::MarauderRuntime;

/// Tauri managed state wrapping the runtime.
///
/// Uses `OnceLock` because the runtime is initialized asynchronously
/// in the setup thread after Tauri state registration. Once set, access
/// is lock-free (no outer Mutex needed).
pub struct TauriRuntimeHandle {
    pub inner: Arc<OnceLock<Arc<Mutex<MarauderRuntime>>>>,
}

impl TauriRuntimeHandle {
    pub fn new(runtime: Arc<OnceLock<Arc<Mutex<MarauderRuntime>>>>) -> Self {
        Self { inner: runtime }
    }
}

fn with_runtime<F, T>(state: &TauriRuntimeHandle, f: F) -> Result<T, String>
where
    F: FnOnce(&mut MarauderRuntime) -> Result<T, String>,
{
    let rt_arc = state.inner.get().ok_or("Runtime not initialized")?;
    let mut rt = lock_or_log(&rt_arc, "runtime::cmd with_runtime");
    f(&mut rt)
}

#[tauri::command]
pub fn runtime_cmd_state(
    state: tauri::State<'_, TauriRuntimeHandle>,
) -> Result<String, String> {
    match state.inner.get() {
        Some(rt_arc) => {
            let rt = lock_or_log(&rt_arc, "runtime::cmd_state");
            Ok(format!("{:?}", rt.state()))
        }
        None => Ok("NotInitialized".to_string()),
    }
}

#[tauri::command]
pub fn runtime_cmd_pane_ids(
    state: tauri::State<'_, TauriRuntimeHandle>,
) -> Result<Vec<u64>, String> {
    with_runtime(&state, |rt| Ok(rt.pane_ids()))
}

/// Create a new pane and **synchronously** register its grid in PaneGridMap.
///
/// This eliminates the race window where the command returns a pane ID but
/// the grid isn't queryable yet (previously depended on an async PaneCreated
/// event subscriber). It also avoids a deadlock: the event bus publish inside
/// `create_pane` fires synchronously while the runtime Mutex is held, so a
/// subscriber that re-acquires the runtime lock would deadlock.
#[tauri::command]
pub fn runtime_cmd_create_pane(
    state: tauri::State<'_, TauriRuntimeHandle>,
    pane_grids: tauri::State<'_, PaneGridMap>,
) -> Result<u64, String> {
    with_runtime(&state, |rt| {
        let pane_id = rt.create_pane().map_err(|e| e.to_string())?;

        // Synchronously register the pipeline's grid so it's immediately
        // queryable by grid_cmd_* commands when this call returns.
        if let Some(pipeline) = rt.pipeline(pane_id) {
            lock_or_log(&pane_grids, "runtime::cmd_create_pane grid_insert")
                .insert(pane_id, Arc::clone(&pipeline.grid));
        }

        Ok(pane_id)
    })
}

/// Close a pane and **synchronously** remove its grid from PaneGridMap.
#[tauri::command]
pub fn runtime_cmd_close_pane(
    state: tauri::State<'_, TauriRuntimeHandle>,
    pane_grids: tauri::State<'_, PaneGridMap>,
    pane_id: u64,
) -> Result<(), String> {
    with_runtime(&state, |rt| {
        rt.close_pane(pane_id).map_err(|e| e.to_string())?;

        // Remove the grid entry so stale pane IDs don't linger.
        lock_or_log(&pane_grids, "runtime::cmd_close_pane grid_remove")
            .remove(&pane_id);

        Ok(())
    })
}
