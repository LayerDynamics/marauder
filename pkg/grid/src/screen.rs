use crate::cell::Cell;
use crate::scrollback::TieredScrollback;

pub type Row = Vec<Cell>;

pub struct Screen {
    pub rows: Vec<Row>,
    pub cols: usize,
    /// Tiered scrollback buffer (hot/warm/cold).
    scrollback: TieredScrollback,
}

impl Screen {
    pub fn new(rows: usize, cols: usize) -> Self {
        let grid_rows = (0..rows)
            .map(|_| vec![Cell::default(); cols])
            .collect();
        Self {
            rows: grid_rows,
            cols,
            scrollback: TieredScrollback::new(cols),
        }
    }

    /// Set the maximum hot-tier capacity of the scrollback.
    pub fn set_scrollback_capacity(&mut self, capacity: usize) {
        self.scrollback.set_hot_capacity(capacity);
    }

    /// Number of rows currently in scrollback (across all tiers).
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.total_rows()
    }

    /// Get a scrollback row by absolute index (0 = oldest).
    /// Returns an owned `Row`; warm/cold tiers deserialize on demand.
    pub fn scrollback_row(&self, idx: usize) -> Option<Row> {
        self.scrollback.get_row(idx)
    }

    /// Scroll the screen up by one line within the given region [top, bottom).
    /// The top row is pushed to scrollback (only if region starts at row 0).
    /// A new blank row is inserted at bottom - 1.
    pub fn scroll_up(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom > self.rows.len() {
            return;
        }
        // If scrolling the entire screen (or from top), save to scrollback.
        if top == 0 {
            let row = self.rows[0].clone();
            self.scrollback.push_row(row);
        }
        // Shift rows up within the region.
        for i in top..bottom - 1 {
            self.rows.swap(i, i + 1);
        }
        // Clear the bottom row.
        self.rows[bottom - 1] = vec![Cell::default(); self.cols];
    }

    /// Scroll the screen down by one line within the given region [top, bottom).
    /// A new blank row is inserted at top.
    pub fn scroll_down(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom > self.rows.len() {
            return;
        }
        for i in (top + 1..bottom).rev() {
            self.rows.swap(i, i - 1);
        }
        self.rows[top] = vec![Cell::default(); self.cols];
    }

    /// Resize the screen to new dimensions.
    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        // Adjust column widths.
        if new_cols != self.cols {
            for row in &mut self.rows {
                row.resize(new_cols, Cell::default());
            }
            self.cols = new_cols;
            self.scrollback.set_cols(new_cols);
        }
        // Adjust row count.
        if new_rows > self.rows.len() {
            // Add rows at the bottom.
            for _ in 0..(new_rows - self.rows.len()) {
                self.rows.push(vec![Cell::default(); self.cols]);
            }
        } else if new_rows < self.rows.len() {
            // Remove rows from the top, pushing them to scrollback.
            let excess = self.rows.len() - new_rows;
            for row in self.rows.drain(..excess) {
                self.scrollback.push_row(row);
            }
        }
    }
}
