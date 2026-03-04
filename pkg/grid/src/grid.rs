use crate::screen::Screen;

pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

pub struct Grid {
    pub primary: Screen,
    pub alternate: Screen,
    pub cursor: Cursor,
    pub using_alternate: bool,
    dirty_rows: Vec<bool>,
}

impl Grid {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            primary: Screen::new(rows, cols),
            alternate: Screen::new(rows, cols),
            cursor: Cursor { row: 0, col: 0 },
            using_alternate: false,
            dirty_rows: vec![true; rows],
        }
    }

    pub fn get_dirty_rows(&self) -> &[bool] {
        &self.dirty_rows
    }

    pub fn clear_dirty(&mut self) {
        self.dirty_rows.fill(false);
    }

    pub fn mark_dirty(&mut self, row: usize) {
        if row < self.dirty_rows.len() {
            self.dirty_rows[row] = true;
        }
    }
}
