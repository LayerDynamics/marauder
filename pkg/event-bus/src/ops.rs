//! deno_core #[op2] ops for the event bus in embedded mode.
//!
//! Provides subscribe, unsubscribe, poll (async recv), publish, interceptor,
//! and subscriber_count ops so Deno can fully participate in the event bus.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use deno_core::op2;
use deno_core::OpState;
use tokio::sync::mpsc;

use crate::bus::{EventBus, SharedEventBus, SubscriberId};
use crate::events::{Event, EventType};
use crate::interceptor::{Interceptor, InterceptorAction, InterceptorId};
use crate::sync_util::lock_or_log;

/// Error type for event bus ops.
#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[error("{0}")]
pub struct EventBusOpError(String);

impl From<crate::events::EventError> for EventBusOpError {
    fn from(e: crate::events::EventError) -> Self {
        Self(e.to_string())
    }
}

/// Per-subscription channel receiver, keyed by a JS-visible subscription handle.
type SubscriptionMap = Arc<Mutex<HashMap<u64, mpsc::UnboundedReceiver<SerializedEvent>>>>;

/// Tracks SubscriberIds so we can unsubscribe from the bus.
type SubscriberIdMap = Arc<Mutex<HashMap<u64, (EventType, SubscriberId)>>>;

/// Tracks InterceptorIds so we can remove interceptors.
type InterceptorIdMap = Arc<Mutex<HashMap<u64, InterceptorId>>>;

/// Counter for generating unique subscription handles visible to JS.
type HandleCounter = Arc<Mutex<u64>>;

/// A serialized event ready for JS consumption.
#[derive(Debug, Clone, serde::Serialize)]
struct SerializedEvent {
    event_type: u32,
    payload_json: String,
    timestamp_us: u64,
    source: Option<String>,
}

impl From<&Event> for SerializedEvent {
    fn from(event: &Event) -> Self {
        Self {
            event_type: event.event_type.as_u32(),
            payload_json: String::from_utf8_lossy(&event.payload).into_owned(),
            timestamp_us: event.timestamp_us,
            source: event.source.clone(),
        }
    }
}

/// Initialize event bus state in OpState with fresh default instances.
pub fn init_event_bus_state(state: &mut OpState) {
    let bus: SharedEventBus = Arc::new(EventBus::new());
    state.put::<SharedEventBus>(bus);
    state.put::<SubscriptionMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<SubscriberIdMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<InterceptorIdMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<HandleCounter>(Arc::new(Mutex::new(1)));
}

/// Inject a shared event bus from the real runtime into OpState,
/// replacing the default disconnected instance.
pub fn inject_shared_event_bus(state: &mut OpState, bus: SharedEventBus) {
    state.put::<SharedEventBus>(bus);
    // Reset auxiliary maps since they were tied to the old bus
    state.put::<SubscriptionMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<SubscriberIdMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<InterceptorIdMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<HandleCounter>(Arc::new(Mutex::new(1)));
}

/// Allocate a unique handle for JS.
fn next_handle(state: &mut OpState) -> u64 {
    let counter = state.borrow::<HandleCounter>().clone();
    let mut c = lock_or_log(&counter, "ops::next_handle/counter");
    let h = *c;
    *c += 1;
    h
}

// ---------------------------------------------------------------------------
// _impl functions — contain the actual logic; called by #[op2] wrappers and tests
// ---------------------------------------------------------------------------

fn event_bus_publish_impl(
    state: &mut OpState,
    event_type: u32,
    payload_json: String,
) -> Result<(), EventBusOpError> {
    let et = EventType::from_u32(event_type)?;

    // Validate that the payload is well-formed JSON before storing as bytes.
    // Consumers use payload_as<T>() (serde_json::from_slice) and expect valid JSON.
    let payload_bytes = serde_json::from_str::<serde_json::Value>(&payload_json)
        .map_err(|e| EventBusOpError(format!("invalid JSON payload: {e}")))?;
    let payload = serde_json::to_vec(&payload_bytes)
        .map_err(|e| EventBusOpError(format!("JSON re-serialization failed: {e}")))?;

    let bus = state.borrow::<SharedEventBus>().clone();

    let event = Event {
        event_type: et,
        payload,
        timestamp_us: Event::now_us(),
        source: None,
    };
    bus.publish(event);
    Ok(())
}

