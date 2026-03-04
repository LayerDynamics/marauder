//! Core runtime lifecycle: boot → run → shutdown.
//!
//! The `MarauderRuntime` struct coordinates all subsystems:
//! - Event bus (pub/sub for all layers)
//! - Config store (layered TOML config + file watching)
//! - PTY manager (process lifecycle)
//! - Pipelines (PTY → parser → grid per pane)
//! - Lifecycle hooks (async Deno notifications)
//!
//! Renderer and compute are initialized separately (they need a window handle)
//! and are coordinated via the event bus.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use marauder_event_bus::bus::{self as event_bus, SharedEventBus};
use marauder_event_bus::events::{Event, EventType};
use marauder_config_store::store::{ConfigStore, SharedConfigStore};
use marauder_config_store::watcher::ConfigWatcher;
use marauder_pty::{PaneId, PtyConfig, PtyManager};

use crate::config::RuntimeConfig;
use crate::error::RuntimeError;
use crate::hooks::{self, LifecycleEvent, LifecycleReceiver, SharedLifecycleHooks};
use crate::pipeline::PanePipeline;
use crate::util::{lock_or_recover, read_or_recover, write_or_recover};

/// Runtime state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    /// Not yet initialized.
    Created,
    /// Subsystems initialized and running.
    Running,
    /// Shutdown in progress.
    ShuttingDown,
    /// Fully stopped.
    Stopped,
}

/// The core Marauder runtime. Coordinates all subsystems.
pub struct MarauderRuntime {
    state: RuntimeState,
    config: RuntimeConfig,

    // Subsystems
    event_bus: SharedEventBus,
    config_store: SharedConfigStore,
    config_watcher: Option<ConfigWatcher>,
    pty_manager: Arc<Mutex<PtyManager>>,
    pipelines: HashMap<PaneId, PanePipeline>,
    lifecycle_hooks: SharedLifecycleHooks,
}

