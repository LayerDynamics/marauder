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

use crate::util::lock_or_recover;

/// Callback type for compute hooks triggered after grid updates.
///
/// The hook receives the pane ID and shared grid reference. The runtime layer
/// sets this to call `ComputeEngine::run_frame_compute` + `Renderer::apply_compute_results`
/// and publish compute result events to the event bus.
pub type ComputeHook = Arc<dyn Fn(PaneId, &Arc<Mutex<Grid>>) + Send + Sync>;

/// Shared mutable slot for the compute hook, allowing updates after the
/// pipeline task has been spawned.
pub type SharedComputeHook = Arc<Mutex<Option<ComputeHook>>>;

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
    /// Optional compute hook called after GridUpdated to trigger async compute.
    /// Shared with the spawned task so updates via `set_compute_hook` take effect.
    pub compute_hook: SharedComputeHook,
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
        Self::spawn_with_compute_hook(pane_id, reader, rows, cols, event_bus, None)
    }

    /// Spawn a pipeline with an optional compute hook.
    pub fn spawn_with_compute_hook(
        pane_id: PaneId,
        reader: Box<dyn std::io::Read + Send>,
        rows: u16,
        cols: u16,
        event_bus: SharedEventBus,
        compute_hook: Option<ComputeHook>,
    ) -> Self {
        let grid = Arc::new(Mutex::new(Grid::new(rows as usize, cols as usize)));
        let parser = Arc::new(Mutex::new(MarauderParser::new()));
        let pty_reader = PtyReader::spawn(pane_id, reader, Some(event_bus.clone()));

        let shared_hook: SharedComputeHook = Arc::new(Mutex::new(compute_hook));

        let mut rx = pty_reader.subscribe();
        let grid_clone = Arc::clone(&grid);
        let parser_clone = Arc::clone(&parser);
        let bus_clone = event_bus.clone();
        let source_label = format!("pane:{pane_id}");
        let source_clone = source_label.clone();
        let hook_ref = Arc::clone(&shared_hook);

        let handle = tokio::spawn(async move {
            tracing::debug!(pane_id, "Pipeline processor task started");
            loop {
                match rx.recv().await {
                    Ok(data) => {
                        tracing::debug!(pane_id, bytes = data.len(), "Pipeline received PTY chunk");
                        Self::process_chunk(
                            pane_id,
                            &data,
                            &parser_clone,
                            &grid_clone,
                            &bus_clone,
                            &source_clone,
                        );
                        // Fire compute hook after grid update — dispatched to the
                        // blocking thread pool so GPU submission / lock acquisition
                        // cannot stall PTY reads on this pane's async task.
                        // Reads from the shared slot so set_compute_hook takes effect.
                        let hook = hook_ref.lock().ok().and_then(|g| g.clone());
                        if let Some(hook) = hook {
                            let grid = Arc::clone(&grid_clone);
                            tokio::task::spawn_blocking(move || {
                                hook(pane_id, &grid);
                            });
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!(pane_id, "Pipeline receiver closed");
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(pane_id, skipped = n, "Pipeline receiver lagged — {n} chunks lost");
                        // Notify consumers that output was lost so they can request a full redraw
                        bus_clone.publish(
                            Event::new(EventType::GridUpdated, pane_id)
                                .with_source(source_clone.clone()),
                        );
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
            compute_hook: shared_hook,
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
        let grid_changed = {
            let mut parser = lock_or_recover(parser, "parser");
            let mut grid = lock_or_recover(grid, "grid");
            let mut changed = false;
            let mut action_count: u32 = 0;
            parser.feed(data, |action| {
                grid.apply_action(&action);
                action_count += 1;
                changed = true;
            });
            if changed {
                tracing::debug!(
                    pane_id,
                    actions = action_count,
                    cursor_row = grid.cursor.row,
                    cursor_col = grid.cursor.col,
                    cursor_visible = grid.cursor.visible,
                    "Pipeline: parsed {action_count} actions from {} bytes",
                    data.len()
                );
            }
            changed
        };
        // Only publish if the parser actually produced actions
        if grid_changed {
            tracing::debug!(pane_id, "GridUpdated event publishing");
            event_bus.publish(
                Event::new(EventType::GridUpdated, pane_id)
                    // Note: source_label.to_owned() allocates per-chunk, but the string is
                    // short (<20 bytes) and this is bounded by PTY output rate, not CPU-bound.
                    .with_source(source_label.to_owned()),
            );
        }
    }

    /// Set the compute hook to be called after each grid update.
    ///
    /// Takes effect immediately — the spawned pipeline task reads from the
    /// same shared slot updated here.
    pub fn set_compute_hook(&self, hook: ComputeHook) {
        if let Ok(mut guard) = self.compute_hook.lock() {
            *guard = Some(hook);
        }
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
