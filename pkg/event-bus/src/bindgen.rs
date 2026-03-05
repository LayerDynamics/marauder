//! High-level deno_bindgen bindings for EventBus.

use deno_bindgen::deno_bindgen;
use std::sync::Arc;

use crate::bus::EventBus;
use crate::events::{Event, EventType};
use crate::handle_registry::HandleRegistry;

static REGISTRY: HandleRegistry<Arc<EventBus>> = HandleRegistry::new();

/// Create a new EventBus. Returns a handle ID (0 on failure).
#[deno_bindgen]
fn event_bus_bindgen_create() -> u32 {
    REGISTRY.allocate(Arc::new(EventBus::new()))
}

/// Publish an event. Returns true on success.
#[deno_bindgen]
fn event_bus_bindgen_publish(handle_id: u32, event_type: u32, payload_json: &str) -> u8 {
    let bus = match REGISTRY.get_clone(handle_id) {
        Some(b) => b,
        None => return 0,
    };
    let et = match EventType::from_u32(event_type) {
        Ok(et) => et,
        Err(_) => return 0,
    };
    let payload = match serde_json::from_str::<serde_json::Value>(payload_json) {
        Ok(v) => serde_json::to_vec(&v).unwrap_or_default(),
        Err(_) => return 0,
    };
    let event = Event::new(et, payload);
    bus.publish(event);
    1
}

/// Get subscriber count for an event type.
#[deno_bindgen]
fn event_bus_bindgen_subscriber_count(handle_id: u32, event_type: u32) -> u32 {
    REGISTRY
        .get(handle_id, |bus| {
            let et = match EventType::from_u32(event_type) {
                Ok(et) => et,
                Err(_) => return 0,
            };
            bus.subscriber_count(et) as u32
        })
        .unwrap_or(0)
}

/// Destroy an EventBus handle.
#[deno_bindgen]
fn event_bus_bindgen_destroy(handle_id: u32) {
    REGISTRY.remove(handle_id);
}
