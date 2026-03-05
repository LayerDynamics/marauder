//! deno_core #[op2] ops for the Marauder runtime in embedded mode.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use deno_core::op2;
use deno_core::OpState;
use marauder_event_bus::lock_or_log;

use crate::config::RuntimeConfig;
use crate::lifecycle::{MarauderRuntime, RuntimeState};

/// Error type for runtime ops.
#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[error("{0}")]
pub struct RuntimeOpError(String);

impl From<crate::error::RuntimeError> for RuntimeOpError {
    fn from(e: crate::error::RuntimeError) -> Self {
        Self(e.to_string())
    }
}

type RuntimeMap = Arc<Mutex<HashMap<u32, Arc<Mutex<MarauderRuntime>>>>>;
type NextRuntimeId = Arc<Mutex<u32>>;

/// Tracks whether the primary runtime is attached (managed by Rust, not JS).
type PrimaryAttached = Arc<Mutex<bool>>;

fn init_runtime_state(state: &mut OpState) {
    state.put::<RuntimeMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<NextRuntimeId>(Arc::new(Mutex::new(1)));
    state.put::<PrimaryAttached>(Arc::new(Mutex::new(false)));
}

/// Mark the primary runtime as attached. JS should use subsystem ops
/// (pty, grid, parser, event-bus, config) rather than creating a new runtime.
pub fn mark_primary_attached(state: &mut OpState) {
    let attached = state.borrow::<PrimaryAttached>().clone();
    *lock_or_log(&attached, "runtime::mark_primary_attached") = true;
}

fn with_runtime<R>(
    state: &mut OpState,
    handle: u32,
    f: impl FnOnce(&mut MarauderRuntime) -> R,
) -> Result<R, RuntimeOpError> {
    let map = state.borrow::<RuntimeMap>().clone();
    let map = lock_or_log(&map, "runtime::with_runtime map");
    let rt_arc = map
        .get(&handle)
        .cloned()
        .ok_or_else(|| RuntimeOpError(format!("invalid runtime handle: {handle}")))?;
    drop(map);
    let mut rt = lock_or_log(&rt_arc, "runtime::with_runtime instance");
    Ok(f(&mut rt))
}

// ── impl functions (called by #[op2] wrappers and by tests) ──────────────────

fn runtime_create_impl(state: &mut OpState) -> Result<u32, RuntimeOpError> {
    let id_rc = state.borrow::<NextRuntimeId>().clone();
    let mut id = lock_or_log(&id_rc, "runtime::create next_id");
    let handle = *id;
    *id = id.checked_add(1).ok_or_else(|| RuntimeOpError("runtime handle ID overflow".to_string()))?;
    drop(id);

    let map = state.borrow::<RuntimeMap>().clone();
    lock_or_log(&map, "runtime::create insert")
        .insert(handle, Arc::new(Mutex::new(MarauderRuntime::new(RuntimeConfig::default()))));
    Ok(handle)
}

fn runtime_is_primary_attached_impl(state: &mut OpState) -> u32 {
    let attached = state.borrow::<PrimaryAttached>().clone();
    if *lock_or_log(&attached, "runtime::is_primary_attached") { 1 } else { 0 }
}

fn runtime_create_pane_impl(state: &mut OpState, handle: u32) -> Result<u32, RuntimeOpError> {
    with_runtime(state, handle, |rt| {
        rt.create_pane().map(|id| id as u32).map_err(RuntimeOpError::from)
    })?
}

fn runtime_close_pane_impl(state: &mut OpState, handle: u32, pane_id: u32) -> Result<(), RuntimeOpError> {
    with_runtime(state, handle, |rt| {
        rt.close_pane(pane_id as u64).map_err(RuntimeOpError::from)
    })?
}

fn runtime_write_to_pane_impl(state: &mut OpState, handle: u32, pane_id: u32, data: &[u8]) -> Result<(), RuntimeOpError> {
    with_runtime(state, handle, |rt| {
        rt.write_to_pane(pane_id as u64, data)
            .map(|_| ())
            .map_err(RuntimeOpError::from)
    })?
}

fn runtime_resize_pane_impl(state: &mut OpState, handle: u32, pane_id: u32, rows: u32, cols: u32) -> Result<(), RuntimeOpError> {
    with_runtime(state, handle, |rt| {
        rt.resize_pane(pane_id as u64, rows as u16, cols as u16)
            .map_err(RuntimeOpError::from)
    })?
}

