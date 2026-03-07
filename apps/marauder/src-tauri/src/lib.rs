mod event_bridge;
mod ipc_bridge;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use tauri::{Emitter, Manager};
use marauder_event_bus::bus;
use marauder_event_bus::events::{Event, EventType};
use marauder_grid::Grid;
use marauder_grid::{SharedGrid, PaneGridMap};
use marauder_pty::{PaneId, TauriPtyManager};
use marauder_renderer::{Renderer, RendererConfig};
use marauder_config_store::commands::TauriConfigStore;
use marauder_runtime::{MarauderRuntime, RuntimeConfig};
use marauder_runtime::commands::TauriRuntimeHandle;

/// Shared renderer handle, accessible from Tauri commands.
type SharedRenderer = Arc<Mutex<Option<Renderer>>>;

/// Currently focused pane's grid, swapped on PaneFocused events.
type ActiveGrid = Arc<Mutex<Option<SharedGrid>>>;

/// Shared extension bridge channel for extension <-> webview communication.
type ExtensionBridgeChannel = Arc<Mutex<Option<tauri::ipc::Channel<String>>>>;

/// Tauri command: start the extension bridge channel.
#[tauri::command]
fn extension_start_bridge(
    state: tauri::State<'_, ExtensionBridgeChannel>,
    channel: tauri::ipc::Channel<String>,
) -> Result<(), String> {
    let mut bridge = state.lock().unwrap_or_else(|e| e.into_inner());
    *bridge = Some(channel);
    tracing::info!("Extension bridge channel started");
    Ok(())
}

/// Tauri command: post a message from the webview to an extension via the event bus.
#[tauri::command]
fn extension_post_message(
    event_bus: tauri::State<'_, bus::SharedEventBus>,
    extension_name: String,
    message_type: String,
    data: String,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "source": "webview",
        "target": extension_name,
        "type": message_type,
        "payload": data,
    });
    let bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    let event = Event::new(EventType::ExtensionMessage, bytes);
    event_bus.publish(event);
    Ok(())
}

/// Tauri command: register an extension panel via the event bus.
///
/// Publishes an ExtensionMessage event so the Deno-side PanelRegistry receives
/// the panel configuration and registers it. The webview calls this when an
/// extension requests a panel via the extension bridge.
#[tauri::command]
fn extension_register_panel(
    event_bus: tauri::State<'_, bus::SharedEventBus>,
    config: serde_json::Value,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "source": "webview",
        "type": "RegisterPanel",
        "payload": config,
    });
    let bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    let event = Event::new(EventType::ExtensionMessage, bytes);
    event_bus.publish(event);
    tracing::info!("Extension panel registration published to event bus");
    Ok(())
}

/// Tauri command: list loaded extensions by querying the Deno runtime.
///
/// Sends a `deno_call_op` request to invoke the runtime's extension listing
/// op, returning the result as a JSON array. Falls back to querying the event
/// bus for extension load events if the Deno bridge is unavailable.
#[tauri::command]
async fn extension_list(
    bridge: tauri::State<'_, ipc_bridge::DenoBridge>,
) -> Result<Vec<serde_json::Value>, String> {
    // Query the Deno runtime for the list of loaded extensions via the IPC bridge.
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    bridge
        .tx
        .send(ipc_bridge::DenoRequest::Eval {
            code: "JSON.stringify(globalThis.__marauder?.extensions?.list?.() ?? [])".into(),
            reply: reply_tx,
        })
        .await
        .map_err(|e| format!("failed to send to Deno bridge: {e}"))?;

    match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
        Ok(Ok(Ok(json_str))) => {
            let list: Vec<serde_json::Value> =
                serde_json::from_str(&json_str).unwrap_or_default();
            Ok(list)
        }
        Ok(Ok(Err(e))) => {
            tracing::warn!(error = %e, "Deno extension_list returned error");
            Ok(vec![])
        }
        Ok(Err(_)) => {
            tracing::warn!("Deno bridge reply channel dropped");
            Ok(vec![])
        }
        Err(_) => {
            tracing::warn!("Deno bridge timed out querying extension list");
            Ok(vec![])
        }
    }
}

/// Tauri command: get the grid dimensions for the active pane.
///
/// Returns (rows, cols) by reading directly from the Grid.
#[tauri::command]
fn grid_get_active_dimensions(
    active_grid: tauri::State<'_, ActiveGrid>,
) -> Result<(usize, usize), String> {
    let guard = active_grid.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(shared_grid) => {
            let grid: std::sync::MutexGuard<'_, Grid> =
                shared_grid.lock().unwrap_or_else(|e| e.into_inner());
            Ok((grid.rows(), grid.cols()))
        }
        None => Err("No active grid".into()),
    }
}

