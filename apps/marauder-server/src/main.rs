//! marauder-server — Headless multiplexer daemon
//!
//! Manages multiple terminal sessions over a Unix socket (or optional TCP port).
//! Each session gets its own PTY, parser, and grid — no renderer in headless mode.
//! The Tauri app connects as a client to proxy input/output through this daemon.

use anyhow::{Context, Result};
use clap::Parser as ClapParser;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::signal;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};

use marauder_config_store::ConfigStore;
use marauder_event_bus::EventBus;
use marauder_grid::Grid;
use marauder_parser::MarauderParser;
use marauder_pty::PtyManager;

// TODO: import session/lifecycle helpers from marauder_runtime when API is finalized
// use marauder_runtime::Runtime;

// TODO: import IPC message framing from marauder_ipc when API is finalized
// use marauder_ipc::{IpcMessage, IpcFrame};

// TODO: import daemon supervision helpers from marauder_daemon when API is finalized
// use marauder_daemon::ProcessSupervisor;

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

    /// Optional TCP port to also listen on (in addition to the Unix socket)
    #[arg(long)]
    port: Option<u16>,

    /// Maximum number of concurrent sessions
    #[arg(long, default_value_t = 64)]
    max_sessions: usize,
}

// ---------------------------------------------------------------------------
// Session — one PTY + parser + grid per connected client
// ---------------------------------------------------------------------------

/// Unique identifier for a client session.
type SessionId = u64;

/// Holds the live state for a single terminal session.
struct Session {
    id: SessionId,
    // TODO: replace with real PtyManager handle once API is finalised
    _pty: PtyManager,
    parser: MarauderParser,
    grid: Grid,
    /// Channel used to push output bytes back to the connected client.
    tx: mpsc::Sender<Vec<u8>>,
}

impl Session {
    /// Spawn a new session: create PTY, parser, and grid.
    ///
    /// `rows`/`cols` come from the client's initial handshake; default to 24×80
    /// until the client sends a resize message.
    fn new(id: SessionId, rows: u16, cols: u16, tx: mpsc::Sender<Vec<u8>>) -> Result<Self> {
        // TODO: thread shell path + env from config store
        let pty = PtyManager::new();
        let parser = MarauderParser::new();
        let grid = Grid::new(rows as usize, cols as usize);

        Ok(Self {
            id,
            _pty: pty,
            parser,
            grid,
            tx,
        })
    }

    /// Feed raw bytes from the PTY into the parser and apply resulting actions
    /// to the grid, then forward the raw bytes to the client.
    async fn handle_pty_output(&mut self, data: Vec<u8>) -> Result<()> {
        // TODO: use real parser.feed(data, callback) API when available.
        // The callback receives each TerminalAction and calls grid.apply_action(action).
        //
        // Pseudocode (uncomment when parser/grid APIs are finalised):
        //
        // self.parser.feed(&data, |action| {
        //     self.grid.apply_action(action);
        // });

        // Forward raw bytes to the client so it can render / display.
        if self.tx.send(data).await.is_err() {
            warn!(session_id = self.id, "client receiver dropped");
        }
        Ok(())
    }

    /// Write input bytes received from the client into the PTY.
    fn write_input(&mut self, data: &[u8]) -> Result<()> {
        // TODO: use real pty_write API when available.
        // self._pty.write(data).context("pty write failed")?;
        let _ = data; // suppress unused warning until API is wired
        Ok(())
    }

    /// Resize the PTY and grid when the client sends a resize event.
    fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        // TODO: self._pty.resize(rows, cols)?;
        // TODO: self.grid.resize(rows as usize, cols as usize);
        let _ = (rows, cols);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Session registry
// ---------------------------------------------------------------------------

struct SessionRegistry {
    sessions: std::collections::HashMap<SessionId, Arc<Mutex<Session>>>,
    next_id: SessionId,
    max_sessions: usize,
}

impl SessionRegistry {
    fn new(max_sessions: usize) -> Self {
        Self {
            sessions: std::collections::HashMap::new(),
            next_id: 1,
            max_sessions,
        }
    }

    fn create(
        &mut self,
        rows: u16,
        cols: u16,
        tx: mpsc::Sender<Vec<u8>>,
    ) -> Result<SessionId> {
        if self.sessions.len() >= self.max_sessions {
            anyhow::bail!("max session limit ({}) reached", self.max_sessions);
        }
        let id = self.next_id;
        self.next_id += 1;
        let session = Session::new(id, rows, cols, tx)?;
        self.sessions.insert(id, Arc::new(Mutex::new(session)));
        info!(session_id = id, "session created");
        Ok(id)
    }

    fn get(&self, id: SessionId) -> Option<Arc<Mutex<Session>>> {
        self.sessions.get(&id).cloned()
    }

