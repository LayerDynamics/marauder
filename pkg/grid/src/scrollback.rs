use std::cell::RefCell;
use std::collections::hash_map::RandomState;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use zeroize::Zeroize;

use crate::screen::Row;

/// LZ4-compressed block of serialized rows for the cold tier.
struct ColdBlock {
    compressed: Vec<u8>,
    row_count: usize,
    /// Column count at the time this block was written (used by scrollback search batching).
    cols: usize,
    /// Whether data is LZ4 compressed.
    is_compressed: bool,
}

impl ColdBlock {
    /// Column count for this block (needed when batching cells for GPU search).
    fn cols(&self) -> usize {
        self.cols
    }
}

impl Drop for ColdBlock {
    fn drop(&mut self) {
        self.compressed.zeroize();
    }
}

/// Warm tier: file-backed storage using JSON-line serialization.
/// Each row occupies one newline-terminated JSON line in the file.
/// The file is wrapped in `RefCell` so reads can seek without `&mut self`.
struct WarmTier {
    /// Interior-mutable file handle; seeks are performed inside RefCell borrows.
    file: RefCell<File>,
    path: PathBuf,
    total_rows: usize,
    cols: usize,
    /// Byte offset of each row in the file, enabling O(1) random access.
    offsets: Vec<u64>,
}

/// Generate a random hex suffix using OS-seeded entropy from `RandomState`.
/// No external crate needed — `RandomState` uses per-process random keys.
fn random_hex_suffix() -> String {
    let mut hasher = RandomState::new().build_hasher();
    // Feed some additional entropy: current time + stack address
    hasher.write_u64(std::time::SystemTime::UNIX_EPOCH
        .elapsed()
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0));
    hasher.write_usize(&hasher as *const _ as usize);
    format!("{:016x}", hasher.finish())
}

impl WarmTier {
    fn create(directory: &Path, cols: usize) -> std::io::Result<Self> {
        // Create directory with restrictive permissions (owner-only on Unix).
        std::fs::create_dir_all(directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700));
        }

        // Use PID + random suffix to prevent path prediction / symlink attacks.
        let filename = format!("scrollback_{}_{}.mscr", std::process::id(), random_hex_suffix());
        let path = directory.join(filename);

        // create_new (O_CREAT|O_EXCL) fails if the path already exists,
        // preventing symlink-following attacks where an attacker creates a
        // symlink at the predicted path before we open it.
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)?;

        // Restrict file permissions to owner-only on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }

        Ok(Self {
            file: RefCell::new(file),
            path,
            total_rows: 0,
            cols,
            offsets: Vec::new(),
        })
    }

    /// Append a row as a JSON line and record its byte offset for O(1) retrieval.
    fn append_row(&mut self, row: &Row) -> std::io::Result<()> {
        let mut f = self.file.borrow_mut();
        let offset = f.seek(SeekFrom::End(0))?;
        self.offsets.push(offset);
        let mut writer = BufWriter::new(&mut *f);
        serde_json::to_writer(&mut writer, row)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        Ok(())
    }

    /// Read a single row by line index (0 = oldest row in warm tier).
    /// Uses the byte offset index for O(1) seek.
    fn get_row(&self, line_idx: usize) -> Option<Row> {
        if line_idx >= self.total_rows || line_idx >= self.offsets.len() {
            return None;
        }
        let offset = self.offsets[line_idx];
        let f = self.file.borrow();
        let mut cloned = f.try_clone().ok()?;
        drop(f);
        cloned.seek(SeekFrom::Start(offset)).ok()?;
        let reader = BufReader::new(cloned);
        let line = reader.lines().next()?.ok()?;
        serde_json::from_str::<Row>(&line).ok()
    }

    /// Return a batched iterator that reads rows from the file in chunks of
    /// `batch_size`, deserializing each batch without loading the entire file
    /// into memory. After iteration completes (or on drop), the file is
    /// truncated and the offset index cleared.
    fn drain_batched(&mut self, batch_size: usize) -> WarmDrainIter {
        let mut f = self.file.borrow_mut();
        let _ = f.seek(SeekFrom::Start(0));
        let reader_file = f.try_clone().ok();
        let truncate_file = f.try_clone().ok();
        let total = self.total_rows;
        self.total_rows = 0;
        self.offsets.clear();
        WarmDrainIter {
            reader: reader_file.map(BufReader::new),
            truncate_file,
            batch_size,
            remaining: total,
        }
    }

    fn remove_file(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Streaming iterator that reads rows from the warm tier file in fixed-size
/// batches, avoiding loading the entire warm tier into memory at once.
struct WarmDrainIter {
    reader: Option<BufReader<File>>,
    /// Separate file handle used to truncate the warm file after all batches
    /// have been read, so the reader isn't invalidated mid-iteration.
    truncate_file: Option<File>,
    batch_size: usize,
    remaining: usize,
}

impl Iterator for WarmDrainIter {
    type Item = Vec<Row>;

    fn next(&mut self) -> Option<Vec<Row>> {
        if self.remaining == 0 {
            return None;
        }
        let reader = self.reader.as_mut()?;
        let to_read = self.batch_size.min(self.remaining);
        let mut batch = Vec::with_capacity(to_read);
        let mut line_buf = String::new();
        for _ in 0..to_read {
            line_buf.clear();
            match reader.read_line(&mut line_buf) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if let Ok(row) = serde_json::from_str::<Row>(line_buf.trim_end()) {
                        batch.push(row);
                    }
                }
                Err(_) => break,
            }
        }
        if batch.is_empty() {
            self.remaining = 0;
            return None;
        }
        self.remaining = self.remaining.saturating_sub(batch.len());
        Some(batch)
    }
}

