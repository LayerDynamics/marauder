use std::sync::Mutex;

use marauder_event_bus::bus::{SharedEventBus, SubscriberId};
use marauder_event_bus::events::{Event, EventType};
use tauri::ipc::Channel;

/// Event types safe for the webview to emit. Excludes internal system events
/// that could be spoofed to confuse subscribers (PTY, parser, render, config internals).
const WEBVIEW_ALLOWED_EMIT_TYPES: &[EventType] = &[
    EventType::KeyInput,
    EventType::MouseInput,
    EventType::PasteInput,
    EventType::PaneFocused,
    EventType::TabCreated,
    EventType::TabClosed,
    EventType::TabFocused,
    EventType::ExtensionMessage,
];

/// Event types forwarded to the webview. Excludes hot-path events
/// (PtyOutput, ParserAction, RenderFrame*) that fire at extreme rates.
const BRIDGE_EVENT_TYPES: &[EventType] = &[
    EventType::KeyInput,
    EventType::MouseInput,
    EventType::PasteInput,
    EventType::PtyExit,
    EventType::PtyError,
    EventType::GridResized,
    EventType::GridScrolled,
    EventType::SelectionChanged,
    EventType::ShellPromptDetected,
    EventType::ShellCommandStarted,
    EventType::ShellCommandFinished,
    EventType::ShellCwdChanged,
    EventType::OverlayChanged,
    EventType::ConfigChanged,
    EventType::ConfigError,
    EventType::SessionCreated,
    EventType::SessionClosed,
    EventType::PaneCreated,
    EventType::PaneClosed,
    EventType::PaneFocused,
    EventType::TabCreated,
    EventType::TabClosed,
    EventType::TabFocused,
    EventType::ExtensionLoaded,
    EventType::ExtensionUnloaded,
    EventType::ExtensionMessage,
];

/// Bridges the event bus to the Tauri webview via Channel streaming.
/// Only forwards non-hot-path events to avoid overwhelming the webview IPC.
pub struct TauriBridge {
    bus: SharedEventBus,
    subscriber_ids: Vec<(EventType, SubscriberId)>,
}

impl TauriBridge {
    /// Create a new bridge that forwards non-hot-path events to the given Tauri Channel.
    pub fn new(bus: SharedEventBus, channel: Channel<String>) -> Self {
        let mut subscriber_ids = Vec::new();

        for &event_type in BRIDGE_EVENT_TYPES {
            let channel = channel.clone();
            let id = bus.subscribe(event_type, move |event: &Event| {
                match serde_json::to_string(event) {
                    Ok(json) => {
                        if let Err(e) = channel.send(json) {
                            tracing::warn!(error = %e, "TauriBridge: failed to send event to webview channel");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "TauriBridge: failed to serialize event");
                    }
                }
            });
            subscriber_ids.push((event_type, id));
        }

        Self {
            bus,
            subscriber_ids,
        }
    }
}

impl Drop for TauriBridge {
    fn drop(&mut self) {
        for (event_type, id) in &self.subscriber_ids {
            self.bus.unsubscribe(*event_type, *id);
        }
    }
}

/// Managed state for tracking webview channel subscriptions so they can be cleaned up.
pub struct WebviewSubscriptions {
    bus: SharedEventBus,
    inner: Mutex<Vec<(EventType, SubscriberId)>>,
}

impl WebviewSubscriptions {
    pub fn new(bus: SharedEventBus) -> Self {
        Self {
            bus,
            inner: Mutex::new(Vec::new()),
        }
    }
}

impl Drop for WebviewSubscriptions {
    fn drop(&mut self) {
        let subs = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for (event_type, id) in subs.iter() {
            self.bus.unsubscribe(*event_type, *id);
        }
    }
}

/// Tauri command: publish an event from the webview.
/// Only allows event types in the webview allowlist to prevent spoofing.
#[tauri::command]
pub fn event_bus_emit(
    state: tauri::State<'_, SharedEventBus>,
    event_type: u32,
    payload: String,
) -> Result<(), String> {
    let et = EventType::from_u32(event_type).map_err(|e| e.to_string())?;

    if !WEBVIEW_ALLOWED_EMIT_TYPES.contains(&et) {
        return Err(format!("Event type {} is not allowed from webview", event_type));
    }

    let event = Event {
        event_type: et,
        payload: payload.into_bytes(),
        timestamp_us: Event::now_us(),
        source: Some("webview".to_string()),
    };
    state.publish(event);
    Ok(())
}

/// Maximum number of webview subscriptions to prevent resource exhaustion.
const MAX_WEBVIEW_SUBSCRIPTIONS: usize = 256;

/// Tauri command: start the server-push event bridge.
/// The webview calls this once at startup, passing a Channel that receives
/// all non-hot-path events (defined in BRIDGE_EVENT_TYPES) for the lifetime
/// of the application.
#[tauri::command]
pub fn event_bus_start_bridge(
    state: tauri::State<'_, SharedEventBus>,
    bridge_state: tauri::State<'_, Mutex<Option<TauriBridge>>>,
    channel: Channel<String>,
) -> Result<(), String> {
    let mut slot = bridge_state.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return Err("Event bridge already started".to_string());
    }
    *slot = Some(TauriBridge::new((*state).clone(), channel));
    tracing::info!("TauriBridge started — forwarding {} event types to webview", BRIDGE_EVENT_TYPES.len());
    Ok(())
}

/// Tauri command: subscribe to events via a streaming Channel.
/// Returns subscriber IDs that are tracked internally for cleanup.
#[tauri::command]
pub fn event_bus_subscribe_channel(
    state: tauri::State<'_, SharedEventBus>,
    subs_state: tauri::State<'_, WebviewSubscriptions>,
    event_types: Vec<u32>,
    channel: Channel<String>,
) -> Result<Vec<u64>, String> {
    let mut ids = Vec::new();
    let mut tracked = subs_state.inner.lock().unwrap_or_else(|e| e.into_inner());

    if tracked.len() + event_types.len() > MAX_WEBVIEW_SUBSCRIPTIONS {
        return Err(format!(
            "Subscription limit exceeded: {} active + {} requested > {} max",
            tracked.len(),
            event_types.len(),
            MAX_WEBVIEW_SUBSCRIPTIONS
        ));
    }

    for et_u32 in event_types {
        let et = EventType::from_u32(et_u32).map_err(|e| e.to_string())?;
        let channel = channel.clone();
        let id = state.subscribe(et, move |event: &Event| {
            match serde_json::to_string(event) {
                Ok(json) => {
                    if let Err(e) = channel.send(json) {
                        tracing::warn!(error = %e, "event_bus_subscribe_channel: failed to send to webview");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "event_bus_subscribe_channel: failed to serialize event");
                }
            }
        });
        ids.push(id.as_u64());
        tracked.push((et, id));
    }
    Ok(ids)
}

/// Tauri command: unsubscribe from previously registered channel subscriptions.
#[tauri::command]
pub fn event_bus_unsubscribe_channel(
    state: tauri::State<'_, SharedEventBus>,
    subs_state: tauri::State<'_, WebviewSubscriptions>,
    event_type: u32,
    subscriber_id: u64,
) -> Result<(), String> {
    let et = EventType::from_u32(event_type).map_err(|e| e.to_string())?;
    let sid = SubscriberId::from_raw(subscriber_id);
    state.unsubscribe(et, sid);

    let mut tracked = subs_state.inner.lock().unwrap_or_else(|e| e.into_inner());
    tracked.retain(|(t, id)| !(*t == et && *id == sid));
    Ok(())
}
