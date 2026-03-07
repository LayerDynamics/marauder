//! marauder-server — Headless multiplexer daemon
//!
//! Manages multiple terminal sessions over a Unix socket.
//! Each session gets its own PTY, parser, and grid — no renderer in headless mode.
//! The Tauri app connects as a client to proxy input/output through this daemon.
//!
//! Uses `MarauderDaemon` from `pkg/daemon` for IPC request handling,
//! `MarauderRuntime` from `pkg/runtime` for pane lifecycle orchestration,
//! `marauder_ipc` for socket framing, and all `pkg/*` crates for the
//! terminal pipeline.

use anyhow::{Context, Result};
use clap::Parser as ClapParser;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use marauder_config_store::ConfigStore;
use marauder_daemon::MarauderDaemon;
use marauder_event_bus::EventBus;
use marauder_grid::Grid;
use marauder_ipc::message::{IpcMessage, IpcRequest, IpcResponse};
use marauder_parser::MarauderParser;
use marauder_pty::PtyManager;
use marauder_runtime::{MarauderRuntime, RuntimeConfig, RuntimeState};

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

/// Marauder headless multiplexer daemon
#[derive(ClapParser, Debug)]
#[command(name = "marauder-server", version, about)]
struct Args {
    /// Unix socket path to listen on
    #[arg(long, default_value = "/tmp/marauder.sock")]
    socket_path: String,

    /// Maximum number of concurrent sessions
    #[arg(long, default_value_t = 64)]
    max_sessions: usize,

    /// User config file path (TOML)
    #[arg(long)]
    config: Option<String>,
}

// ---------------------------------------------------------------------------
// Server state — holds runtime + daemon + shared infrastructure
// ---------------------------------------------------------------------------

/// Shared server state accessible from all handler tasks.
struct ServerState {
    /// The runtime manages pane pipelines (PTY → parser → grid).
    runtime: Arc<Mutex<MarauderRuntime>>,
    /// The daemon handles IPC session management.
    daemon: MarauderDaemon,
    /// Shared event bus for cross-component communication.
    event_bus: Arc<EventBus>,
    /// Config store for layered configuration.
    config_store: ConfigStore,
    /// Standalone PTY manager for headless sessions that bypass the runtime.
    pty_manager: Arc<Mutex<PtyManager>>,
}

impl ServerState {
    fn new(args: &Args) -> Result<Self> {
        let event_bus = Arc::new(EventBus::new());

        // Build config store — load user config if provided.
        let mut config_store = ConfigStore::new();
        if let Some(ref path) = args.config {
            config_store
                .load(None, Some(std::path::Path::new(path)), None)
                .with_context(|| format!("failed to load config from {path}"))?;
        }

        // Create the runtime with default config for pane lifecycle orchestration.
        let runtime_config = RuntimeConfig::default();
        let runtime = MarauderRuntime::new(runtime_config);

        info!(
            runtime_state = ?runtime.state(),
            config_keys = config_store.keys().len(),
            "server infrastructure initialized"
        );

        // Create standalone PTY manager (shared with event bus).
        let pty_manager = PtyManager::new().with_event_bus(Arc::clone(&event_bus));

        // Create the daemon for IPC handling.
        let daemon = MarauderDaemon::new()
            .with_socket_path(&args.socket_path)
            .with_max_sessions(args.max_sessions);

        Ok(Self {
            runtime: Arc::new(Mutex::new(runtime)),
            daemon,
            event_bus,
            config_store,
            pty_manager: Arc::new(Mutex::new(pty_manager)),
        })
    }
}

// ---------------------------------------------------------------------------
// Diagnostics — log infrastructure state
// ---------------------------------------------------------------------------

