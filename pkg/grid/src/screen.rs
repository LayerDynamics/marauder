use std::collections::VecDeque;
use crate::cell::Cell;

pub type Row = Vec<Cell>;

/// Default scrollback capacity (number of rows).
const DEFAULT_SCROLLBACK_CAPACITY: usize = 10_000;

pub struct Screen {
    pub rows: Vec<Row>,
    pub cols: usize,
    /// Ring buffer of rows that have scrolled off the top.
    scrollback: VecDeque<Row>,
    /// Maximum number of scrollback rows to retain.
    scrollback_capacity: usize,
}

impl Screen {
    pub fn new(rows: usize, cols: usize) -> Self {
        let grid_rows = (0..rows)
            .map(|_| vec![Cell::default(); cols])
            .collect();
        Self {
            rows: grid_rows,
            cols,
            scrollback: VecDeque::new(),
            scrollback_capacity: DEFAULT_SCROLLBACK_CAPACITY,
        }
    }

    /// Set the maximum scrollback capacity.
    pub fn set_scrollback_capacity(&mut self, capacity: usize) {
        self.scrollback_capacity = capacity;
        while self.scrollback.len() > self.scrollback_capacity {
            self.scrollback.pop_front();
        }
    }

    /// Number of rows currently in scrollback.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Get a scrollback row (0 = oldest).
    pub fn scrollback_row(&self, idx: usize) -> Option<&Row> {
        self.scrollback.get(idx)
    }

    /// Scroll the screen up by one line within the given region [top, bottom).
    /// The top row is pushed to scrollback (only if region starts at row 0).
    /// A new blank row is inserted at bottom - 1.
    pub fn scroll_up(&mut self, top: usize, bottom: usize) {
        if top >= bottom || bottom > self.rows.len() {
            return;
        }
        // If scrolling the entire screen (or from top), save to scrollback
        if top == 0 {
            let row = self.rows[0].clone();
            self.scrollback.push_back(row);
            if self.scrollback.len() > self.scrollback_capacity {
                self.scrollback.pop_front();
            }
        }
        // Shift rows up within the region
        for i in top..bottom - 1 {
            self.rows.swap(i, i + 1);
        }
        // Clear the bottom row
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
        // Adjust column widths
        if new_cols != self.cols {
            for row in &mut self.rows {
                row.resize(new_cols, Cell::default());
            }
            self.cols = new_cols;
        }
        // Adjust row count
        if new_rows > self.rows.len() {
            // Add rows at the bottom
            for _ in 0..(new_rows - self.rows.len()) {
                self.rows.push(vec![Cell::default(); self.cols]);
            }
        } else if new_rows < self.rows.len() {
            // Remove rows from the top, pushing them to scrollback
            let excess = self.rows.len() - new_rows;
            for _ in 0..excess {
                let row = self.rows.remove(0);
                self.scrollback.push_back(row);
                if self.scrollback.len() > self.scrollback_capacity {
                    self.scrollback.pop_front();
                }
            }
        }
    }
}
