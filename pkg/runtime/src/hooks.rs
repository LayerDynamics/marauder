//! Deno lifecycle hooks — async notifications for runtime events.
//!
//! Rust owns the hot path. Deno gets notified asynchronously via these hooks
//! so it can make policy decisions (keybindings, config, extension loading)
//! without blocking rendering.

use std::sync::Arc;
use tokio::sync::mpsc;
use serde::{Serialize, Deserialize};

/// Lifecycle events sent to Deno.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum LifecycleEvent {
    /// Runtime has booted and is ready.
    Booted,
    /// A new pane was created.
    PaneCreated { pane_id: u64 },
    /// A pane was closed.
    PaneClosed { pane_id: u64 },
    /// A pane was resized.
    PaneResized { pane_id: u64, rows: u16, cols: u16 },
    /// Config was reloaded. Changed keys are provided.
    ConfigReloaded { changed_keys: Vec<String> },
    /// Runtime is shutting down. Extensions should clean up.
    ShuttingDown,
    /// Runtime has fully shut down.
    Shutdown,
}

/// Hook receiver for the Deno side.
pub type LifecycleReceiver = mpsc::UnboundedReceiver<LifecycleEvent>;

/// Hook sender for the Rust side.
pub type LifecycleSender = mpsc::UnboundedSender<LifecycleEvent>;

/// Manages lifecycle hook channels for Deno integration.
pub struct LifecycleHooks {
    senders: Vec<LifecycleSender>,
}

impl LifecycleHooks {
    pub fn new() -> Self {
        Self {
            senders: Vec::new(),
        }
    }

    /// Register a new lifecycle hook consumer. Returns a receiver for events.
    pub fn register(&mut self) -> LifecycleReceiver {
        let (tx, rx) = mpsc::unbounded_channel();
        self.senders.push(tx);
        rx
    }

    /// Send a lifecycle event to all registered consumers.
    /// Dead consumers are automatically removed.
    pub fn notify(&mut self, event: LifecycleEvent) {
        self.senders.retain(|tx| tx.send(event.clone()).is_ok());
    }

    /// Number of active hook consumers.
    pub fn consumer_count(&self) -> usize {
        self.senders.len()
    }
}

impl Default for LifecycleHooks {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared lifecycle hooks for cross-thread access.
pub type SharedLifecycleHooks = Arc<std::sync::Mutex<LifecycleHooks>>;

/// Create a new shared lifecycle hooks instance.
pub fn create_shared_hooks() -> SharedLifecycleHooks {
    Arc::new(std::sync::Mutex::new(LifecycleHooks::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_notify() {
        let mut hooks = LifecycleHooks::new();
        let mut rx = hooks.register();
        assert_eq!(hooks.consumer_count(), 1);

        hooks.notify(LifecycleEvent::Booted);
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, LifecycleEvent::Booted));
    }

    #[test]
    fn test_multiple_consumers() {
        let mut hooks = LifecycleHooks::new();
        let mut rx1 = hooks.register();
        let mut rx2 = hooks.register();
        assert_eq!(hooks.consumer_count(), 2);

        hooks.notify(LifecycleEvent::ShuttingDown);
        assert!(matches!(rx1.try_recv().unwrap(), LifecycleEvent::ShuttingDown));
        assert!(matches!(rx2.try_recv().unwrap(), LifecycleEvent::ShuttingDown));
    }

    #[test]
    fn test_dead_consumer_pruned() {
        let mut hooks = LifecycleHooks::new();
        let rx = hooks.register();
        assert_eq!(hooks.consumer_count(), 1);

        // Drop the receiver
        drop(rx);

        // Notify should prune the dead consumer
        hooks.notify(LifecycleEvent::Shutdown);
        assert_eq!(hooks.consumer_count(), 0);
    }

    #[test]
    fn test_notify_empty_is_noop() {
        let mut hooks = LifecycleHooks::new();
        // Should not panic
        hooks.notify(LifecycleEvent::Booted);
        assert_eq!(hooks.consumer_count(), 0);
    }

    #[test]
    fn test_lifecycle_event_serialization() {
        let event = LifecycleEvent::PaneCreated { pane_id: 42 };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("PaneCreated"));
        assert!(json.contains("42"));

        let deserialized: LifecycleEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, LifecycleEvent::PaneCreated { pane_id: 42 }));
    }

    #[test]
    fn test_shared_hooks() {
        let shared = create_shared_hooks();
        let mut hooks = shared.lock().unwrap();
        let _rx = hooks.register();
        assert_eq!(hooks.consumer_count(), 1);
    }
}