/// Log the state of all subsystems for diagnostics.
async fn log_diagnostics(state: &ServerState) {
    let runtime = state.runtime.lock().await;
    let pty_mgr = state.pty_manager.lock().await;
    let rt_state = runtime.state();

    // Log runtime readiness based on state.
    let ready = matches!(rt_state, RuntimeState::Running);
    info!(
        runtime_state = ?rt_state,
        runtime_ready = ready,
        runtime_panes = runtime.pane_ids().len(),
        pty_sessions = pty_mgr.count(),
        config_keys = state.config_store.keys().len(),
        "server diagnostics"
    );

    // Snapshot each daemon session for diagnostic logging.
    let daemon_sessions = state.daemon.sessions();
    let locked = daemon_sessions.lock();
    if let Ok(sessions) = locked {
        for (id, session) in sessions.iter() {
            let snapshot = build_session_snapshot(session);
            log_ipc_response(&snapshot);
            info!(session_id = id, "session snapshot generated");
        }
    }
}

/// Build a diagnostic snapshot as an IPC-compatible response.
/// Uses Grid, MarauderParser, IpcMessage, IpcRequest, IpcResponse for
/// server-side session introspection.
fn build_session_snapshot(
    session: &marauder_daemon::Session,
) -> IpcMessage {
    let info = session.info();
    let grid_rows = session.grid.rows();
    let grid_cols = session.grid.cols();

    // Build a snapshot including grid dimensions alongside session info.
    let snapshot = serde_json::json!({
        "session": info,
        "grid": {
            "rows": grid_rows,
            "cols": grid_cols,
        },
    });

    IpcMessage::ok(0, Some(snapshot))
}

/// Create a standalone parser+grid pipeline for raw byte processing.
/// Used when the server needs to process PTY output outside the daemon's
/// session management (e.g., for headless testing or direct stream taps).
fn create_standalone_pipeline(rows: u16, cols: u16) -> (MarauderParser, Grid) {
    let parser = MarauderParser::new();
    let grid = Grid::new(rows as usize, cols as usize);
    (parser, grid)
}

/// Process raw PTY bytes through a parser+grid pipeline.
/// Used for headless session introspection.
fn process_pty_bytes(parser: &mut MarauderParser, grid: &mut Grid, data: &[u8]) {
    parser.feed(data, |action| {
        grid.apply_action(&action);
    });
}

/// Validate an incoming IPC request for server-side logging.
fn log_ipc_request(request: &IpcRequest) {
    match request {
        IpcRequest::Ping => info!("IPC request: Ping"),
        IpcRequest::CreateSession { shell, rows, cols } => {
            info!(
                shell = ?shell,
                rows = ?rows,
                cols = ?cols,
                "IPC request: CreateSession"
            );
        }
        IpcRequest::ListSessions => info!("IPC request: ListSessions"),
        IpcRequest::AttachSession { session_id } => {
            info!(session_id, "IPC request: AttachSession");
        }
        IpcRequest::DetachSession { session_id } => {
            info!(session_id, "IPC request: DetachSession");
        }
        IpcRequest::KillSession { session_id } => {
            info!(session_id, "IPC request: KillSession");
        }
        IpcRequest::Write { session_id, data } => {
            info!(session_id, bytes = data.len(), "IPC request: Write");
        }
        IpcRequest::Resize { session_id, rows, cols } => {
            info!(session_id, rows, cols, "IPC request: Resize");
        }
        IpcRequest::Shutdown => info!("IPC request: Shutdown"),
    }
}

