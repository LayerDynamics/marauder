mod event_bridge;

use marauder_event_bus::bus;
use marauder_pty::TauriPtyManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let event_bus = bus::create_shared();
    let webview_subs = event_bridge::WebviewSubscriptions::new(event_bus.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(event_bus)
        .manage(webview_subs)
        .manage(TauriPtyManager::new())
        .invoke_handler(tauri::generate_handler![
            event_bridge::event_bus_emit,
            event_bridge::event_bus_subscribe_channel,
            event_bridge::event_bus_unsubscribe_channel,
            marauder_pty::commands::pty_cmd_create,
            marauder_pty::commands::pty_cmd_write,
            marauder_pty::commands::pty_cmd_read,
            marauder_pty::commands::pty_cmd_resize,
            marauder_pty::commands::pty_cmd_close,
            marauder_pty::commands::pty_cmd_get_pid,
            marauder_pty::commands::pty_cmd_wait,
            marauder_pty::commands::pty_cmd_list,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