impl Drop for WarmDrainIter {
    fn drop(&mut self) {
        // Truncate the warm file now that all batches have been consumed (or
        // the iterator was abandoned).
        if let Some(ref mut f) = self.truncate_file {
            let _ = f.set_len(0);
            let _ = f.seek(SeekFrom::Start(0));
        }
    }
}

/// Default cold tier maximum rows (0 = unlimited).
const DEFAULT_COLD_MAX_ROWS: usize = 5_000_000;

/// Cold tier: in-memory LZ4-compressed blocks of rows.
struct ColdTier {
    blocks: VecDeque<ColdBlock>,
    total_rows: usize,
    /// Maximum rows to retain. Oldest blocks are evicted when exceeded. 0 = unlimited.
    max_rows: usize,
}

impl ColdTier {
    fn new() -> Self {
        Self {
            blocks: VecDeque::new(),
            total_rows: 0,
            max_rows: DEFAULT_COLD_MAX_ROWS,
        }
    }

    fn with_max_rows(max_rows: usize) -> Self {
        Self {
            blocks: VecDeque::new(),
            total_rows: 0,
            max_rows,
        }
    }

    /// Compress and store a batch of rows as one block.
    fn push_block(&mut self, rows: &[Row], cols: usize, compress: bool) {
        if rows.is_empty() {
            return;
        }
        let mut lines: Vec<u8> = Vec::new();
        for row in rows {
            let serialized = serde_json::to_vec(row).unwrap_or_default();
            lines.extend_from_slice(&serialized);
            lines.push(b'\n');
        }
        let row_count = rows.len();
        let compressed = if compress {
            let c = lz4_flex::compress_prepend_size(&lines);
            tracing::debug!(
                "ColdTier: pushed block of {} rows ({} cols), {} bytes -> {} bytes compressed",
                row_count, cols, lines.len(), c.len()
            );
            c
        } else {
            tracing::debug!(
                "ColdTier: pushed block of {} rows ({} cols), {} bytes uncompressed",
                row_count, cols, lines.len()
            );
            lines.clone()
        };
        self.total_rows += row_count;
        self.blocks.push_back(ColdBlock {
            compressed,
            row_count,
            cols,
            is_compressed: compress,
        });
        self.evict();
    }