/// Tauri command: get the renderer's cell size (width, height) in pixels.
#[tauri::command]
fn renderer_get_cell_size(
    state: tauri::State<'_, SharedRenderer>,
) -> Result<(f32, f32), String> {
    let rend = state.lock().unwrap_or_else(|e| e.into_inner());
    match rend.as_ref() {
        Some(r) => Ok(r.cell_size()),
        None => Err("Renderer not initialized".into()),
    }
}

/// Tauri command: set pane border quads for split pane dividers.
#[tauri::command]
fn renderer_set_pane_borders(
    state: tauri::State<'_, SharedRenderer>,
    borders: Vec<marauder_renderer::PaneBorder>,
) -> Result<(), String> {
    let mut rend = state.lock().unwrap_or_else(|e| e.into_inner());
    match rend.as_mut() {
        Some(r) => {
            r.set_pane_borders(borders);
            Ok(())
        }
        None => Err("Renderer not initialized".into()),
    }
}

/// Tauri command: set subpixel scroll offset for smooth scrolling.
#[tauri::command]
fn renderer_set_scroll_offset(
    state: tauri::State<'_, SharedRenderer>,
    offset: f32,
) -> Result<(), String> {
    let mut rend = state.lock().unwrap_or_else(|e| e.into_inner());
    match rend.as_mut() {
        Some(r) => {
            r.set_scroll_offset(offset);
            Ok(())
        }
        None => Err("Renderer not initialized".into()),
    }
}

/// Tauri command: mark renderer activity for adaptive frame rate.
#[tauri::command]
fn renderer_mark_activity(
    state: tauri::State<'_, SharedRenderer>,
) -> Result<(), String> {
    let mut rend = state.lock().unwrap_or_else(|e| e.into_inner());
    match rend.as_mut() {
        Some(r) => {
            r.mark_activity();
            Ok(())
        }
        None => Err("Renderer not initialized".into()),
    }
}

