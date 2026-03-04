//! Async reader thread per PTY session.
//!
//! Spawns a tokio task that reads from the PTY reader in a blocking thread,
//! forwards output bytes through a tokio broadcast channel, and optionally
//! publishes PtyOutput events to the event bus.

use std::io::Read;
use tokio::sync::broadcast;
use marauder_event_bus::bus::SharedEventBus;
use marauder_event_bus::events::{Event, EventType};

use crate::manager::PaneId;

/// Size of the read buffer per PTY reader iteration.
const READ_BUF_SIZE: usize = 4096;

/// Capacity of the broadcast channel (number of messages buffered).
const CHANNEL_CAPACITY: usize = 256;

/// A handle to an async PTY reader. Dropping it signals the reader to stop.
///
/// **Cancellation note:** The reader loop checks the cancel signal between reads,
/// but `reader.read()` may block indefinitely. Reliable shutdown requires closing
/// the PTY (which causes read to return EOF/error) rather than relying solely on
/// the cancel signal. Dropping the `PtyReader` sets the cancel flag, but the
/// blocked read will only unblock when the PTY master is closed or the child exits.
pub struct PtyReader {
    /// Receive PTY output bytes.
    pub rx: broadcast::Receiver<Vec<u8>>,
    /// Shared sender — clone to get additional receivers.
    tx: broadcast::Sender<Vec<u8>>,
    /// Signal the reader thread to stop.
    _cancel: tokio::sync::watch::Sender<bool>,
}

impl PtyReader {
    /// Spawn an async reader for the given PTY reader stream.
    ///
    /// The reader runs on a blocking tokio thread and sends output bytes
    /// through a broadcast channel. If an event bus is provided, PtyOutput
    /// events are published for each read chunk.
    pub fn spawn(
        pane_id: PaneId,
        reader: Box<dyn Read + Send>,
        event_bus: Option<SharedEventBus>,
    ) -> Self {
        let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        let tx_clone = tx.clone();

        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let mut reader = reader;
                let mut buf = vec![0u8; READ_BUF_SIZE];

                loop {
                    // Check cancellation
                    if *cancel_rx.borrow() {
                        break;
                    }

                    match reader.read(&mut buf) {
                        Ok(0) => {
                            // EOF — child exited or PTY closed
                            tracing::debug!(pane_id, "PTY reader got EOF");
                            break;
                        }
                        Ok(n) => {
                            let data = buf[..n].to_vec();

                            // Publish to event bus if available
                            if let Some(ref bus) = event_bus {
                                bus.publish(Event {
                                    event_type: EventType::PtyOutput,
                                    payload: data.clone(),
                                    timestamp_us: Event::now_us(),
                                    source: Some(format!("pane:{pane_id}")),
                                });
                            }

                            // Send through channel — if no receivers, just drop
                            if tx_clone.send(data).is_err() {
                                // All receivers dropped, but keep reading
                                // to avoid blocking the PTY
                            }
                        }
                        Err(e) => {
                            tracing::debug!(pane_id, error = %e, "PTY reader error");
                            break;
                        }
                    }
                }
            })
            .await;

            if let Err(e) = result {
                tracing::error!(pane_id, error = %e, "PTY reader task panicked");
            }
        });

        Self {
            rx,
            tx,
            _cancel: cancel_tx,
        }
    }

    /// Get an additional receiver for the PTY output stream.
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.tx.subscribe()
    }
}

impl Drop for PtyReader {
    fn drop(&mut self) {
        // Send the cancel signal so the reader loop exits on its next iteration.
        // Note: if the reader is blocked on `read()`, this won't unblock it —
        // the PTY master must be closed for the read to return EOF/error.
        let _ = self._cancel.send(true);
    }
}