fn runtime_pane_ids_impl(state: &mut OpState, handle: u32) -> Result<Vec<u32>, RuntimeOpError> {
    with_runtime(state, handle, |rt| {
        rt.pane_ids().into_iter().map(|id| id as u32).collect()
    })
}

fn runtime_state_impl(state: &mut OpState, handle: u32) -> Result<String, RuntimeOpError> {
    with_runtime(state, handle, |rt| {
        match rt.state() {
            RuntimeState::Created => "created".to_string(),
            RuntimeState::Running => "running".to_string(),
            RuntimeState::ShuttingDown => "shutting_down".to_string(),
            RuntimeState::Stopped => "stopped".to_string(),
        }
    })
}

fn runtime_destroy_impl(state: &mut OpState, handle: u32) -> Result<(), RuntimeOpError> {
    let map = state.borrow::<RuntimeMap>().clone();
    lock_or_log(&map, "runtime::destroy")
        .remove(&handle)
        .ok_or_else(|| RuntimeOpError(format!("invalid runtime handle: {handle}")))?;
    // The Arc<Mutex<MarauderRuntime>> is dropped here, releasing the runtime
    // if no other references exist.
    Ok(())
}

// ── #[op2] wrappers ──────────────────────────────────────────────────────────

/// Create a new runtime with default config, returns handle ID.
#[op2(fast)]
#[smi]
pub fn op_runtime_create(state: &mut OpState) -> Result<u32, RuntimeOpError> {
    runtime_create_impl(state)
}

/// Check if the primary runtime is attached (managed by Rust).
/// Returns 1 if attached, 0 if not.
#[op2(fast)]
#[smi]
pub fn op_runtime_is_primary_attached(state: &mut OpState) -> u32 {
    runtime_is_primary_attached_impl(state)
}

/// Boot the runtime (async).
#[op2(async)]
pub async fn op_runtime_boot(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
) -> Result<(), RuntimeOpError> {
    let rt_arc = {
        let state = state.borrow();
        let map = state.borrow::<RuntimeMap>().clone();
        let map = lock_or_log(&map, "runtime::boot map");
        map.get(&handle)
            .cloned()
            .ok_or_else(|| RuntimeOpError(format!("invalid runtime handle: {handle}")))?
    };
    let mut rt = lock_or_log(&rt_arc, "runtime::boot instance");
    rt.boot().await.map_err(RuntimeOpError::from)
}

/// Shutdown the runtime (async).
#[op2(async)]
pub async fn op_runtime_shutdown(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u32,
) -> Result<(), RuntimeOpError> {
    let rt_arc = {
        let state = state.borrow();
        let map = state.borrow::<RuntimeMap>().clone();
        let map = lock_or_log(&map, "runtime::shutdown map");
        map.get(&handle)
            .cloned()
            .ok_or_else(|| RuntimeOpError(format!("invalid runtime handle: {handle}")))?
    };
    let mut rt = lock_or_log(&rt_arc, "runtime::shutdown instance");
    rt.shutdown().await.map_err(RuntimeOpError::from)
}