/// Tauri command: notify the renderer of a window resize.
#[tauri::command]
fn renderer_resize(
    state: tauri::State<'_, SharedRenderer>,
    width: u32,
    height: u32,
    scale_factor: f32,
) -> Result<(), String> {
    let mut rend = state.lock().unwrap_or_else(|e| e.into_inner());
    match rend.as_mut() {
        Some(r) => {
            r.resize_surface(width, height, scale_factor);
            Ok(())
        }
        None => Err("Renderer not initialized".into()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing subscriber so tracing::* macros produce output.
    // Respects RUST_LOG env var (e.g. RUST_LOG=marauder=debug,wgpu=warn).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("marauder=info,warn")),
        )
        .with_target(true)
        .init();

    let event_bus = bus::create_shared();
    let webview_subs = event_bridge::WebviewSubscriptions::new(event_bus.clone());

    let active_grid: ActiveGrid = Arc::new(Mutex::new(None));
    let pane_grids: PaneGridMap = Arc::new(Mutex::new(HashMap::new()));
    let shared_renderer: SharedRenderer = Arc::new(Mutex::new(None));
    let extension_bridge: ExtensionBridgeChannel = Arc::new(Mutex::new(None));

    let active_grid_for_setup = active_grid.clone();
    let pane_grids_for_setup = pane_grids.clone();
    let event_bus_for_setup = event_bus.clone();
    let renderer_for_setup = shared_renderer.clone();

    // Listen for PaneFocused events to swap the active grid
    let active_grid_for_focus = active_grid.clone();
    let pane_grids_for_focus = pane_grids.clone();
    event_bus.subscribe(EventType::PaneFocused, move |event: &Event| {
        if let Ok(pane_id) = event.payload_as::<PaneId>() {
            let grids = pane_grids_for_focus.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(grid) = grids.get(&pane_id) {
                *active_grid_for_focus.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(grid));
                tracing::debug!(pane_id, "Switched active grid for rendering");
            }
        }
    });

    // Pre-create Tauri managed state wrappers — populated inside setup thread after boot.
    let tauri_config_store = TauriConfigStore::new();
    let config_store_for_setup = tauri_config_store.clone();

    let shared_runtime: Arc<OnceLock<Arc<Mutex<MarauderRuntime>>>> =
        Arc::new(OnceLock::new());
    let runtime_for_setup = shared_runtime.clone();

    let (deno_tx, deno_rx) = tokio::sync::mpsc::channel::<ipc_bridge::DenoRequest>(64);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(event_bus.clone())
        .manage(webview_subs)
        .manage(TauriPtyManager::new())
        .manage(active_grid.clone())
        .manage(shared_renderer)
        .manage(pane_grids.clone())
        .manage(tauri_config_store)
        .manage(TauriRuntimeHandle::new(shared_runtime))
        .manage(ipc_bridge::DenoBridge { tx: deno_tx })
        .manage(Mutex::new(Option::<event_bridge::TauriBridge>::None))
        .manage(extension_bridge)
        .setup(move |app| {
            let window = app.get_webview_window("main")
                .expect("main window not found");

            let active_grid_for_thread = active_grid_for_setup.clone();
            let pane_grids_for_thread = pane_grids_for_setup.clone();
            let event_bus_for_thread = event_bus_for_setup.clone();
            let renderer_for_thread = renderer_for_setup.clone();
            let window_arc = Arc::new(window.clone());
            let size = window.inner_size().unwrap_or(tauri::PhysicalSize::new(800, 600));
            let scale = window.scale_factor().unwrap_or(1.0) as f32;

            let config_store_for_thread = config_store_for_setup.clone();
            let runtime_handle_for_thread = runtime_for_setup.clone();

            // Clone window for error reporting from the background thread
            let window_for_error = window.clone();

            // Spawn everything on a dedicated thread with its own tokio runtime
            let mut deno_rx = deno_rx;
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
                        let msg = format!("Runtime boot failed: {}", e);
                        let _ = window_for_error.set_title(&format!("Marauder — {}", msg));
                        let _ = window_for_error.emit("marauder://runtime-error", msg);
                        return;
                    }

                    // Create initial pane
                    match runtime.create_pane() {
                        Ok(pane_id) => {
                            tracing::info!(pane_id, "Initial pane created");
                            if let Some(pipeline) = runtime.pipeline(pane_id) {
                                let grid = Arc::clone(&pipeline.grid);
                                // Register in pane grid map and set as active
                                pane_grids_for_thread.lock().unwrap_or_else(|e| e.into_inner())
                                    .insert(pane_id, Arc::clone(&grid));
                                *active_grid_for_thread.lock().unwrap_or_else(|e| e.into_inner()) = Some(grid);
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to create initial pane");
                        }
                    }

                    // Wrap runtime in Arc<Mutex<>> so the PaneCreated handler can
                    // access pipeline grids (the real ones connected to PTY data).
                    let runtime = Arc::new(Mutex::new(runtime));

                    // Inject runtime into Tauri managed state so commands can access it.
                    let _ = runtime_handle_for_thread.set(Arc::clone(&runtime));

                    // Inject the real config store into the Tauri managed state.
                    {
                        let rt = runtime.lock().unwrap_or_else(|e| e.into_inner());
                        config_store_for_thread.inject(rt.config_store().clone());
                    }

                    // Listen for new panes to register their grids from the runtime pipeline
                    let pane_grids_for_new = pane_grids_for_thread.clone();
                    let runtime_for_pane_handler = Arc::clone(&runtime);
                    event_bus_for_thread.subscribe(EventType::PaneCreated, move |event: &Event| {
                        if let Ok(pane_id) = event.payload_as::<PaneId>() {
                            let rt = runtime_for_pane_handler.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(pipeline) = rt.pipeline(pane_id) {
                                let grid = Arc::clone(&pipeline.grid);
                                pane_grids_for_new.lock().unwrap_or_else(|e| e.into_inner())
                                    .insert(pane_id, grid);
                                tracing::debug!(pane_id, "Registered pipeline grid for new pane");
                            } else {
                                tracing::warn!(pane_id, "PaneCreated event but no pipeline found in runtime");
                            }
                        }
                    });

                    // Init wgpu renderer
                    let renderer_config = RendererConfig::default();
                    match Renderer::new(
                        window_arc,
                        size.width,
                        size.height,
                        scale,
                        renderer_config,
                    ).await {
                        Ok(renderer) => {
                            tracing::info!("wgpu renderer initialized");
                            *renderer_for_thread.lock().unwrap_or_else(|e| e.into_inner()) = Some(renderer);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to initialize wgpu renderer");
                            return;
                        }
                    };

                    // Initialize deno_core JsRuntime with all ops extensions
                    let mut js_runtime = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
                        extensions: vec![
                            marauder_pty::ops::pty_extension(),
                            marauder_event_bus::ops::event_bus_extension(),
                            marauder_parser::ops::parser_extension(),
                            marauder_grid::ops::grid_extension(),
                            marauder_config_store::ops::config_store_extension(),
                            marauder_runtime::ops::runtime_extension(),
                        ],
                        ..Default::default()
                    });

                    // Inject shared state from the real runtime into OpState so JS ops
                    // operate on the same subsystems as the Rust pipeline (not phantom copies).
                    {
                        let op_state = js_runtime.op_state();
                        let mut state = op_state.borrow_mut();

                        // Share the real event bus
                        marauder_event_bus::ops::inject_shared_event_bus(
                            &mut state,
                            event_bus_for_thread.clone(),
                        );

                        // Share the real PTY manager
                        {
                            let rt = runtime.lock().unwrap_or_else(|e| e.into_inner());
                            marauder_pty::ops::inject_shared_pty_manager(
                                &mut state,
                                rt.pty_manager().clone(),
                            );

                            // Share the real config store
                            marauder_config_store::ops::inject_shared_config_store(
                                &mut state,
                                0, // handle 0 = primary config store
                                rt.config_store().clone(),
                            );

                            // Pre-register existing pane grids and parsers
                            for pane_id in rt.pane_ids() {
                                let handle: u32 = match pane_id.try_into() {
                                    Ok(h) => h,
                                    Err(_) => {
                                        tracing::error!(pane_id, "PaneId exceeds u32 range, skipping op registration");
                                        continue;
                                    }
                                };
                                if let Some(pipeline) = rt.pipeline(pane_id) {
                                    marauder_grid::ops::inject_shared_grid(
                                        &mut state,
                                        handle,
                                        pipeline.grid.clone(),
                                    );
                                    marauder_parser::ops::inject_shared_parser(
                                        &mut state,
                                        handle,
                                        pipeline.parser.clone(),
                                    );
                                }
                            }
                        }

                        // Mark the primary runtime as attached so JS knows not to create a new one
                        marauder_runtime::ops::mark_primary_attached(&mut state);

                        tracing::info!("Injected shared runtime state into JsRuntime OpState");
                    }

                    // Run bootstrap script to set up JS-side API surface
                    js_runtime
                        .execute_script(
                            "[marauder:bootstrap]",
                            deno_core::FastString::from_static(include_str!("../../src/bootstrap.js")),
                        )
                        .expect("Failed to execute bootstrap script");

                    tracing::info!("deno_core JsRuntime initialized with all ops");

                    // Render loop runs on a dedicated OS thread so vsync/present()
                    // never blocks the tokio current_thread runtime (which drives
                    // the JsRuntime event loop, cursor blink timer, and async ops).
                    let active_grid_for_render = active_grid_for_thread;
                    let renderer_for_render = Arc::clone(&renderer_for_thread);
                    let (render_tx, render_rx) = std::sync::mpsc::sync_channel::<()>(4);

                    let render_tx_event = render_tx.clone();
                    let renderer_for_activity = Arc::clone(&renderer_for_thread);
                    event_bus_for_thread.subscribe(EventType::GridUpdated, move |_: &Event| {
                        // Mark activity on PTY output for adaptive frame rate
                        if let Ok(mut rend) = renderer_for_activity.lock() {
                            if let Some(ref mut r) = *rend {
                                r.mark_activity();
                            }
                        }
                        let _ = render_tx_event.try_send(());
                    });

                    let render_thread = std::thread::Builder::new()
                        .name("marauder-render".into())
                        .spawn(move || {
                            loop {
                                // Wait for a render signal or timeout for cursor blink (~30fps)
                                match render_rx.recv_timeout(std::time::Duration::from_millis(33)) {
                                    Ok(()) => {
                                        // Coalesce queued signals
                                        while render_rx.try_recv().is_ok() {}
                                    }
                                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                        // Normal timeout — render for cursor blink
                                    }
                                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                        tracing::info!("Render channel disconnected, exiting render loop");
                                        break;
                                    }
                                }

                                let grid = active_grid_for_render
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .clone();

                                if let Some(ref grid) = grid {
                                    // Renderer lock is held for upload+present. The grid lock
                                    // inside render_frame is scoped to build_instances only,
                                    // so Tauri commands that need the grid are not blocked
                                    // during GPU work.
                                    let mut rend = renderer_for_render
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner());
                                    if let Some(ref mut renderer) = *rend {
                                        match renderer.render_frame(grid) {
                                            Ok(()) => {}
                                            Err(wgpu::SurfaceError::Lost) => {
                                                tracing::debug!("Surface lost, waiting for resize");
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
                            }
                        })
                        .expect("Failed to spawn render thread");

                    // Main async loop: drive the JsRuntime event loop + IPC bridge
                    loop {
                        tokio::select! {
                            result = js_runtime.run_event_loop(deno_core::PollEventLoopOptions::default()) => {
                                match result {
                                    Ok(()) => {
                                        tracing::debug!("JsRuntime event loop completed");
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "JsRuntime event loop error");
                                        break;
                                    }
                                }
                            }
                            Some(request) = deno_rx.recv() => {
                                match request {
                                    ipc_bridge::DenoRequest::Eval { code, reply } => {
                                        #[cfg(debug_assertions)]
                                        {
                                            let wrapped = format!(
                                                "((r) => typeof r === 'undefined' ? 'undefined' : JSON.stringify(r))({})",
                                                code
                                            );
                                            let result = js_runtime
                                                .execute_script("<eval>", deno_core::FastString::from(wrapped));
                                            let reply_result = match result {
                                                Ok(global) => {
                                                    deno_core::scope!(scope, js_runtime);
                                                    let local = deno_core::v8::Local::new(scope, global);
                                                    Ok(local.to_rust_string_lossy(scope))
                                                }
                                                Err(e) => Err(e.to_string()),
                                            };
                                            let _ = reply.send(reply_result);
                                        }
                                        #[cfg(not(debug_assertions))]
                                        {
                                            let _ = reply.send(Err("deno_eval is disabled in release builds".to_string()));
                                        }
                                    }
                                    ipc_bridge::DenoRequest::CallOp { op_name, args, reply } => {
                                        let args_js = args
                                            .iter()
                                            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "null".into()))
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        let js = format!(
                                            "JSON.stringify(Deno.core.ops.{}({}))",
                                            op_name,
                                            args_js
                                        );
                                        let result = js_runtime
                                            .execute_script("<call_op>", deno_core::FastString::from(js));
                                        let reply_result = match result {
                                            Ok(global) => {
                                                deno_core::scope!(scope, js_runtime);
                                                let local = deno_core::v8::Local::new(scope, global);
                                                Ok(local.to_rust_string_lossy(scope))
                                            }
                                            Err(e) => Err(e.to_string()),
                                        };
                                        let _ = reply.send(reply_result);
                                    }
                                }
                            }
                        }
                    }

                    // Signal render thread to stop by dropping the sender
                    drop(render_tx);
                    let _ = render_thread.join();

                    // Keep runtime alive until render loop ends
                    let mut rt = runtime.lock().unwrap_or_else(|e| e.into_inner());
                    rt.shutdown().await.ok();
                });
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            event_bridge::event_bus_emit,
            event_bridge::event_bus_start_bridge,
            event_bridge::event_bus_subscribe_channel,
            event_bridge::event_bus_unsubscribe_channel,
            grid_get_active_dimensions,
            renderer_get_cell_size,
            renderer_resize,
            renderer_set_pane_borders,
            renderer_set_scroll_offset,
            renderer_mark_activity,
            marauder_pty::commands::pty_cmd_create,
            marauder_pty::commands::pty_cmd_write,
            marauder_pty::commands::pty_cmd_read,
            marauder_pty::commands::pty_cmd_resize,
            marauder_pty::commands::pty_cmd_close,
            marauder_pty::commands::pty_cmd_get_pid,
            marauder_pty::commands::pty_cmd_wait,
            marauder_pty::commands::pty_cmd_list,
            marauder_config_store::commands::config_cmd_get,
            marauder_config_store::commands::config_cmd_set,
            marauder_config_store::commands::config_cmd_keys,
            marauder_config_store::commands::config_cmd_save,
            marauder_config_store::commands::config_cmd_reload,
            marauder_grid::commands::grid_cmd_get_cursor,
            marauder_grid::commands::grid_cmd_get_cell,
            marauder_grid::commands::grid_cmd_get_selection_text,
            marauder_grid::commands::grid_cmd_set_selection,
            marauder_grid::commands::grid_cmd_clear_selection,
            marauder_grid::commands::grid_cmd_scroll_viewport,
            marauder_grid::commands::grid_cmd_scroll_viewport_by,
            marauder_grid::commands::grid_cmd_get_dimensions,
            marauder_grid::commands::grid_cmd_get_screen_snapshot,
            marauder_runtime::commands::runtime_cmd_state,
            marauder_runtime::commands::runtime_cmd_pane_ids,
            marauder_runtime::commands::runtime_cmd_create_pane,
            marauder_runtime::commands::runtime_cmd_close_pane,
            ipc_bridge::deno_eval,
            ipc_bridge::deno_call_op,
            ipc_bridge::resolve_keybinding,
            extension_start_bridge,
            extension_post_message,
            extension_register_panel,
            extension_list,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