impl MarauderRuntime {
    /// Create a new runtime (does not boot — call `boot()` to start).
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            state: RuntimeState::Created,
            config,
            event_bus: event_bus::create_shared(),
            config_store: Arc::new(RwLock::new(ConfigStore::new())),
            config_watcher: None,
            pty_manager: Arc::new(Mutex::new(PtyManager::new())),
            pipelines: HashMap::new(),
            lifecycle_hooks: hooks::create_shared_hooks(),
        }
    }

    /// Boot the runtime: init event bus → config → PTY manager.
    ///
    /// This is the bootstrap sequence. After boot, the runtime is ready
    /// to create panes and process terminal data.
    pub async fn boot(&mut self) -> Result<(), RuntimeError> {
        if self.state != RuntimeState::Created {
            return Err(RuntimeError::AlreadyBooted);
        }

        tracing::info!("Marauder runtime booting...");

        // Step 1: Event bus is already created in new()
        tracing::debug!("Event bus initialized");

        // Step 2: Init config store with event bus
        {
            let mut store = write_or_recover(&self.config_store, "config_store");
            *store = ConfigStore::with_event_bus(self.event_bus.clone());
            store
                .load(
                    self.config.system_config.as_deref(),
                    self.config.user_config.as_deref(),
                    self.config.project_config.as_deref(),
                )
                .map_err(|e| RuntimeError::Config(e.to_string()))?;
        }
        tracing::debug!("Config store loaded");

        // Step 3: Start config watcher if enabled
        if self.config.watch_config {
            let paths = {
                let store = read_or_recover(&self.config_store, "config_store");
                store.watched_paths()
            };
            if !paths.is_empty() {
                match ConfigWatcher::new(self.config_store.clone(), paths) {
                    Ok(watcher) => {
                        self.config_watcher = Some(watcher);
                        tracing::debug!("Config watcher started");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to start config watcher, continuing without");
                    }
                }
            }
        }

        // Step 4: Wire PTY manager with event bus
        {
            let mut mgr = lock_or_recover(&self.pty_manager, "pty_manager");
            *mgr = PtyManager::new().with_event_bus(self.event_bus.clone());
        }
        tracing::debug!("PTY manager initialized");

        // Step 5: Mark as running
        self.state = RuntimeState::Running;
        tracing::info!("Marauder runtime booted");

        // Notify lifecycle hooks
        self.notify_hooks(LifecycleEvent::Booted);

        // Publish session created event
        self.event_bus.publish(
            Event::new(EventType::SessionCreated, "runtime")
                .with_source("runtime"),
        );

        Ok(())
    }

    /// Create a new pane with a PTY session and wire the pipeline.
    pub fn create_pane(&mut self) -> Result<PaneId, RuntimeError> {
        self.ensure_running()?;

        let shell = self.resolve_shell()?;
        let pty_config = PtyConfig {
            shell,
            env: self.config.env.clone(),
            cwd: self.config.cwd.clone(),
            rows: self.config.rows,
            cols: self.config.cols,
        };

        let pane_id;
        let reader;
        {
            let mut mgr = lock_or_recover(&self.pty_manager, "pty_manager");
            pane_id = mgr.create(pty_config).map_err(RuntimeError::pty)?;
            reader = mgr.take_reader(pane_id).map_err(RuntimeError::pty)?;
        }

        // Spawn the pipeline: PTY reader → parser → grid
        let pipeline = PanePipeline::spawn(
            pane_id,
            reader,
            self.config.rows,
            self.config.cols,
            self.event_bus.clone(),
        );

        self.pipelines.insert(pane_id, pipeline);

        self.notify_hooks(LifecycleEvent::PaneCreated { pane_id });
        tracing::info!(pane_id, "Pane created with pipeline");

        Ok(pane_id)
    }

    /// Close a pane and its pipeline.
    pub fn close_pane(&mut self, pane_id: PaneId) -> Result<(), RuntimeError> {
        self.ensure_running()?;

        // Remove pipeline (drops reader, parser, grid)
        self.pipelines.remove(&pane_id);

        // Close the PTY session
        {
            let mut mgr = lock_or_recover(&self.pty_manager, "pty_manager");
            mgr.close(pane_id).map_err(RuntimeError::pty)?;
        }

        self.notify_hooks(LifecycleEvent::PaneClosed { pane_id });
        tracing::info!(pane_id, "Pane closed");

        Ok(())
    }

    /// Resize a pane (PTY + grid).
    ///
    /// Validates pipeline existence before resizing PTY to avoid
    /// leaving PTY and grid in inconsistent states.
    pub fn resize_pane(&mut self, pane_id: PaneId, rows: u16, cols: u16) -> Result<(), RuntimeError> {
        self.ensure_running()?;

        if rows == 0 || cols == 0 {
            return Err(RuntimeError::InvalidResize { rows, cols });
        }

        // Check pipeline exists FIRST to avoid resizing PTY without a matching grid
        if !self.pipelines.contains_key(&pane_id) {
            return Err(RuntimeError::PaneNotFound(pane_id));
        }

        // Resize PTY
        {
            let mut mgr = lock_or_recover(&self.pty_manager, "pty_manager");
            mgr.resize(pane_id, rows, cols).map_err(RuntimeError::pty)?;
        }

        // Resize grid in pipeline (guaranteed to exist from check above)
        if let Some(pipeline) = self.pipelines.get(&pane_id) {
            pipeline.resize(rows, cols);
        }

        self.notify_hooks(LifecycleEvent::PaneResized { pane_id, rows, cols });

        Ok(())
    }

    /// Write data to a pane's PTY.
    pub fn write_to_pane(&self, pane_id: PaneId, data: &[u8]) -> Result<usize, RuntimeError> {
        self.ensure_running()?;
        let mut mgr = lock_or_recover(&self.pty_manager, "pty_manager");
        mgr.write(pane_id, data).map_err(RuntimeError::pty)
    }

    /// Shutdown the runtime gracefully.
    pub async fn shutdown(&mut self) -> Result<(), RuntimeError> {
        if self.state == RuntimeState::Stopped || self.state == RuntimeState::Created {
            return Ok(());
        }

        self.state = RuntimeState::ShuttingDown;
        tracing::info!("Marauder runtime shutting down...");

        self.notify_hooks(LifecycleEvent::ShuttingDown);

        // Close all pipelines
        self.pipelines.clear();

        // Close all PTY sessions
        {
            let mut mgr = lock_or_recover(&self.pty_manager, "pty_manager");
            mgr.close_all();
        }

        // Stop config watcher
        if let Some(watcher) = self.config_watcher.take() {
            watcher.shutdown().await;
        }

        // Publish session closed
        self.event_bus.publish(
            Event::new(EventType::SessionClosed, "runtime")
                .with_source("runtime"),
        );

        self.state = RuntimeState::Stopped;
        self.notify_hooks(LifecycleEvent::Shutdown);
        tracing::info!("Marauder runtime stopped");

        Ok(())
    }

    // --- Accessors ---

    /// Get the shared event bus.
    pub fn event_bus(&self) -> &SharedEventBus {
        &self.event_bus
    }

    /// Get the shared config store.
    pub fn config_store(&self) -> &SharedConfigStore {
        &self.config_store
    }

    /// Get the shared PTY manager.
    pub fn pty_manager(&self) -> &Arc<Mutex<PtyManager>> {
        &self.pty_manager
    }

    /// Get a reference to a pane's pipeline.
    pub fn pipeline(&self, pane_id: PaneId) -> Option<&PanePipeline> {
        self.pipelines.get(&pane_id)
    }

    /// Get the current runtime state.
    pub fn state(&self) -> RuntimeState {
        self.state
    }

    /// List all active pane IDs.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.pipelines.keys().copied().collect()
    }

    /// Register a lifecycle hook consumer. Returns a receiver for events.
    pub fn register_lifecycle_hook(&self) -> LifecycleReceiver {
        let mut hooks = lock_or_recover(&self.lifecycle_hooks, "lifecycle_hooks");
        hooks.register()
    }

    /// Get the shared lifecycle hooks handle.
    pub fn lifecycle_hooks(&self) -> &SharedLifecycleHooks {
        &self.lifecycle_hooks
    }

    // --- Internal helpers ---

    fn ensure_running(&self) -> Result<(), RuntimeError> {
        match self.state {
            RuntimeState::Running => Ok(()),
            RuntimeState::ShuttingDown => Err(RuntimeError::ShuttingDown),
            _ => Err(RuntimeError::NotBooted),
        }
    }

    /// Resolve the shell path from config store, with validation.
    ///
    /// Checks that the resolved path:
    /// 1. Is an absolute path (prevents relative path tricks)
    /// 2. Points to an existing file
    /// 3. Is executable
    fn resolve_shell(&self) -> Result<String, RuntimeError> {
        let shell = {
            let store = read_or_recover(&self.config_store, "config_store");
            if let Some(val) = store.get("terminal.shell") {
                if let Some(s) = val.as_str() {
                    if !s.is_empty() {
                        s.to_string()
                    } else {
                        self.config.shell.clone()
                    }
                } else {
                    self.config.shell.clone()
                }
            } else {
                self.config.shell.clone()
            }
        };

        Self::validate_shell_path(&shell)?;
        Ok(shell)
    }

    /// Validate a shell path is safe to execute.
    fn validate_shell_path(shell: &str) -> Result<(), RuntimeError> {
        let path = Path::new(shell);

        // Must be absolute to prevent relative path attacks
        if !path.is_absolute() {
            return Err(RuntimeError::InvalidShell(format!(
                "shell path must be absolute, got: {shell}"
            )));
        }

        // Must exist
        if !path.exists() {
            return Err(RuntimeError::InvalidShell(format!(
                "shell does not exist: {shell}"
            )));
        }

        // Check executable permission (Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(path).map_err(|e| {
                RuntimeError::InvalidShell(format!(
                    "cannot read shell metadata for {shell}: {e}"
                ))
            })?;
            let mode = metadata.permissions().mode();
            if mode & 0o111 == 0 {
                return Err(RuntimeError::InvalidShell(format!(
                    "shell is not executable: {shell}"
                )));
            }
        }

        Ok(())
    }

    fn notify_hooks(&self, event: LifecycleEvent) {
        let mut hooks = lock_or_recover(&self.lifecycle_hooks, "lifecycle_hooks");
        hooks.notify(event);
    }
}

