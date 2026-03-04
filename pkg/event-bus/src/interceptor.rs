use std::sync::Arc;

use crate::events::Event;

/// What an interceptor decides to do with an event.
#[derive(Debug)]
pub enum InterceptorAction {
    /// Pass the event through unchanged.
    Pass,
    /// Replace the event with a modified version.
    Modify(Event),
    /// Suppress the event entirely — subscribers will not receive it.
    Suppress,
}

/// Trait for event interceptors that can inspect, modify, or suppress events.
pub trait Interceptor: Send + Sync {
    /// The priority of this interceptor. Lower values run first.
    fn priority(&self) -> i32 {
        0
    }

    /// Inspect and optionally modify or suppress an event before it reaches subscribers.
    fn intercept(&self, event: &Event) -> InterceptorAction;
}

/// Unique ID for an interceptor, used for removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterceptorId(pub(crate) u64);

/// A boxed interceptor with its priority cached for sorting.
pub(crate) struct RegisteredInterceptor {
    pub id: InterceptorId,
    pub priority: i32,
    pub interceptor: Arc<dyn Interceptor>,
}

impl RegisteredInterceptor {
    pub fn new(id: InterceptorId, interceptor: Box<dyn Interceptor>) -> Self {
        let priority = interceptor.priority();
        Self {
            id,
            priority,
            interceptor: Arc::from(interceptor),
        }
    }

    /// Get an Arc clone for snapshot-then-invoke pattern.
    pub fn interceptor_arc(&self) -> Arc<dyn Interceptor> {
        Arc::clone(&self.interceptor)
    }
}