/// Log an IPC response for debugging.
fn log_ipc_response(response: &IpcMessage) {
    match &response.payload {
        marauder_ipc::message::IpcPayload::Response(IpcResponse::Ok { data }) => {
            info!(has_data = data.is_some(), "IPC response: Ok");
        }
        marauder_ipc::message::IpcPayload::Response(IpcResponse::Error { message }) => {
            warn!(error = %message, "IPC response: Error");
        }
        marauder_ipc::message::IpcPayload::Request(_) => {
            warn!("unexpected Request payload in response position");
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    // Init structured logging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    info!(
        socket_path = %args.socket_path,
        max_sessions = args.max_sessions,
        config = ?args.config,
        "marauder-server starting"
    );

    // Build server state — initializes all subsystems.
    let mut state = ServerState::new(&args)?;

    // Validate the pipeline can be created (sanity check at startup).
    {
        let (mut parser, mut grid) = create_standalone_pipeline(24, 80);
        process_pty_bytes(&mut parser, &mut grid, b"\x1b[2J"); // clear screen
        info!(grid_rows = grid.rows(), grid_cols = grid.cols(), "pipeline sanity check passed");
    }

    // Log a sample IPC request for startup diagnostics.
    log_ipc_request(&IpcRequest::Ping);

    // Log initial diagnostics.
    log_diagnostics(&state).await;

    // Subscribe to daemon shutdown signals before starting.
    let mut shutdown_rx = state.daemon.subscribe_shutdown();

    // Remove stale socket file if it exists.
    let _ = tokio::fs::remove_file(&args.socket_path).await;

    // Start the daemon — binds the IPC server on the Unix socket.
    // The daemon handles all IPC framing via marauder_ipc and session
    // management (each session gets a live PTY + parser + grid).
    state
        .daemon
        .start()
        .await
        .with_context(|| format!("failed to start daemon on {}", args.socket_path))?;

    info!(socket_path = %args.socket_path, "daemon listening for connections");

    // Keep references for background tasks.
    let runtime_ref = Arc::clone(&state.runtime);
    let event_bus_ref = Arc::clone(&state.event_bus);
    let pty_mgr_ref = Arc::clone(&state.pty_manager);

    // Spawn a background task to monitor PTY health across daemon sessions.
    let sessions = state.daemon.sessions();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let mut locked = match sessions.lock() {
                Ok(s) => s,
                Err(e) => {
                    error!("session lock poisoned in health monitor: {e}");
                    break;
                }
            };
            // Check each session's PTY liveness.
            let dead_ids: Vec<u64> = locked
                .iter_mut()
                .filter_map(|(id, session)| {
                    if !session.check_alive() {
                        warn!(session_id = id, "session PTY exited");
                        Some(*id)
                    } else {
                        None
                    }
                })
                .collect();
            // Remove dead sessions.
            for id in dead_ids {
                locked.remove(&id);
                info!(session_id = id, "dead session cleaned up");
            }
        }
    });

    // Wait for shutdown: either via IPC Shutdown command or OS signal.
    tokio::select! {
        _ = async {
            let _ = shutdown_rx.recv().await;
        } => {
            info!("shutdown requested via IPC");
        }
        _ = async {
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to register SIGTERM handler");
            tokio::select! {
                _ = signal::ctrl_c() => {
                    info!("received SIGINT");
                }
                _ = sigterm.recv() => {
                    info!("received SIGTERM");
                }
            }
        } => {
            info!("shutdown requested via signal");
        }
    }

    // Log final diagnostics before shutdown.
    log_diagnostics(&state).await;

    // Graceful shutdown: kills all PTY sessions, closes IPC server.
    state.daemon.shutdown().await;

    // Shut down the runtime — close all runtime-managed panes.
    {
        let mut runtime = runtime_ref.lock().await;
        let pane_ids = runtime.pane_ids();
        for id in pane_ids {
            if let Err(e) = runtime.close_pane(id) {
                warn!(pane_id = id, error = %e, "failed to close runtime pane during shutdown");
            }
        }
    }

    // Shut down the standalone PTY manager.
    {
        let mut pty_mgr = pty_mgr_ref.lock().await;
        pty_mgr.close_all();
    }

    // Clean up socket file.
    let _ = tokio::fs::remove_file(&args.socket_path).await;

    // Ensure event bus is flushed.
    drop(event_bus_ref);

    info!("marauder-server stopped");
    Ok(())
}