fn event_bus_subscriber_count_impl(
    state: &mut OpState,
    event_type: u32,
) -> Result<u32, EventBusOpError> {
    let et = EventType::from_u32(event_type)?;
    let bus = state.borrow::<SharedEventBus>().clone();
    Ok(bus.subscriber_count(et) as u32)
}

fn event_bus_subscribe_impl(
    state: &mut OpState,
    event_type: u32,
) -> Result<u64, EventBusOpError> {
    let et = EventType::from_u32(event_type)?;
    let handle = next_handle(state);

    let (tx, rx) = mpsc::unbounded_channel::<SerializedEvent>();

    // Register the channel receiver in state
    {
        let subs = state.borrow::<SubscriptionMap>().clone();
        lock_or_log(&subs, "ops::subscribe/subscription_map").insert(handle, rx);
    }

    // Subscribe on the bus — callback sends into the channel
    let bus = state.borrow::<SharedEventBus>().clone();
    let sub_id = bus.subscribe(et, move |event: &Event| {
        let serialized = SerializedEvent::from(event);
        let _ = tx.send(serialized);
    });

    // Track the SubscriberId for unsubscribe
    {
        let ids = state.borrow::<SubscriberIdMap>().clone();
        lock_or_log(&ids, "ops::subscribe/subscriber_id_map").insert(handle, (et, sub_id));
    }

    Ok(handle)
}

fn event_bus_unsubscribe_impl(
    state: &mut OpState,
    handle: u64,
) -> Result<(), EventBusOpError> {
    {
        let subs = state.borrow::<SubscriptionMap>().clone();
        lock_or_log(&subs, "ops::unsubscribe/subscription_map").remove(&handle);
    }

    let entry = {
        let ids = state.borrow::<SubscriberIdMap>().clone();
        let result = lock_or_log(&ids, "ops::unsubscribe/subscriber_id_map").remove(&handle);
        result
    };
    if let Some((et, sub_id)) = entry {
        let bus = state.borrow::<SharedEventBus>().clone();
        bus.unsubscribe(et, sub_id);
    }

    Ok(())
}

fn event_bus_add_interceptor_impl(
    state: &mut OpState,
    priority: i32,
) -> Result<u64, EventBusOpError> {
    let handle = next_handle(state);
    let (tx, rx) = mpsc::unbounded_channel::<SerializedEvent>();

    let interceptor = Box::new(JsInterceptor { priority, tx });

    let bus = state.borrow::<SharedEventBus>().clone();
    let interceptor_id = bus.add_interceptor(interceptor);

    {
        let subs = state.borrow::<SubscriptionMap>().clone();
        lock_or_log(&subs, "ops::add_interceptor/subscription_map").insert(handle, rx);
    }

    {
        let ids = state.borrow::<InterceptorIdMap>().clone();
        lock_or_log(&ids, "ops::add_interceptor/interceptor_id_map").insert(handle, interceptor_id);
    }

    Ok(handle)
}