    /// Evict oldest blocks until total_rows is within max_rows (if set).
    fn evict(&mut self) {
        if self.max_rows == 0 {
            return;
        }
        while self.total_rows > self.max_rows {
            if let Some(oldest) = self.blocks.pop_front() {
                let evicted = oldest.row_count;
                self.total_rows = self.total_rows.saturating_sub(evicted);
                tracing::debug!(
                    "ColdTier: evicted oldest block ({} rows), total_rows now {}",
                    evicted,
                    self.total_rows
                );
            } else {
                break;
            }
        }
    }

    /// Get a row by absolute index within the cold tier (0 = oldest).
    fn get_row(&self, abs_idx: usize) -> Option<Row> {
        if abs_idx >= self.total_rows {
            return None;
        }
        let mut cumulative = 0usize;
        for block in &self.blocks {
            let block_end = cumulative + block.row_count;
            if abs_idx < block_end {
                let local_idx = abs_idx - cumulative;
                return self.get_row_from_block(block, local_idx);
            }
            cumulative = block_end;
        }
        None
    }

    fn get_row_from_block(&self, block: &ColdBlock, local_idx: usize) -> Option<Row> {
        tracing::trace!("ColdTier: reading row {} from block with {} cols", local_idx, block.cols());
        let decompressed = if block.is_compressed {
            lz4_flex::decompress_size_prepended(&block.compressed).ok()?
        } else {
            block.compressed.clone()
        };
        let mut lines = decompressed.split(|&b| b == b'\n').filter(|l| !l.is_empty());
        lines.nth(local_idx).and_then(|l| serde_json::from_slice::<Row>(l).ok())
    }

    fn clear(&mut self) {
        self.blocks.clear();
        self.total_rows = 0;
    }
}

impl Drop for ColdTier {
    fn drop(&mut self) {
        // Each ColdBlock's own Drop will zeroize its compressed data.
        self.blocks.clear();
        self.total_rows = 0;
    }
}

/// Number of rows to batch into each cold-tier compressed block.
const COLD_BLOCK_SIZE: usize = 10_000;

/// Default in-memory hot tier capacity (rows).
const DEFAULT_HOT_CAPACITY: usize = 10_000;

/// Default warm tier maximum rows before spilling to cold.
const DEFAULT_WARM_MAX_ROWS: usize = 1_000_000;

/// Three-tiered scrollback buffer.
///
/// - **Hot tier**: recent rows kept in a `VecDeque` in memory.
/// - **Warm tier**: older rows stored in a temp file as JSON lines (only when `disk_enabled`).
/// - **Cold tier**: oldest rows compressed with LZ4 and kept in memory as byte blobs.
///
/// When disk is disabled the warm tier is skipped; rows flow hot → cold directly.
pub struct TieredScrollback {
    /// Hot tier: most recent rows (newest at back).
    hot: VecDeque<Row>,
    hot_capacity: usize,
    /// Warm tier: file-backed storage (optional).
    warm: Option<WarmTier>,
    warm_max_rows: usize,
    /// Cold tier: LZ4-compressed blocks (oldest rows).
    cold: ColdTier,
    /// Column count used for serialization.
    cols: usize,
    /// Whether file-backed warm tier is enabled.
    disk_enabled: bool,
    /// Whether LZ4 compression is used for the cold tier.
    compress_enabled: bool,
    /// Directory for warm tier files.
    directory: PathBuf,
}

impl TieredScrollback {
    /// Create a new `TieredScrollback` with defaults.
    /// Disk backing is disabled by default.
    pub fn new(cols: usize) -> Self {
        Self {
            hot: VecDeque::new(),
            hot_capacity: DEFAULT_HOT_CAPACITY,
            warm: None,
            warm_max_rows: DEFAULT_WARM_MAX_ROWS,
            cold: ColdTier::new(),
            cols,
            disk_enabled: false,
            compress_enabled: true,
            directory: std::env::temp_dir().join("marauder"),
        }
    }

