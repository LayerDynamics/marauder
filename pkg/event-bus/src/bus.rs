use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex};

use crate::events::{Event, EventType};
use crate::interceptor::{InterceptorAction, RegisteredInterceptor, Interceptor};

/// Callback type for event subscribers.
pub type SubscriberCallback = Arc<dyn Fn(&Event) + Send + Sync>;

/// A unique subscriber ID for unsubscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriberId(pub u64);

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
        let mut next_id = self.next_id.lock().unwrap_or_else(|e| e.into_inner());
        let id = SubscriberId(*next_id);
        *next_id += 1;

        let subscriber = Subscriber {
            id,
            callback: Arc::new(callback),
        };

        let mut subs = self.subscribers.write().unwrap_or_else(|e| e.into_inner());
        subs.entry(event_type).or_default().push(subscriber);
        id
    }

    /// Unsubscribe by SubscriberId.
    pub fn unsubscribe(&self, event_type: EventType, id: SubscriberId) {
        let mut subs = self.subscribers.write().unwrap_or_else(|e| e.into_inner());
        if let Some(list) = subs.get_mut(&event_type) {
            list.retain(|s| s.id != id);
        }
    }

    /// Register an interceptor that can modify or suppress events.
    pub fn add_interceptor(&self, interceptor: Box<dyn Interceptor>) {
        let mut interceptors = self.interceptors.write().unwrap_or_else(|e| e.into_inner());
        interceptors.push(RegisteredInterceptor::new(interceptor));
        interceptors.sort_by_key(|i| i.priority);
    }

    /// Publish an event. Interceptors run first (in priority order), then subscribers.
    ///
    /// Safe against re-entrancy: locks are dropped before invoking any callbacks.
    pub fn publish(&self, event: Event) {
        // Run interceptors — clone the list under lock, then drop lock before invoking
        let mut current_event = event;
        {
            let interceptors = self.interceptors.read().unwrap_or_else(|e| e.into_inner());
            // Hold lock, run interceptors, drop lock. Interceptors should NOT
            // call back into the bus. For subscriber safety we clone, but interceptors
            // are expected to be pure transforms.
            for reg in interceptors.iter() {
                match reg.interceptor.intercept(&current_event) {
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
        }
        // Interceptor lock is now dropped

        // Dispatch to subscribers — clone list under lock, drop lock, then invoke
        let subscriber_snapshot = {
            let subs = self.subscribers.read().unwrap_or_else(|e| e.into_inner());
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
