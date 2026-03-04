mod event_bridge;

use std::sync::{Arc, Mutex};

use tauri::Manager;
use marauder_event_bus::bus;
use marauder_event_bus::events::{Event, EventType};
use marauder_grid::Grid;
use marauder_pty::TauriPtyManager;
use marauder_renderer::{Renderer, RendererConfig};
use marauder_runtime::{MarauderRuntime, RuntimeConfig};

/// Active grid for the focused pane, shared between pipeline and renderer.
type SharedGrid = Arc<Mutex<Grid>>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let event_bus = bus::create_shared();
    let webview_subs = event_bridge::WebviewSubscriptions::new(event_bus.clone());

    let active_grid: Arc<Mutex<Option<SharedGrid>>> = Arc::new(Mutex::new(None));

    let active_grid_for_setup = active_grid.clone();
    let event_bus_for_setup = event_bus.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(event_bus.clone())
        .manage(webview_subs)
        .manage(TauriPtyManager::new())
        .setup(move |app| {
            let window = app.get_webview_window("main")
                .expect("main window not found");

            let active_grid_for_thread = active_grid_for_setup.clone();
            let event_bus_for_thread = event_bus_for_setup.clone();
            let window_arc = Arc::new(window.clone());
            let size = window.inner_size().unwrap_or(tauri::PhysicalSize::new(800, 600));
            let scale = window.scale_factor().unwrap_or(1.0) as f32;

            // Spawn everything on a dedicated thread with its own tokio runtime
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();

                rt.block_on(async move {
                    // Boot runtime
                    let rt_config = RuntimeConfig::default();
                    let mut runtime = MarauderRuntime::new(rt_config);

                    if let Err(e) = runtime.boot().await {
                        tracing::error!(error = %e, "Failed to boot Marauder runtime");
                        return;
                    }

                    // Create initial pane
                    match runtime.create_pane() {
                        Ok(pane_id) => {
                            tracing::info!(pane_id, "Initial pane created");
                            if let Some(pipeline) = runtime.pipeline(pane_id) {
                                let grid = Arc::clone(&pipeline.grid);
                                *active_grid_for_thread.lock().unwrap() = Some(grid);
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to create initial pane");
                        }
                    }

                    // Init wgpu renderer
                    let renderer_config = RendererConfig::default();
                    let renderer = match Renderer::new(
                        window_arc,
                        size.width,
                        size.height,
                        scale,
                        renderer_config,
                    ).await {
                        Ok(r) => {
                            tracing::info!("wgpu renderer initialized");
                            r
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to initialize wgpu renderer");
                            return;
                        }
                    };

                    let renderer = Arc::new(Mutex::new(renderer));

                    // Render loop: triggered by GridUpdated events + idle timer
                    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(4);

                    let tx_event = tx.clone();
                    event_bus_for_thread.subscribe(EventType::GridUpdated, move |_: &Event| {
                        let _ = tx_event.try_send(());
                    });

                    // Idle timer for cursor blink (~30fps)
                    let tx_timer = tx;
                    tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(33)).await;
                            if tx_timer.try_send(()).is_err() {
                                break;
                            }
                        }
                    });

                    let active_grid_for_render = active_grid_for_thread;
                    loop {
                        if rx.recv().await.is_none() {
                            break;
                        }
                        // Drain queued signals (coalesce)
                        while rx.try_recv().is_ok() {}

                        let grid = active_grid_for_render.lock().unwrap().clone();
                        if let Some(ref grid) = grid {
                            let mut rend = renderer.lock().unwrap();
                            match rend.render_frame(grid) {
                                Ok(()) => {}
                                Err(wgpu::SurfaceError::Lost) => {
                                    rend.resize_surface(size.width, size.height, scale);
                                }
                                Err(wgpu::SurfaceError::OutOfMemory) => {
                                    tracing::error!("GPU out of memory");
                                    break;
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "Render frame error");
                                }
                            }
                        }
                    }

                    // Keep runtime alive until render loop ends
                    runtime.shutdown().await.ok();
                });
            });

            Ok(())
        })
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
