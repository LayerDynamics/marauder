//! deno_core #[op2] ops for embedded mode.
//!
//! These ops allow the Deno runtime embedded in Tauri to interact with
//! PTY sessions directly through V8, without going through FFI.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use deno_core::op2;
use deno_core::OpState;

use crate::manager::{PaneId, PtyConfig, PtyManager};
use crate::pty;

/// Error type for PTY ops, compatible with deno_core's error handling.
#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[error("{0}")]
pub struct PtyOpError(String);

impl From<anyhow::Error> for PtyOpError {
    fn from(e: anyhow::Error) -> Self {
        Self(e.to_string())
    }
}

/// State key for the PtyManager stored in OpState.
type SharedPtyManager = Rc<RefCell<PtyManager>>;

/// Initialize PtyManager in the OpState.
pub fn init_pty_state(state: &mut OpState) {
    state.put::<SharedPtyManager>(Rc::new(RefCell::new(PtyManager::new())));
}

#[op2]
#[smi]
pub fn op_pty_create(
    state: &mut OpState,
    #[string] shell: Option<String>,
    #[string] cwd: Option<String>,
    #[smi] rows: u32,
    #[smi] cols: u32,
) -> Result<u32, PtyOpError> {
    let mgr = state.borrow::<SharedPtyManager>().clone();
    let mut mgr = mgr.borrow_mut();

    let config = PtyConfig {
        shell: shell.unwrap_or_else(pty::default_shell),
        env: HashMap::new(),
        cwd: cwd.map(PathBuf::from).unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
        }),
        rows: rows as u16,
        cols: cols as u16,
    };

    let id = mgr.create(config)?;
    if id > u32::MAX as u64 {
        return Err(PtyOpError("PaneId exceeded u32 range".to_string()));
    }
    Ok(id as u32)
}

#[op2(fast)]
pub fn op_pty_write(
    state: &mut OpState,
    #[smi] pane_id: u32,
    #[buffer] data: &[u8],
) -> Result<(), PtyOpError> {
    let mgr = state.borrow::<SharedPtyManager>().clone();
    let mut mgr = mgr.borrow_mut();
    mgr.write(pane_id as PaneId, data)?;
    Ok(())
}

/// Maximum allowed read buffer size (64 KiB) to prevent OOM.
const MAX_READ_BYTES: u32 = 65_536;

#[op2]
#[buffer]
pub fn op_pty_read(
    state: &mut OpState,
    #[smi] pane_id: u32,
    #[smi] max_bytes: u32,
) -> Result<Vec<u8>, PtyOpError> {
    let capped = max_bytes.min(MAX_READ_BYTES);
    let mgr = state.borrow::<SharedPtyManager>().clone();
    let mut mgr = mgr.borrow_mut();
    let mut buf = vec![0u8; capped as usize];
    let n = mgr.read(pane_id as PaneId, &mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

#[op2(fast)]
pub fn op_pty_resize(
    state: &mut OpState,
    #[smi] pane_id: u32,
    #[smi] rows: u32,
    #[smi] cols: u32,
) -> Result<(), PtyOpError> {
    let mgr = state.borrow::<SharedPtyManager>().clone();
    let mut mgr = mgr.borrow_mut();
    mgr.resize(pane_id as PaneId, rows as u16, cols as u16)?;
    Ok(())
}

#[op2(fast)]
pub fn op_pty_close(
    state: &mut OpState,
    #[smi] pane_id: u32,
) -> Result<(), PtyOpError> {
    let mgr = state.borrow::<SharedPtyManager>().clone();
    let mut mgr = mgr.borrow_mut();
    mgr.close(pane_id as PaneId)?;
    Ok(())
}

#[op2(fast)]
#[smi]
pub fn op_pty_get_pid(
    state: &mut OpState,
    #[smi] pane_id: u32,
) -> Result<u32, PtyOpError> {
    let mgr = state.borrow::<SharedPtyManager>().clone();
    let mgr = mgr.borrow();
    let pid = mgr.get_pid(pane_id as PaneId)?;
    Ok(pid.unwrap_or(0))
}

#[op2(fast)]
#[smi]
pub fn op_pty_wait(
    state: &mut OpState,
    #[smi] pane_id: u32,
) -> Result<i32, PtyOpError> {
    let mgr = state.borrow::<SharedPtyManager>().clone();
    let mut mgr = mgr.borrow_mut();
    match mgr.try_wait(pane_id as PaneId)? {
        Some(_) => Ok(1),
        None => Ok(0),
    }
}

#[op2(fast)]
#[smi]
pub fn op_pty_count(state: &mut OpState) -> Result<u32, PtyOpError> {
    let mgr = state.borrow::<SharedPtyManager>().clone();
    let mgr = mgr.borrow();
    Ok(mgr.count() as u32)
}

deno_core::extension!(
    marauder_pty_ext,
    ops = [
        op_pty_create,
        op_pty_write,
        op_pty_read,
        op_pty_resize,
        op_pty_close,
        op_pty_get_pid,
        op_pty_wait,
        op_pty_count,
    ],
    state = |state| init_pty_state(state),
);

/// Build the deno_core Extension for PTY ops.
pub fn pty_extension() -> deno_core::Extension {
    marauder_pty_ext::init()
}
