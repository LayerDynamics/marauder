//! Shared utility helpers for the runtime crate.

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Lock a `Mutex`, recovering from poison and logging a warning.
pub fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, label: &str) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|e| {
        tracing::warn!("{label} mutex was poisoned, recovering");
        e.into_inner()
    })
}

/// Read-lock a `RwLock`, recovering from poison and logging a warning.
pub fn read_or_recover<'a, T>(lock: &'a RwLock<T>, label: &str) -> RwLockReadGuard<'a, T> {
    lock.read().unwrap_or_else(|e| {
        tracing::warn!("{label} RwLock was poisoned (read), recovering");
        e.into_inner()
    })
}

/// Write-lock a `RwLock`, recovering from poison and logging a warning.
pub fn write_or_recover<'a, T>(lock: &'a RwLock<T>, label: &str) -> RwLockWriteGuard<'a, T> {
    lock.write().unwrap_or_else(|e| {
        tracing::warn!("{label} RwLock was poisoned (write), recovering");
        e.into_inner()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_lock_or_recover_normal() {
        let m = Mutex::new(42);
        let guard = lock_or_recover(&m, "test");
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_lock_or_recover_poisoned() {
        let m = Arc::new(Mutex::new(42));
        let m2 = Arc::clone(&m);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut guard = m2.lock().unwrap();
            *guard = 99;
            panic!("intentional");
        }));
        let guard = lock_or_recover(&m, "test");
        assert_eq!(*guard, 99);
    }

    #[test]
    fn test_read_or_recover_normal() {
        let lock = RwLock::new(42);
        let guard = read_or_recover(&lock, "test");
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_write_or_recover_normal() {
        let lock = RwLock::new(42);
        let mut guard = write_or_recover(&lock, "test");
        *guard = 99;
        assert_eq!(*guard, 99);
    }
}
