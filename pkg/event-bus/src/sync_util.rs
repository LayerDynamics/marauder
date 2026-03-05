//! Helpers for Mutex/RwLock poison recovery with logging.
//!
//! Mutex poisoning means a thread panicked while holding the lock.
//! We recover the inner value (the alternative is crashing the whole app),
//! but emit a `tracing::error!` so the event is observable in logs.

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Lock a `Mutex`, logging an error if it was poisoned.
#[inline]
pub fn lock_or_log<'a, T>(mutex: &'a Mutex<T>, context: &str) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|e| {
        tracing::error!("Mutex poisoned ({}): a thread panicked while holding this lock", context);
        e.into_inner()
    })
}

/// Acquire a read lock on an `RwLock`, logging an error if it was poisoned.
#[inline]
pub fn read_or_log<'a, T>(rw: &'a RwLock<T>, context: &str) -> RwLockReadGuard<'a, T> {
    rw.read().unwrap_or_else(|e| {
        tracing::error!("RwLock poisoned read ({}): a thread panicked while holding this lock", context);
        e.into_inner()
    })
}

/// Acquire a write lock on an `RwLock`, logging an error if it was poisoned.
#[inline]
pub fn write_or_log<'a, T>(rw: &'a RwLock<T>, context: &str) -> RwLockWriteGuard<'a, T> {
    rw.write().unwrap_or_else(|e| {
        tracing::error!("RwLock poisoned write ({}): a thread panicked while holding this lock", context);
        e.into_inner()
    })
}
