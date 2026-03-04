//! Shared handle registry for deno_bindgen modules.
//!
//! Provides thread-safe ID allocation and handle storage, enforcing that
//! `0` is never a valid handle ID (used as an error sentinel across FFI).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// A thread-safe registry mapping `u32` handle IDs to values of type `T`.
///
/// IDs start at 1 and increment monotonically. On overflow, `allocate()`
/// returns `0` and does not insert the value — callers must treat `0` as invalid.
pub struct HandleRegistry<T> {
    handles: OnceLock<Mutex<HashMap<u32, T>>>,
    next_id: OnceLock<Mutex<u32>>,
}

impl<T> HandleRegistry<T> {
    /// Create a new registry. Must be used in a `static` context.
    pub const fn new() -> Self {
        Self {
            handles: OnceLock::new(),
            next_id: OnceLock::new(),
        }
    }

    fn map(&self) -> &Mutex<HashMap<u32, T>> {
        self.handles.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn id_counter(&self) -> &Mutex<u32> {
        self.next_id.get_or_init(|| Mutex::new(1))
    }

    /// Allocate a new ID and insert the value. Returns the ID, or `0` on overflow.
    pub fn allocate(&self, value: T) -> u32 {
        let mut id = self.id_counter().lock().unwrap_or_else(|e| e.into_inner());
        let val = *id;
        match val.checked_add(1) {
            Some(next) => {
                *id = next;
                drop(id); // release ID lock before acquiring map lock
                self.map().lock().unwrap_or_else(|e| e.into_inner()).insert(val, value);
                val
            }
            None => {
                tracing::error!("bindgen handle ID counter overflow");
                0
            }
        }
    }

    /// Get a clone of the value for a handle ID.
    pub fn get<R>(&self, id: u32, f: impl FnOnce(&T) -> R) -> Option<R> {
        let map = self.map().lock().unwrap_or_else(|e| e.into_inner());
        map.get(&id).map(f)
    }

    /// Get a clone of the value (requires T: Clone).
    pub fn get_clone(&self, id: u32) -> Option<T>
    where
        T: Clone,
    {
        self.get(id, |v| v.clone())
    }

    /// Remove a handle.
    pub fn remove(&self, id: u32) -> Option<T> {
        self.map().lock().unwrap_or_else(|e| e.into_inner()).remove(&id)
    }
}
