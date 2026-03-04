//! Hot path pipeline: PTY read → VT parse → grid update → (renderer notification).
//!
//! This module wires the data flow from PTY output through the parser into the grid.
//! The renderer is notified via the event bus (GridUpdated) but rendering itself is
//! handled by `pkg/renderer` — the pipeline never blocks on GPU work.

use std::sync::{Arc, Mutex};

use marauder_event_bus::bus::SharedEventBus;
use marauder_event_bus::events::{Event, EventType};
use marauder_grid::Grid;
use marauder_parser::MarauderParser;
use marauder_pty::{PaneId, PtyReader};
use tokio::sync::broadcast;

/// Helper to lock a mutex, logging a warning if it was poisoned.
fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, label: &str) -> std::sync::MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|e| {
        tracing::warn!("{label} mutex was poisoned, recovering");
        e.into_inner()
    })
}

/// A single pane's pipeline: PTY reader → parser → grid.
pub struct PanePipeline {
    /// The pane this pipeline belongs to.
    pub pane_id: PaneId,
    /// The terminal grid state, protected by a mutex for concurrent access.
    pub grid: Arc<Mutex<Grid>>,
    /// The VT parser state, protected by a mutex for concurrent access.
    pub parser: Arc<Mutex<MarauderParser>>,
    /// The PTY reader broadcasting output bytes.
    pub pty_reader: PtyReader,
    event_bus: SharedEventBus,
    /// Cached source string for event bus events (avoids allocation per chunk).
    source_label: String,
    _processor_handle: tokio::task::JoinHandle<()>,
}

impl PanePipeline {
    /// Create and start a pipeline for a pane.
    ///
    /// Takes ownership of the PTY reader stream and spawns an async task that:
    /// 1. Reads output bytes from the PTY (via broadcast channel)
    /// 2. Feeds them through the VT parser
    /// 3. Applies resulting actions to the grid
    /// 4. Publishes GridUpdated events
    pub fn spawn(
        pane_id: PaneId,
        reader: Box<dyn std::io::Read + Send>,
        rows: u16,
        cols: u16,
        event_bus: SharedEventBus,
    ) -> Self {
        let grid = Arc::new(Mutex::new(Grid::new(rows as usize, cols as usize)));
        let parser = Arc::new(Mutex::new(MarauderParser::new()));
        let pty_reader = PtyReader::spawn(pane_id, reader, Some(event_bus.clone()));

        let mut rx = pty_reader.subscribe();
        let grid_clone = Arc::clone(&grid);
        let parser_clone = Arc::clone(&parser);
        let bus_clone = event_bus.clone();
        let source_label = format!("pane:{pane_id}");
        let source_clone = source_label.clone();

        let handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(data) => {
                        Self::process_chunk(
                            pane_id,
                            &data,
                            &parser_clone,
                            &grid_clone,
                            &bus_clone,
                            &source_clone,
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!(pane_id, "Pipeline receiver closed");
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(pane_id, skipped = n, "Pipeline receiver lagged");
                    }
                }
            }
        });

        Self {
            pane_id,
            grid,
            parser,
            pty_reader,
            event_bus,
            source_label,
            _processor_handle: handle,
        }
    }

    /// Process a chunk of PTY output: parse → apply to grid → notify.
    ///
    /// Locks are released before publishing to the event bus to avoid
    /// blocking the pipeline if subscribers do slow work.
    fn process_chunk(
        pane_id: PaneId,
        data: &[u8],
        parser: &Arc<Mutex<MarauderParser>>,
        grid: &Arc<Mutex<Grid>>,
        event_bus: &SharedEventBus,
        source_label: &str,
    ) {
        // Lock parser and grid, apply actions, then release before publish
        {
            let mut parser = lock_or_recover(parser, "parser");
            let mut grid = lock_or_recover(grid, "grid");
            parser.feed(data, |action| {
                grid.apply_action(&action);
            });
        }
        // Locks released — safe to publish without blocking pipeline on subscribers
        event_bus.publish(
            Event::new(EventType::GridUpdated, pane_id)
                .with_source(source_label.to_owned()),
        );
    }

    /// Resize this pane's grid.
    pub fn resize(&self, rows: u16, cols: u16) {
        {
            let mut grid = lock_or_recover(&self.grid, "grid");
            grid.resize(rows as usize, cols as usize);
        }
        // Lock released before publish
        self.event_bus.publish(
            Event::new(
                EventType::GridResized,
                serde_json::json!({ "pane_id": self.pane_id, "rows": rows, "cols": cols }),
            )
            .with_source(self.source_label.clone()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marauder_event_bus::bus;

    #[test]
    fn test_lock_or_recover_normal() {
        let m = Mutex::new(42);
        let guard = lock_or_recover(&m, "test");
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_lock_or_recover_poisoned() {
        let m = Arc::new(Mutex::new(42));
        let m2 = Arc::clone(&m);
        // Poison the mutex by panicking while holding the lock
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut guard = m2.lock().unwrap();
            *guard = 99;
            panic!("intentional");
        }));
        // Should recover the value
        let guard = lock_or_recover(&m, "test");
        assert_eq!(*guard, 99);
    }

    #[tokio::test]
    async fn test_pipeline_spawn_and_resize() {
        let _event_bus = bus::create_shared();
        // We can't easily test the full pipeline without a real PTY, but
        // we can test that resize works on the grid
        let grid = Arc::new(Mutex::new(marauder_grid::Grid::new(24, 80)));
        {
            let mut g = grid.lock().unwrap();
            g.resize(48, 120);
            assert_eq!(g.rows(), 48);
            assert_eq!(g.cols(), 120);
        }
    }
}
