mod event_bridge;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tauri::Manager;
use marauder_event_bus::bus;
use marauder_event_bus::events::{Event, EventType};
use marauder_grid::Grid;
use marauder_pty::{PaneId, TauriPtyManager};
use marauder_renderer::{Renderer, RendererConfig};
use marauder_runtime::{MarauderRuntime, RuntimeConfig};

/// Active grid for the focused pane, shared between pipeline and renderer.
type SharedGrid = Arc<Mutex<Grid>>;

/// Shared renderer handle, accessible from Tauri commands.
type SharedRenderer = Arc<Mutex<Option<Renderer>>>;

/// Map of pane_id → grid, shared so focus changes can swap the active grid.
type PaneGridMap = Arc<Mutex<HashMap<PaneId, SharedGrid>>>;

/// Currently focused pane's grid, swapped on PaneFocused events.
type ActiveGrid = Arc<Mutex<Option<SharedGrid>>>;

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

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(event_bus.clone())
        .manage(webview_subs)
        .manage(TauriPtyManager::new())
        .manage(shared_renderer)
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
                                if let Some(pipeline) = rt.pipeline(pane_id) {
                                    marauder_grid::ops::inject_shared_grid(
                                        &mut state,
                                        pane_id as u32,
                                        pipeline.grid.clone(),
                                    );
                                    marauder_parser::ops::inject_shared_parser(
                                        &mut state,
                                        pane_id as u32,
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
                    event_bus_for_thread.subscribe(EventType::GridUpdated, move |_: &Event| {
                        let _ = render_tx_event.try_send(());
                    });

                    let render_thread = std::thread::Builder::new()
                        .name("marauder-render".into())
                        .spawn(move || {
                            loop {
                                // Wait for a render signal or timeout for cursor blink (~30fps)
                                let got_signal = render_rx
                                    .recv_timeout(std::time::Duration::from_millis(33))
                                    .is_ok();

                                // Coalesce queued signals
                                if got_signal {
                                    while render_rx.try_recv().is_ok() {}
                                }

                                let grid = active_grid_for_render
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .clone();

                                if let Some(ref grid) = grid {
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

                    // Main async loop: drive the JsRuntime event loop
                    loop {
                        match js_runtime.run_event_loop(deno_core::PollEventLoopOptions::default()).await {
                            Ok(()) => {
                                tracing::debug!("JsRuntime event loop completed");
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "JsRuntime event loop error");
                                break;
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
            event_bridge::event_bus_subscribe_channel,
            event_bridge::event_bus_unsubscribe_channel,
            renderer_get_cell_size,
            renderer_resize,
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