fn event_bus_remove_interceptor_impl(
    state: &mut OpState,
    handle: u64,
) -> Result<(), EventBusOpError> {
    {
        let subs = state.borrow::<SubscriptionMap>().clone();
        lock_or_log(&subs, "ops::remove_interceptor/subscription_map").remove(&handle);
    }

    let entry = {
        let ids = state.borrow::<InterceptorIdMap>().clone();
        let result = lock_or_log(&ids, "ops::remove_interceptor/interceptor_id_map").remove(&handle);
        result
    };
    if let Some(interceptor_id) = entry {
        let bus = state.borrow::<SharedEventBus>().clone();
        bus.remove_interceptor(interceptor_id);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// #[op2] wrappers — delegate to _impl functions
// ---------------------------------------------------------------------------

/// Publish an event to the bus.
#[op2(fast)]
pub fn op_event_bus_publish(
    state: &mut OpState,
    #[smi] event_type: u32,
    #[string] payload_json: String,
) -> Result<(), EventBusOpError> {
    event_bus_publish_impl(state, event_type, payload_json)
}

/// Get the subscriber count for a given event type.
#[op2(fast)]
#[smi]
pub fn op_event_bus_subscriber_count(
    state: &mut OpState,
    #[smi] event_type: u32,
) -> Result<u32, EventBusOpError> {
    event_bus_subscriber_count_impl(state, event_type)
}

/// Subscribe to an event type. Returns a handle (u64) used to poll and unsubscribe.
#[op2(fast)]
#[smi]
pub fn op_event_bus_subscribe(
    state: &mut OpState,
    #[smi] event_type: u32,
) -> Result<u64, EventBusOpError> {
    event_bus_subscribe_impl(state, event_type)
}

/// Unsubscribe from an event type using the handle returned by subscribe.
#[op2(fast)]
pub fn op_event_bus_unsubscribe(
    state: &mut OpState,
    #[smi] handle: u64,
) -> Result<(), EventBusOpError> {
    event_bus_unsubscribe_impl(state, handle)
}

/// Async op: wait for the next event on a subscription.
#[op2(async)]
#[string]
pub async fn op_event_bus_poll(
    state: Rc<RefCell<OpState>>,
    #[smi] handle: u64,
) -> Result<Option<String>, EventBusOpError> {
    let rx_opt = {
        let st = state.borrow();
        let subs = st.borrow::<SubscriptionMap>().clone();
        let result = lock_or_log(&subs, "ops::poll/subscription_map_remove").remove(&handle);
        result
    };

    let mut rx = match rx_opt {
        Some(rx) => rx,
        None => return Ok(None),
    };

    let result = rx.recv().await;

    // Put the receiver back
    {
        let st = state.borrow();
        let subs = st.borrow::<SubscriptionMap>().clone();
        lock_or_log(&subs, "ops::poll/subscription_map_reinsert").insert(handle, rx);
    }

    match result {
        Some(event) => {
            let json = serde_json::to_string(&event)
                .map_err(|e| EventBusOpError(e.to_string()))?;
            Ok(Some(json))
        }
        None => Ok(None),
    }
}

/// An interceptor driven from JS.
struct JsInterceptor {
    priority: i32,
    tx: mpsc::UnboundedSender<SerializedEvent>,
}

impl Interceptor for JsInterceptor {
    fn priority(&self) -> i32 {
        self.priority
    }

    fn intercept(&self, event: &Event) -> InterceptorAction {
        let _ = self.tx.send(SerializedEvent::from(event));
        InterceptorAction::Pass
    }
}

/// Add an interceptor that forwards events to JS for observation.
#[op2(fast)]
#[smi]
pub fn op_event_bus_add_interceptor(
    state: &mut OpState,
    #[smi] priority: i32,
) -> Result<u64, EventBusOpError> {
    event_bus_add_interceptor_impl(state, priority)
}

/// Remove an interceptor by handle.
#[op2(fast)]
pub fn op_event_bus_remove_interceptor(
    state: &mut OpState,
    #[smi] handle: u64,
) -> Result<(), EventBusOpError> {
    event_bus_remove_interceptor_impl(state, handle)
}

deno_core::extension!(
    marauder_event_bus_ext,
    ops = [
        op_event_bus_publish,
        op_event_bus_subscriber_count,
        op_event_bus_subscribe,
        op_event_bus_unsubscribe,
        op_event_bus_poll,
        op_event_bus_add_interceptor,
        op_event_bus_remove_interceptor,
    ],
    state = |state| init_event_bus_state(state),
);

/// Build the deno_core Extension for event bus ops.
pub fn event_bus_extension() -> deno_core::Extension {
    marauder_event_bus_ext::init()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deno_core::OpState;

    /// Build a fresh OpState with event bus state initialized.
    fn make_state() -> OpState {
        let mut state = OpState::new(None);
        init_event_bus_state(&mut state);
        state
    }

    #[test]
    fn test_init_state() {
        let state = make_state();
        let _bus = state.borrow::<SharedEventBus>();
        let _subs = state.borrow::<SubscriptionMap>();
        let _sub_ids = state.borrow::<SubscriberIdMap>();
        let _icp_ids = state.borrow::<InterceptorIdMap>();
        let _counter = state.borrow::<HandleCounter>();
    }

    #[test]
    fn test_publish_valid_event() {
        let mut state = make_state();
        let et = EventType::GridUpdated.as_u32();
        let result = event_bus_publish_impl(&mut state, et, r#"{"row":0}"#.to_string());
        assert!(result.is_ok(), "publish with valid event type should succeed");
    }

    #[test]
    fn test_publish_invalid_event_type() {
        let mut state = make_state();
        let result = event_bus_publish_impl(&mut state, 99999, "{}".to_string());
        assert!(
            result.is_err(),
            "publish with invalid event type u32 should return an error"
        );
    }

    #[test]
    fn test_subscriber_count_zero() {
        let mut state = make_state();
        let et = EventType::PaneCreated.as_u32();
        let count = event_bus_subscriber_count_impl(&mut state, et)
            .expect("subscriber_count should not error for valid event type");
        assert_eq!(count, 0, "count before any subscriptions should be 0");
    }

    #[test]
    fn test_subscribe_and_count() {
        let mut state = make_state();
        let et = EventType::PaneCreated.as_u32();

        event_bus_subscribe_impl(&mut state, et).expect("subscribe should succeed");

        let count = event_bus_subscriber_count_impl(&mut state, et)
            .expect("subscriber_count should not error");
        assert_eq!(count, 1, "count after one subscribe should be 1");
    }

    #[test]
    fn test_subscribe_returns_unique_handles() {
        let mut state = make_state();
        let et = EventType::KeyInput.as_u32();

        let h1 = event_bus_subscribe_impl(&mut state, et).expect("first subscribe should succeed");
        let h2 = event_bus_subscribe_impl(&mut state, et).expect("second subscribe should succeed");

        assert_ne!(h1, h2, "two subscribes must return different handles");
    }

    #[test]
    fn test_unsubscribe_decrements_count() {
        let mut state = make_state();
        let et = EventType::ShellCommandStarted.as_u32();

        let handle =
            event_bus_subscribe_impl(&mut state, et).expect("subscribe should succeed");

        event_bus_unsubscribe_impl(&mut state, handle)
            .expect("unsubscribe should succeed");

        let count = event_bus_subscriber_count_impl(&mut state, et)
            .expect("subscriber_count should not error");
        assert_eq!(count, 0, "count after unsubscribe should be 0");
    }

    #[test]
    fn test_unsubscribe_nonexistent_ok() {
        let mut state = make_state();
        let result = event_bus_unsubscribe_impl(&mut state, 999_999);
        assert!(
            result.is_ok(),
            "unsubscribing a bogus handle should not return an error"
        );
    }

    #[test]
    fn test_add_and_remove_interceptor() {
        let mut state = make_state();

        let handle =
            event_bus_add_interceptor_impl(&mut state, 10).expect("add_interceptor should succeed");

        let result = event_bus_remove_interceptor_impl(&mut state, handle);
        assert!(result.is_ok(), "remove_interceptor should succeed");
    }

    #[test]
    fn test_remove_interceptor_nonexistent_ok() {
        let mut state = make_state();
        let result = event_bus_remove_interceptor_impl(&mut state, 999_999);
        assert!(
            result.is_ok(),
            "removing a bogus interceptor handle should not return an error"
        );
    }

    #[test]
    fn test_publish_invalid_json_payload() {
        let mut state = make_state();
        let et = EventType::GridUpdated.as_u32();
        let result = event_bus_publish_impl(&mut state, et, "not valid json {{{".to_string());
        assert!(
            result.is_err(),
            "publish with invalid JSON payload should return an error"
        );
    }

    #[test]
    fn test_publish_empty_json_object() {
        let mut state = make_state();
        let et = EventType::GridUpdated.as_u32();
        let result = event_bus_publish_impl(&mut state, et, "{}".to_string());
        assert!(result.is_ok(), "publish with empty JSON object should succeed");
    }
}