    /// Create with explicit configuration.
    ///
    /// `cold_max_rows`: maximum rows retained in the cold tier (0 = unlimited).
    pub fn with_config(
        cols: usize,
        hot_capacity: usize,
        warm_max: usize,
        disk_enabled: bool,
        compress: bool,
        directory: PathBuf,
    ) -> Self {
        Self::with_full_config(cols, hot_capacity, warm_max, DEFAULT_COLD_MAX_ROWS, disk_enabled, compress, directory)
    }

    /// Create with full configuration including cold tier cap.
    pub fn with_full_config(
        cols: usize,
        hot_capacity: usize,
        warm_max: usize,
        cold_max_rows: usize,
        disk_enabled: bool,
        compress: bool,
        directory: PathBuf,
    ) -> Self {
        Self {
            hot: VecDeque::new(),
            hot_capacity,
            warm: None,
            warm_max_rows: warm_max,
            cold: ColdTier::with_max_rows(cold_max_rows),
            cols,
            disk_enabled,
            compress_enabled: compress,
            directory,
        }
    }

    /// Set the hot tier capacity. Immediately spills excess rows.
    pub fn set_hot_capacity(&mut self, capacity: usize) {
        self.hot_capacity = capacity;
        while self.hot.len() > self.hot_capacity {
            if let Some(row) = self.hot.pop_front() {
                self.spill_to_warm_or_cold(row);
            }
        }
    }

    /// Push a new row (the most recent row) onto the scrollback.
    pub fn push_row(&mut self, row: Row) {
        self.hot.push_back(row);
        if self.hot.len() > self.hot_capacity {
            let oldest = self.hot.pop_front().expect("just pushed");
            self.spill_to_warm_or_cold(oldest);
        }
    }

    /// Get a row by absolute index (0 = oldest across all tiers).
    /// Takes `&self` — warm tier uses interior mutability for file seeks.
    pub fn get_row(&self, abs_idx: usize) -> Option<Row> {
        let total = self.total_rows();
        if abs_idx >= total {
            return None;
        }
        let cold_len = self.cold.total_rows;
        let warm_len = self.warm.as_ref().map_or(0, |w| w.total_rows);
        let hot_len = self.hot.len();

        if abs_idx < cold_len {
            return self.cold.get_row(abs_idx);
        }
        let idx_after_cold = abs_idx - cold_len;
        if idx_after_cold < warm_len {
            return self.warm.as_ref().and_then(|w| w.get_row(idx_after_cold));
        }
        let hot_idx = abs_idx - cold_len - warm_len;
        if hot_idx < hot_len {
            return self.hot.get(hot_idx).cloned();
        }
        None
    }

    /// Total number of rows across all tiers.
    pub fn total_rows(&self) -> usize {
        let cold_len = self.cold.total_rows;
        let warm_len = self.warm.as_ref().map_or(0, |w| w.total_rows);
        cold_len + warm_len + self.hot.len()
    }

    /// Alias for `total_rows`.
    pub fn len(&self) -> usize {
        self.total_rows()
    }

    pub fn is_empty(&self) -> bool {
        self.total_rows() == 0
    }

    /// Update the column count for newly pushed rows.
    /// Does not reformat existing serialized rows.
    pub fn set_cols(&mut self, cols: usize) {
        self.cols = cols;
        if let Some(ref mut warm) = self.warm {
            warm.cols = cols;
        }
    }

    /// Remove the warm tier file and clear all cold blocks.
    pub fn cleanup(&mut self) {
        if let Some(ref warm) = self.warm {
            warm.remove_file();
        }
        self.warm = None;
        self.cold.clear();
        self.hot.clear();
    }

    // --- Private helpers ---

    fn spill_to_warm_or_cold(&mut self, row: Row) {
        if self.disk_enabled {
            self.spill_to_warm(row);
        } else {
            self.spill_to_cold_direct(row);
        }
    }

