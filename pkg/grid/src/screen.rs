use crate::cell::Cell;

pub type Row = Vec<Cell>;

pub struct Screen {
    pub rows: Vec<Row>,
    pub cols: usize,
}

impl Screen {
    pub fn new(rows: usize, cols: usize) -> Self {
        let grid_rows = (0..rows)
            .map(|_| vec![Cell::default(); cols])
            .collect();
        Self {
            rows: grid_rows,
            cols,
        }
    }
}