/// Create a new pane, returns pane ID.
#[op2(fast)]
#[smi]
pub fn op_runtime_create_pane(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<u32, RuntimeOpError> {
    runtime_create_pane_impl(state, handle)
}

/// Close a pane by ID.
#[op2(fast)]
pub fn op_runtime_close_pane(
    state: &mut OpState,
    #[smi] handle: u32,
    #[smi] pane_id: u32,
) -> Result<(), RuntimeOpError> {
    runtime_close_pane_impl(state, handle, pane_id)
}

/// Write data to a pane's PTY.
#[op2(fast)]
pub fn op_runtime_write_to_pane(
    state: &mut OpState,
    #[smi] handle: u32,
    #[smi] pane_id: u32,
    #[buffer] data: &[u8],
) -> Result<(), RuntimeOpError> {
    runtime_write_to_pane_impl(state, handle, pane_id, data)
}

/// Resize a pane.
#[op2(fast)]
pub fn op_runtime_resize_pane(
    state: &mut OpState,
    #[smi] handle: u32,
    #[smi] pane_id: u32,
    #[smi] rows: u32,
    #[smi] cols: u32,
) -> Result<(), RuntimeOpError> {
    runtime_resize_pane_impl(state, handle, pane_id, rows, cols)
}

/// Get all active pane IDs.
#[op2]
#[serde]
pub fn op_runtime_pane_ids(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<Vec<u32>, RuntimeOpError> {
    runtime_pane_ids_impl(state, handle)
}

/// Get current runtime state as a string.
#[op2]
#[string]
pub fn op_runtime_state(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<String, RuntimeOpError> {
    runtime_state_impl(state, handle)
}

/// Destroy a runtime instance.
#[op2(fast)]
pub fn op_runtime_destroy(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<(), RuntimeOpError> {
    runtime_destroy_impl(state, handle)
}

deno_core::extension!(
    marauder_runtime_ext,
    ops = [
        op_runtime_create,
        op_runtime_is_primary_attached,
        op_runtime_boot,
        op_runtime_shutdown,
        op_runtime_create_pane,
        op_runtime_close_pane,
        op_runtime_write_to_pane,
        op_runtime_resize_pane,
        op_runtime_pane_ids,
        op_runtime_state,
        op_runtime_destroy,
    ],
    state = |state| init_runtime_state(state),
);

/// Build the deno_core Extension for runtime ops.
pub fn runtime_extension() -> deno_core::Extension {
    marauder_runtime_ext::init()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deno_core::OpState;

    fn make_state() -> OpState {
        let mut state = OpState::new(None);
        init_runtime_state(&mut state);
        state
    }

    #[test]
    fn test_create_returns_incrementing_handles() {
        let mut state = make_state();
        let h1 = runtime_create_impl(&mut state).expect("first create should succeed");
        let h2 = runtime_create_impl(&mut state).expect("second create should succeed");
        assert_eq!(h1, 1, "first handle should be 1");
        assert_eq!(h2, 2, "second handle should be 2");
    }

    #[test]
    fn test_state_initial() {
        let mut state = make_state();
        let handle = runtime_create_impl(&mut state).expect("create should succeed");
        let rt_state = runtime_state_impl(&mut state, handle).expect("state should succeed");
        assert_eq!(rt_state, "created", "newly created runtime state should be 'created'");
    }

    #[test]
    fn test_pane_ids_empty() {
        let mut state = make_state();
        let handle = runtime_create_impl(&mut state).expect("create should succeed");
        let panes = runtime_pane_ids_impl(&mut state, handle).expect("pane_ids should succeed");
        assert!(panes.is_empty(), "newly created runtime should have no panes");
    }

    #[test]
    fn test_invalid_handle_error() {
        let mut state = make_state();
        let result = runtime_state_impl(&mut state, 9999);
        assert!(result.is_err(), "state on invalid handle should return error");
        let err = result.unwrap_err();
        assert!(
            err.0.contains("9999"),
            "error message should mention the invalid handle: {}", err.0
        );
    }

    #[test]
    fn test_destroy_then_access() {
        let mut state = make_state();
        let handle = runtime_create_impl(&mut state).expect("create should succeed");
        runtime_destroy_impl(&mut state, handle).expect("destroy should succeed");
        let result = runtime_state_impl(&mut state, handle);
        assert!(result.is_err(), "accessing state after destroy should fail");
    }

    #[test]
    fn test_destroy_invalid_handle() {
        let mut state = make_state();
        let result = runtime_destroy_impl(&mut state, 9999);
        assert!(result.is_err(), "destroy on nonexistent handle should return error");
        let err = result.unwrap_err();
        assert!(
            err.0.contains("9999"),
            "error message should mention the invalid handle: {}", err.0
        );
    }

    #[test]
    fn test_primary_attached() {
        let mut state = make_state();
        let before = runtime_is_primary_attached_impl(&mut state);
        assert_eq!(before, 0, "primary should not be attached initially");
        mark_primary_attached(&mut state);
        let after = runtime_is_primary_attached_impl(&mut state);
        assert_eq!(after, 1, "primary should be attached after mark_primary_attached");
    }

    #[test]
    fn test_handle_overflow() {
        let mut state = make_state();
        // Set NextRuntimeId to u32::MAX so the next checked_add overflows.
        {
            let id_rc = state.borrow::<NextRuntimeId>().clone();
            let mut id = lock_or_log(&id_rc, "runtime::test overflow");
            *id = u32::MAX;
        }
        let result = runtime_create_impl(&mut state);
        assert!(result.is_err(), "create with overflowed ID should return error");
        let err = result.unwrap_err();
        assert!(
            err.0.contains("overflow"),
            "error message should mention overflow: {}", err.0
        );
    }
}