    fn spill_to_warm(&mut self, row: Row) {
        // Ensure warm tier is initialised.
        if self.warm.is_none() {
            match WarmTier::create(&self.directory, self.cols) {
                Ok(w) => {
                    tracing::debug!("TieredScrollback: warm tier created at {:?}", w.path);
                    self.warm = Some(w);
                }
                Err(e) => {
                    tracing::warn!(
                        "TieredScrollback: failed to create warm tier file: {e}; falling back to cold"
                    );
                    self.spill_to_cold_direct(row);
                    return;
                }
            }
        }
        let warm = self.warm.as_mut().unwrap();
        if let Err(e) = warm.append_row(&row) {
            tracing::warn!(
                "TieredScrollback: warm tier append failed: {e}; falling back to cold"
            );
            self.spill_to_cold_direct(row);
            return;
        }
        // Increment total_rows on the warm tier after successful append.
        self.warm.as_mut().unwrap().total_rows += 1;

        // If warm tier exceeds max, spill entire warm content to cold.
        let warm_len = self.warm.as_ref().map_or(0, |w| w.total_rows);
        if warm_len >= self.warm_max_rows {
            self.flush_warm_to_cold();
        }
    }

    fn flush_warm_to_cold(&mut self) {
        if let Some(ref mut warm) = self.warm {
            let cols = warm.cols;
            let total = warm.total_rows;
            tracing::debug!(
                "TieredScrollback: flushing {} warm rows to cold tier in batches of {}",
                total,
                COLD_BLOCK_SIZE,
            );
            let compress = self.compress_enabled;
            // Stream rows from the warm file in COLD_BLOCK_SIZE batches so we
            // never hold more than one batch in memory at a time.
            for batch in warm.drain_batched(COLD_BLOCK_SIZE) {
                self.cold.push_block(&batch, cols, compress);
            }
        }
    }

    /// Spill a single row directly to the cold tier (no warm file).
    /// Each row becomes its own single-row block for simplicity.
    /// A production optimization would batch these into COLD_BLOCK_SIZE chunks.
    fn spill_to_cold_direct(&mut self, row: Row) {
        self.cold.push_block(&[row], self.cols, self.compress_enabled);
    }
}

impl Drop for TieredScrollback {
    fn drop(&mut self) {
        // Securely clear all tiers. ColdBlock::drop zeroizes compressed data.
        // Hot tier rows are dropped normally (cell data is not secret, but we
        // clear the containers to release memory promptly).
        self.hot.clear();
        // Cold tier: each block's Drop zeroizes its compressed buffer.
        self.cold.clear();
        // Warm tier: remove the backing file from disk.
        if let Some(ref warm) = self.warm {
            warm.remove_file();
        }
        self.warm = None;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::Cell;

    fn make_row(cols: usize, c: char) -> Row {
        vec![
            Cell {
                c,
                ..Cell::default()
            };
            cols
        ]
    }

    #[test]
    fn test_basic_push_and_get() {
        let mut sb = TieredScrollback::new(80);
        sb.push_row(make_row(80, 'a'));
        sb.push_row(make_row(80, 'b'));
        sb.push_row(make_row(80, 'c'));

        assert_eq!(sb.total_rows(), 3);
        let row = sb.get_row(0).expect("row 0");
        assert_eq!(row[0].c, 'a');
        let row = sb.get_row(2).expect("row 2");
        assert_eq!(row[0].c, 'c');
    }

    #[test]
    fn test_hot_spill_to_cold() {
        let mut sb = TieredScrollback::with_config(
            80,
            3,   // hot_capacity
            100, // warm_max
            false,
            true,
            std::env::temp_dir(),
        );

        // Push 5 rows; first 2 should spill to cold.
        for i in 0..5u8 {
            sb.push_row(make_row(80, (b'a' + i) as char));
        }

        assert_eq!(sb.total_rows(), 5);
        // Oldest row (index 0) is in cold tier.
        let row0 = sb.get_row(0).expect("row 0 from cold");
        assert_eq!(row0[0].c, 'a');

        // Newest row (index 4) is in hot tier.
        let row4 = sb.get_row(4).expect("row 4 from hot");
        assert_eq!(row4[0].c, 'e');
    }

    #[test]
    fn test_warm_tier() {
        let dir = std::env::temp_dir().join(format!("marauder_test_{}", std::process::id()));
        let mut sb = TieredScrollback::with_config(
            80,
            2,    // tiny hot
            4,    // tiny warm
            true, // disk enabled
            true,
            dir.clone(),
        );

        for i in 0..8u8 {
            sb.push_row(make_row(80, (b'a' + i) as char));
        }

        assert_eq!(sb.total_rows(), 8);

        // Row 0 should be in cold (warm flushed when it hit 4).
        let row0 = sb.get_row(0).expect("row 0");
        assert_eq!(row0[0].c, 'a');

        // Cleanup removes warm file.
        sb.cleanup();
        // Verify no scrollback files remain in the temp directory.
        let remaining: Vec<_> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("scrollback_"))
            .collect();
        assert!(remaining.is_empty(), "warm tier file should be removed after cleanup");
    }

