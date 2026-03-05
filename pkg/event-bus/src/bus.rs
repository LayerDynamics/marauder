use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex};

use crate::events::{Event, EventType};
use crate::interceptor::{InterceptorAction, InterceptorId, RegisteredInterceptor, Interceptor};
use crate::sync_util::{lock_or_log, read_or_log, write_or_log};

/// Callback type for event subscribers.
pub type SubscriberCallback = Arc<dyn Fn(&Event) + Send + Sync>;

/// A unique subscriber ID for unsubscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriberId(pub(crate) u64);

impl SubscriberId {
    /// Get the raw ID value (for FFI and serialization).
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Construct from a raw ID (for FFI boundary use only).
    pub fn from_raw(id: u64) -> Self {
        Self(id)
    }
}

struct Subscriber {
    id: SubscriberId,
    callback: SubscriberCallback,
}

impl Clone for Subscriber {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            callback: Arc::clone(&self.callback),
        }
    }
}

/// Typed pub/sub event bus. Thread-safe via RwLock (concurrent reads) and Mutex (writes).
pub struct EventBus {
    subscribers: RwLock<HashMap<EventType, Vec<Subscriber>>>,
    interceptors: RwLock<Vec<RegisteredInterceptor>>,
    next_id: Mutex<u64>,
}

impl EventBus {
    #[must_use]
    pub fn new() -> Self {
        Self {
            subscribers: RwLock::new(HashMap::new()),
            interceptors: RwLock::new(Vec::new()),
            next_id: Mutex::new(1),
        }
    }

    /// Subscribe to a specific event type. Returns a SubscriberId for later unsubscription.
    pub fn subscribe<F>(&self, event_type: EventType, callback: F) -> SubscriberId
    where
        F: Fn(&Event) + Send + Sync + 'static,
    {
        let mut next_id = lock_or_log(&self.next_id, "EventBus::subscribe/next_id");
        let id = SubscriberId(*next_id);
        *next_id += 1;

        let subscriber = Subscriber {
            id,
            callback: Arc::new(callback),
        };

        let mut subs = write_or_log(&self.subscribers, "EventBus::subscribe/subscribers");
        subs.entry(event_type).or_default().push(subscriber);
        id
    }

    /// Unsubscribe by SubscriberId.
    pub fn unsubscribe(&self, event_type: EventType, id: SubscriberId) {
        let mut subs = write_or_log(&self.subscribers, "EventBus::unsubscribe/subscribers");
        if let Some(list) = subs.get_mut(&event_type) {
            list.retain(|s| s.id != id);
        }
    }

    /// Register an interceptor that can modify or suppress events. Returns an ID for removal.
    pub fn add_interceptor(&self, interceptor: Box<dyn Interceptor>) -> InterceptorId {
        let mut next_id = lock_or_log(&self.next_id, "EventBus::add_interceptor/next_id");
        let id = InterceptorId(*next_id);
        *next_id += 1;

        let mut interceptors = write_or_log(&self.interceptors, "EventBus::add_interceptor/interceptors");
        interceptors.push(RegisteredInterceptor::new(id, interceptor));
        interceptors.sort_by_key(|i| i.priority);
        id
    }

    /// Get the number of subscribers for a given event type.
    pub fn subscriber_count(&self, event_type: EventType) -> usize {
        let subs = read_or_log(&self.subscribers, "EventBus::subscriber_count/subscribers");
        subs.get(&event_type).map_or(0, |list| list.len())
    }

    /// Remove an interceptor by its ID.
    pub fn remove_interceptor(&self, id: InterceptorId) {
        let mut interceptors = write_or_log(&self.interceptors, "EventBus::remove_interceptor/interceptors");
        interceptors.retain(|i| i.id != id);
    }

    /// Publish an event. Interceptors run first (in priority order), then subscribers.
    ///
    /// Safe against re-entrancy: locks are dropped before invoking any callbacks.
    pub fn publish(&self, event: Event) {
        // Run interceptors — snapshot the list under lock, then drop lock before invoking.
        // This prevents deadlock if an interceptor calls back into the bus.
        let mut current_event = event;
        let interceptor_snapshot: Vec<_> = {
            let interceptors = read_or_log(&self.interceptors, "EventBus::publish/interceptors");
            interceptors.iter().map(|reg| reg.interceptor_arc()).collect()
        };
        // Interceptor lock is now dropped — safe to call interceptors
        for interceptor in &interceptor_snapshot {
            match interceptor.intercept(&current_event) {
                InterceptorAction::Pass => {}
                InterceptorAction::Modify(modified) => {
                    current_event = modified;
                }
                InterceptorAction::Suppress => {
                    tracing::trace!(event_type = ?current_event.event_type, "Event suppressed by interceptor");
                    return;
                }
            }
        }

        // Dispatch to subscribers — clone list under lock, drop lock, then invoke
        let subscriber_snapshot = {
            let subs = read_or_log(&self.subscribers, "EventBus::publish/subscribers");
            subs.get(&current_event.event_type).cloned().unwrap_or_default()
        };
        // Subscriber lock is now dropped — callbacks can safely call subscribe/unsubscribe/publish
        for subscriber in &subscriber_snapshot {
            (subscriber.callback)(&current_event);
        }
    }

