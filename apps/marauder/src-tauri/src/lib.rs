mod event_bridge;

use marauder_event_bus::bus;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let event_bus = bus::create_shared();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(event_bus)
        .manage(event_bridge::WebviewSubscriptions::new())
        .invoke_handler(tauri::generate_handler![
            event_bridge::event_bus_emit,
            event_bridge::event_bus_subscribe_channel,
            event_bridge::event_bus_unsubscribe_channel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