    #[test]
    fn test_out_of_bounds_returns_none() {
        let mut sb = TieredScrollback::new(80);
        assert!(sb.get_row(0).is_none());
        sb.push_row(make_row(80, 'x'));
        assert!(sb.get_row(1).is_none());
    }

    #[test]
    fn test_set_cols() {
        let mut sb = TieredScrollback::new(80);
        sb.push_row(make_row(80, 'a'));
        sb.set_cols(120);
        assert_eq!(sb.cols, 120);
    }

    #[test]
    fn test_cold_tier_eviction() {
        // Cold max = 5 rows, hot capacity = 2, disk disabled → rows spill hot→cold.
        let mut sb = TieredScrollback::with_full_config(
            80,
            2,    // hot_capacity
            100,  // warm_max (irrelevant, disk disabled)
            5,    // cold_max_rows
            false,
            false, // no compression for simpler debugging
            std::env::temp_dir(),
        );

        // Push 10 rows: hot keeps 2, cold gets 8 but should evict down to 5.
        for i in 0..10u8 {
            sb.push_row(make_row(80, (b'a' + i) as char));
        }

        // Total should be hot(2) + cold(capped at 5) = 7 (3 evicted).
        assert_eq!(sb.hot.len(), 2);
        assert!(sb.cold.total_rows <= 5, "cold tier should be capped at 5, got {}", sb.cold.total_rows);
        assert_eq!(sb.total_rows(), sb.cold.total_rows + 2);

        // Oldest surviving row should NOT be 'a' (it was evicted).
        let oldest = sb.get_row(0).expect("oldest row");
        assert_ne!(oldest[0].c, 'a', "oldest rows should have been evicted");
    }

    #[test]
    fn test_cold_tier_unlimited() {
        // cold_max_rows = 0 means unlimited.
        let mut sb = TieredScrollback::with_full_config(
            80, 2, 100, 0, false, false, std::env::temp_dir(),
        );
        for i in 0..20u8 {
            sb.push_row(make_row(80, (b'a' + (i % 26)) as char));
        }
        // All 18 spilled rows should be retained (no eviction).
        assert_eq!(sb.cold.total_rows, 18);
        assert_eq!(sb.total_rows(), 20);
    }

    #[test]
    fn test_total_rows_across_tiers() {
        let mut sb = TieredScrollback::with_config(
            80,
            5,
            1000,
            false,
            true,
            std::env::temp_dir(),
        );

        for i in 0..20u8 {
            sb.push_row(make_row(80, (b'a' + (i % 26)) as char));
        }

        assert_eq!(sb.total_rows(), 20);
        assert_eq!(sb.len(), 20);
        assert!(!sb.is_empty());
    }
}