    /// Publish an event from parts (convenience method).
    pub fn emit(&self, event_type: EventType, payload: impl serde::Serialize) {
        self.publish(Event::new(event_type, payload));
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe shared reference to an EventBus.
pub type SharedEventBus = Arc<EventBus>;

/// Create a new shared EventBus.
pub fn create_shared() -> SharedEventBus {
    Arc::new(EventBus::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn test_publish_subscribe() {
        let bus = EventBus::new();
        let count = Arc::new(AtomicU32::new(0));
        let count_clone = count.clone();

        bus.subscribe(EventType::KeyInput, move |_event| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        bus.emit(EventType::KeyInput, "test");
        bus.emit(EventType::KeyInput, "test2");
        bus.emit(EventType::PtyOutput, "ignored");

        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_unsubscribe() {
        let bus = EventBus::new();
        let count = Arc::new(AtomicU32::new(0));
        let count_clone = count.clone();

        let id = bus.subscribe(EventType::GridUpdated, move |_| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        bus.emit(EventType::GridUpdated, ());
        assert_eq!(count.load(Ordering::SeqCst), 1);

        bus.unsubscribe(EventType::GridUpdated, id);
        bus.emit(EventType::GridUpdated, ());
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_interceptor_suppress() {
        struct SuppressAll;
        impl Interceptor for SuppressAll {
            fn intercept(&self, _event: &Event) -> InterceptorAction {
                InterceptorAction::Suppress
            }
        }

        let bus = EventBus::new();
        let count = Arc::new(AtomicU32::new(0));
        let count_clone = count.clone();

        bus.subscribe(EventType::KeyInput, move |_| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        bus.add_interceptor(Box::new(SuppressAll));
        bus.emit(EventType::KeyInput, "suppressed");

        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_interceptor_modify() {
        struct AddSource;
        impl Interceptor for AddSource {
            fn intercept(&self, event: &Event) -> InterceptorAction {
                let mut modified = event.clone();
                modified.source = Some("interceptor".into());
                InterceptorAction::Modify(modified)
            }
        }

        let bus = EventBus::new();
        let received_source = Arc::new(Mutex::new(None));
        let source_clone = received_source.clone();

        bus.subscribe(EventType::KeyInput, move |event| {
            *source_clone.lock().unwrap() = event.source.clone();
        });

        bus.add_interceptor(Box::new(AddSource));
        bus.emit(EventType::KeyInput, "test");

        assert_eq!(
            received_source.lock().unwrap().as_deref(),
            Some("interceptor")
        );
    }

    #[test]
    fn test_interceptor_priority_order() {
        use std::sync::atomic::AtomicI32;

        let order = Arc::new(AtomicI32::new(0));

        struct PriorityInterceptor {
            prio: i32,
            expected_order: i32,
            order: Arc<AtomicI32>,
        }
        impl Interceptor for PriorityInterceptor {
            fn priority(&self) -> i32 {
                self.prio
            }
            fn intercept(&self, _event: &Event) -> InterceptorAction {
                let current = self.order.fetch_add(1, Ordering::SeqCst);
                assert_eq!(current, self.expected_order);
                InterceptorAction::Pass
            }
        }

        let bus = EventBus::new();
        bus.add_interceptor(Box::new(PriorityInterceptor {
            prio: 10,
            expected_order: 1,
            order: order.clone(),
        }));
        bus.add_interceptor(Box::new(PriorityInterceptor {
            prio: -5,
            expected_order: 0,
            order: order.clone(),
        }));

        bus.subscribe(EventType::KeyInput, |_| {});
        bus.emit(EventType::KeyInput, "test");

        assert_eq!(order.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_reentrant_publish_no_deadlock() {
        let bus = Arc::new(EventBus::new());
        let bus_clone = bus.clone();
        let count = Arc::new(AtomicU32::new(0));
        let count_clone = count.clone();

        // Subscriber that publishes another event — would deadlock with old Mutex impl
        bus.subscribe(EventType::KeyInput, move |_| {
            count_clone.fetch_add(1, Ordering::SeqCst);
            if count_clone.load(Ordering::SeqCst) == 1 {
                bus_clone.emit(EventType::PtyOutput, "reentrant");
            }
        });

        let count_clone2 = count.clone();
        bus.subscribe(EventType::PtyOutput, move |_| {
            count_clone2.fetch_add(1, Ordering::SeqCst);
        });

        bus.emit(EventType::KeyInput, "trigger");
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