impl Drop for MarauderRuntime {
    fn drop(&mut self) {
        if self.state == RuntimeState::Running || self.state == RuntimeState::ShuttingDown {
            tracing::warn!("Runtime dropped while running — cleaning up synchronously");
            // Synchronous cleanup: close pipelines and PTY sessions
            self.pipelines.clear();
            let mut mgr = lock_or_recover(&self.pty_manager, "pty_manager");
            mgr.close_all();
            // Drop config watcher — its internal watcher handle will be dropped,
            // which stops the file watcher. The async shutdown task won't run,
            // but the debounce loop will exit when the shutdown channel is dropped.
            self.config_watcher.take();
            self.state = RuntimeState::Stopped;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RuntimeConfig {
        RuntimeConfig {
            watch_config: false,
            system_config: None,
            user_config: None,
            project_config: None,
            ..RuntimeConfig::default()
        }
    }

    #[tokio::test]
    async fn test_boot_and_shutdown() {
        let mut rt = MarauderRuntime::new(test_config());
        assert_eq!(rt.state(), RuntimeState::Created);

        rt.boot().await.unwrap();
        assert_eq!(rt.state(), RuntimeState::Running);

        rt.shutdown().await.unwrap();
        assert_eq!(rt.state(), RuntimeState::Stopped);
    }

    #[tokio::test]
    async fn test_double_boot_fails() {
        let mut rt = MarauderRuntime::new(test_config());
        rt.boot().await.unwrap();
        assert!(rt.boot().await.is_err());
        rt.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_pane() {
        let mut rt = MarauderRuntime::new(test_config());
        rt.boot().await.unwrap();

        let pane_id = rt.create_pane().unwrap();
        assert!(pane_id > 0);
        assert_eq!(rt.pane_ids().len(), 1);
        assert!(rt.pipeline(pane_id).is_some());

        rt.close_pane(pane_id).unwrap();
        assert!(rt.pane_ids().is_empty());

        rt.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_pane_before_boot_fails() {
        let mut rt = MarauderRuntime::new(test_config());
        assert!(rt.create_pane().is_err());
    }

    #[tokio::test]
    async fn test_write_to_pane() {
        let mut rt = MarauderRuntime::new(test_config());
        rt.boot().await.unwrap();

        let pane_id = rt.create_pane().unwrap();
        let n = rt.write_to_pane(pane_id, b"echo hello\n").unwrap();
        assert_eq!(n, 11);

        rt.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_write_before_boot_fails() {
        let rt = MarauderRuntime::new(test_config());
        assert!(rt.write_to_pane(1, b"test").is_err());
    }

    #[tokio::test]
    async fn test_resize_pane() {
        let mut rt = MarauderRuntime::new(test_config());
        rt.boot().await.unwrap();

        let pane_id = rt.create_pane().unwrap();
        rt.resize_pane(pane_id, 48, 120).unwrap();

        rt.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_resize_nonexistent_pane_fails() {
        let mut rt = MarauderRuntime::new(test_config());
        rt.boot().await.unwrap();
        assert!(rt.resize_pane(999, 48, 120).is_err());
        rt.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_lifecycle_hooks() {
        let mut rt = MarauderRuntime::new(test_config());
        let mut rx = rt.register_lifecycle_hook();

        rt.boot().await.unwrap();

        // Should receive Booted event
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, LifecycleEvent::Booted));

        let pane_id = rt.create_pane().unwrap();
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, LifecycleEvent::PaneCreated { .. }));

        rt.close_pane(pane_id).unwrap();
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, LifecycleEvent::PaneClosed { .. }));

        rt.shutdown().await.unwrap();
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, LifecycleEvent::ShuttingDown));
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, LifecycleEvent::Shutdown));
    }

    #[tokio::test]
    async fn test_config_store_accessible() {
        let mut rt = MarauderRuntime::new(test_config());
        rt.boot().await.unwrap();

        let store = rt.config_store().read().unwrap();
        // Defaults should be loaded
        assert!(store.get("terminal.scrollback").is_some());

        drop(store);
        rt.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_shutdown_idempotent() {
        let mut rt = MarauderRuntime::new(test_config());
        rt.boot().await.unwrap();
        rt.shutdown().await.unwrap();
        // Second shutdown is a no-op
        rt.shutdown().await.unwrap();
        assert_eq!(rt.state(), RuntimeState::Stopped);
    }

    #[test]
    fn test_validate_shell_path_rejects_relative() {
        let result = MarauderRuntime::validate_shell_path("bash");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("absolute"));
    }

    #[test]
    fn test_validate_shell_path_rejects_nonexistent() {
        let result = MarauderRuntime::validate_shell_path("/usr/bin/nonexistent_shell_12345");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn test_validate_shell_path_accepts_valid() {
        // /bin/sh should exist on any Unix system
        let result = MarauderRuntime::validate_shell_path("/bin/sh");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_drop_cleans_up_running_runtime() {
        let mut rt = MarauderRuntime::new(test_config());
        rt.boot().await.unwrap();
        let _pane_id = rt.create_pane().unwrap();
        // Drop without shutdown — should clean up gracefully
        drop(rt);
        // If we get here without panic, cleanup worked
    }

    #[test]
    fn test_pane_id_zero_is_never_assigned() {
        // PtyManager starts at 1, so pane_id > 0 always.
        // This test documents the invariant relied on by FFI.
        let mgr = PtyManager::new();
        // New manager has no sessions — next create would give id=1
        assert_eq!(mgr.count(), 0);
    }
}