    fn remove(&mut self, id: SessionId) {
        if self.sessions.remove(&id).is_some() {
            info!(session_id = id, "session removed");
        }
    }
}

// ---------------------------------------------------------------------------
// Client connection handler
// ---------------------------------------------------------------------------

/// Handle a single client connection for its full lifetime.
///
/// Protocol (placeholder framing — replace with `marauder_ipc` framing):
///   - Client sends length-prefixed JSON messages.
///   - Server sends length-prefixed JSON or raw PTY bytes back.
async fn handle_client(
    stream: tokio::net::UnixStream,
    registry: Arc<Mutex<SessionRegistry>>,
    _event_bus: Arc<EventBus>,
) {
    // TODO: replace ad-hoc framing with marauder_ipc::IpcFrame read/write helpers.

    // Channel for PTY output → client
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(256);

    // Create a session with default 24×80 size; client should send resize immediately.
    let session_id = {
        let mut reg = registry.lock().await;
        match reg.create(24, 80, tx) {
            Ok(id) => id,
            Err(e) => {
                error!("failed to create session: {e}");
                return;
            }
        }
    };

    // Split the Unix stream so we can read and write concurrently.
    let (reader, mut writer) = stream.into_split();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut reader = tokio::io::BufReader::new(reader);

    // Spawn a task to forward PTY output to the client socket.
    let write_task = tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            if writer.write_all(&data).await.is_err() {
                break;
            }
        }
    });

    // Read loop: receive input from client and forward to PTY / handle control messages.
    let mut buf = vec![0u8; 4096];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => {
                info!(session_id, "client disconnected");
                break;
            }
            Ok(n) => {
                let data = buf[..n].to_vec();
                let reg = registry.lock().await;
                if let Some(session_arc) = reg.get(session_id) {
                    drop(reg); // release registry lock before locking session
                    let mut session = session_arc.lock().await;
                    if let Err(e) = session.write_input(&data) {
                        error!(session_id, "pty write error: {e}");
                        break;
                    }
                } else {
                    break;
                }
            }
            Err(e) => {
                error!(session_id, "read error: {e}");
                break;
            }
        }
    }

    // Clean up session.
    write_task.abort();
    registry.lock().await.remove(session_id);
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
        port = ?args.port,
        max_sessions = args.max_sessions,
        "marauder-server starting"
    );

    // Initialise shared infrastructure.
    let event_bus = Arc::new(EventBus::new());
    // TODO: load config from file via ConfigStore::open(path)?
    let _config_store = ConfigStore::new();

    let registry = Arc::new(Mutex::new(SessionRegistry::new(args.max_sessions)));

    // Remove stale socket file if it exists.
    let _ = tokio::fs::remove_file(&args.socket_path).await;

    // Bind Unix socket.
    let listener = UnixListener::bind(&args.socket_path)
        .with_context(|| format!("failed to bind Unix socket at {}", args.socket_path))?;

    info!(socket_path = %args.socket_path, "listening for connections");

    // Optional TCP listener (parallel accept loop).
    if let Some(port) = args.port {
        let tcp_listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .with_context(|| format!("failed to bind TCP port {port}"))?;
        info!(port, "also listening on TCP");

        let reg_clone = Arc::clone(&registry);
        let eb_clone = Arc::clone(&event_bus);
        tokio::spawn(async move {
            loop {
                match tcp_listener.accept().await {
                    Ok((_tcp_stream, addr)) => {
                        info!(%addr, "TCP client connected");
                        // TODO: wrap TcpStream in a compatibility shim or migrate
                        // handle_client to accept a generic AsyncRead+AsyncWrite.
                        // For now, log and drop — Unix socket is the primary transport.
                        warn!("TCP session handling not yet implemented; dropping {addr}");
                        let _ = (Arc::clone(&reg_clone), Arc::clone(&eb_clone));
                    }
                    Err(e) => {
                        error!("TCP accept error: {e}");
                    }
                }
            }
        });
    }

    // Graceful shutdown channel.
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // Listen for SIGTERM / SIGINT.
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        let mut sigterm =
            signal::unix::signal(signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("received SIGINT, shutting down");
            }
            _ = sigterm.recv() => {
                info!("received SIGTERM, shutting down");
            }
        }
        let _ = shutdown_tx_clone.send(()).await;
    });

    // Main accept loop.
    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _addr)) => {
                        info!("Unix client connected");
                        let reg = Arc::clone(&registry);
                        let eb = Arc::clone(&event_bus);
                        tokio::spawn(handle_client(stream, reg, eb));
                    }
                    Err(e) => {
                        error!("accept error: {e}");
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                info!("shutdown signal received; stopping accept loop");
                break;
            }
        }
    }

    // Clean up socket file.
    let _ = tokio::fs::remove_file(&args.socket_path).await;
    info!("marauder-server stopped");

    Ok(())
}
