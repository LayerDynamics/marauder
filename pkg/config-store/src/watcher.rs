use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event as NotifyEvent};
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Sleep;
use tracing::{debug, error, warn};

use crate::layer::ConfigError;
use crate::store::SharedConfigStore;

/// Watches config files for changes and triggers reload.
pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    shutdown_tx: mpsc::Sender<()>,
}

impl ConfigWatcher {
    /// Start watching the given paths. Spawns a tokio task for debounced reload.
    pub fn new(
        store: SharedConfigStore,
        paths: Vec<PathBuf>,
    ) -> Result<Self, ConfigError> {
        let (event_tx, event_rx) = mpsc::channel::<NotifyEvent>(64);
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

        let mut watcher = notify::recommended_watcher(move |res: Result<NotifyEvent, notify::Error>| {
            match res {
                Ok(event) => {
                    let _ = event_tx.blocking_send(event);
                }
                Err(e) => {
                    warn!("file watcher error: {e}");
                }
            }
        })
        .map_err(|e| ConfigError::WatcherError(e.to_string()))?;

        // Watch parent directories of config files so renames (editor save patterns) are caught.
        for path in &paths {
            if let Some(parent) = path.parent() {
                if parent.exists() {
                    watcher
                        .watch(parent, RecursiveMode::NonRecursive)
                        .map_err(|e| ConfigError::WatcherError(e.to_string()))?;
                }
            }
        }

        // Collect just the file names for matching (handles rename-to patterns)
        let watched_filenames: Vec<std::ffi::OsString> = paths
            .iter()
            .filter_map(|p| p.file_name().map(|f| f.to_os_string()))
            .collect();
        let watched_parents: Vec<PathBuf> = paths
            .iter()
            .filter_map(|p| p.parent().map(|d| d.to_path_buf()))
            .collect();

        tokio::spawn(Self::debounce_loop(
            store,
            watched_filenames,
            watched_parents,
            event_rx,
            shutdown_rx,
        ));

        Ok(Self {
            _watcher: watcher,
            shutdown_tx,
        })
    }

    async fn debounce_loop(
        store: SharedConfigStore,
        watched_filenames: Vec<std::ffi::OsString>,
        watched_parents: Vec<PathBuf>,
        mut event_rx: mpsc::Receiver<NotifyEvent>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) {
        let debounce_duration = Duration::from_millis(500);
        // Resettable debounce timer — starts as a far-future sleep (effectively disabled)
        let mut debounce_timer: Pin<Box<Sleep>> = Box::pin(tokio::time::sleep(Duration::from_secs(86400)));
        let mut pending = false;

        loop {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    // Match by file name within watched parent dirs to handle editor rename patterns
                    let relevant = event.paths.iter().any(|p| {
                        if let (Some(name), Some(parent)) = (p.file_name(), p.parent()) {
                            watched_filenames.contains(&name.to_os_string())
                                && watched_parents.iter().any(|wp| wp == parent)
                        } else {
                            false
                        }
                    });
                    if !relevant {
                        continue;
                    }

                    // Reset the debounce timer — this ensures the *last* event in a burst triggers reload
                    debounce_timer.as_mut().reset(tokio::time::Instant::now() + debounce_duration);
                    pending = true;
                }
                _ = &mut debounce_timer, if pending => {
                    pending = false;
                    debug!("config file changed, reloading");
                    match store.write() {
                        Ok(mut s) => {
                            if let Err(e) = s.reload() {
                                error!("failed to reload config: {e}");
                            }
                        }
                        Err(e) => {
                            error!("failed to acquire config store lock: {e}");
                        }
                    }
                    // Reset timer to far future
                    debounce_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(86400));
                }
                _ = shutdown_rx.recv() => {
                    debug!("config watcher shutting down");
                    break;
                }
            }
        }
    }

    /// Signal the watcher to stop.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(()).await;
    }
}
