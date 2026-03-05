use std::fmt;

use unicode_width::UnicodeWidthChar;

use crate::cell::{Cell, CellAttributes, Color};
use crate::screen::Screen;
use marauder_parser::actions::*;

/// Saved cursor state for DECSC/DECRC.
#[derive(Debug, Clone)]
pub struct SavedCursor {
    pub row: usize,
    pub col: usize,
    pub attrs: CellAttributes,
    pub fg: Color,
    pub bg: Color,
}

#[derive(Debug, Clone)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub attrs: CellAttributes,
    pub fg: Color,
    pub bg: Color,
    pub saved: Option<SavedCursor>,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            attrs: CellAttributes::empty(),
            fg: Color::Default,
            bg: Color::Default,
            saved: None,
        }
    }
}

/// Selection state for text copy.
#[derive(Debug, Clone)]
pub struct Selection {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

pub struct Grid {
    pub primary: Screen,
    pub alternate: Screen,
    pub cursor: Cursor,
    pub using_alternate: bool,
    dirty_rows: Vec<bool>,
    /// Scroll region: (top, bottom) where bottom is exclusive. 0-indexed.
    scroll_top: usize,
    scroll_bottom: usize,
    /// Current selection, if any.
    selection: Option<Selection>,
    /// Viewport scroll offset (0 = at bottom / live, >0 = scrolled up into history).
    viewport_offset: usize,
}

impl fmt::Debug for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let screen = self.active_screen();
        f.debug_struct("Grid")
            .field("rows", &screen.rows.len())
            .field("cols", &screen.cols)
            .field("cursor", &self.cursor)
            .field("using_alternate", &self.using_alternate)
            .field("scroll_region", &(self.scroll_top, self.scroll_bottom))
            .field("selection", &self.selection)
            .field("viewport_offset", &self.viewport_offset)
            .finish()
    }
}

impl Grid {
    #[must_use]
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            primary: Screen::new(rows, cols),
            alternate: Screen::new(rows, cols),
            cursor: Cursor::default(),
            using_alternate: false,
            dirty_rows: vec![true; rows],
            scroll_top: 0,
            scroll_bottom: rows,
            selection: None,
            viewport_offset: 0,
        }
    }

    pub fn rows(&self) -> usize {
        self.active_screen().rows.len()
    }

    pub fn cols(&self) -> usize {
        self.active_screen().cols
    }

    pub fn active_screen(&self) -> &Screen {
        if self.using_alternate { &self.alternate } else { &self.primary }
    }

    pub fn active_screen_mut(&mut self) -> &mut Screen {
        if self.using_alternate { &mut self.alternate } else { &mut self.primary }
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

    fn mark_all_dirty(&mut self) {
        self.dirty_rows.fill(true);
    }

    /// Resize the grid to new dimensions.
    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        self.primary.resize(new_rows, new_cols);
        self.alternate.resize(new_rows, new_cols);
        self.dirty_rows.resize(new_rows, true);
        self.mark_all_dirty();
        self.scroll_bottom = new_rows;
        // Clamp cursor
        if self.cursor.row >= new_rows {
            self.cursor.row = new_rows.saturating_sub(1);
        }
        if self.cursor.col >= new_cols {
            self.cursor.col = new_cols.saturating_sub(1);
        }
    }

    /// Set text selection.
    pub fn set_selection(&mut self, start_row: usize, start_col: usize, end_row: usize, end_col: usize) {
        self.selection = Some(Selection { start_row, start_col, end_row, end_col });
        // Mark selection rows dirty for rendering
        let min = start_row.min(end_row);
        let max = start_row.max(end_row);
        for r in min..=max {
            self.mark_dirty(r);
        }
    }

    /// Clear selection.
    pub fn clear_selection(&mut self) {
        if let Some(sel) = self.selection.take() {
            let min = sel.start_row.min(sel.end_row);
            let max = sel.start_row.max(sel.end_row);
            for r in min..=max {
                self.mark_dirty(r);
            }
        }
    }

    /// Get the current selection.
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    /// Extract selected text as a string.
    // TODO: When viewport_offset > 0, selection coordinates may refer to
    // scrollback rows. Currently only the active screen is searched.
    pub fn get_selection_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;
        let screen = self.active_screen();
        let mut text = String::new();

        let (sr, sc, er, ec) = if (sel.start_row, sel.start_col) <= (sel.end_row, sel.end_col) {
            (sel.start_row, sel.start_col, sel.end_row, sel.end_col)
        } else {
            (sel.end_row, sel.end_col, sel.start_row, sel.start_col)
        };

        for row in sr..=er {
            if row >= screen.rows.len() { break; }
            let row_data = &screen.rows[row];
            let col_start = if row == sr { sc } else { 0 };
            let col_end = if row == er { ec.min(row_data.len()) } else { row_data.len() };
            for col in col_start..col_end {
                text.push(row_data[col].c);
            }
            if row != er {
                text.push('\n');
            }
        }
        // Trim trailing spaces per line
        Some(text.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n"))
    }

    /// Scroll the viewport into scrollback history.
    pub fn scroll_viewport(&mut self, offset: usize) {
        let max = self.active_screen().scrollback_len();
        self.viewport_offset = offset.min(max);
    }

    /// Apply a parsed terminal action to the grid.
    pub fn apply_action(&mut self, action: &TerminalAction) {
        match action {
            TerminalAction::Print(c) => self.action_print(*c),
            TerminalAction::Execute(_) => {} // handled by parser as specific actions
            TerminalAction::Bell => {} // event bus notification, not a grid mutation
            TerminalAction::Linefeed => self.action_linefeed(),
            TerminalAction::CarriageReturn => { self.cursor.col = 0; }
            TerminalAction::Backspace => {
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
            }
            TerminalAction::Tab => {
                // Advance to next tab stop (every 8 columns)
                let next_tab = (self.cursor.col / 8 + 1) * 8;
                self.cursor.col = next_tab.min(self.cols().saturating_sub(1));
            }
            TerminalAction::CursorMove { direction, count } => {
                let n = *count as usize;
                match direction {
                    CursorDirection::Up => {
                        let top = if self.cursor.row >= self.scroll_top && self.cursor.row <= self.scroll_bottom {
                            self.scroll_top
                        } else {
                            0
                        };
                        self.cursor.row = self.cursor.row.saturating_sub(n).max(top);
                    }
                    CursorDirection::Down => {
                        let bottom = if self.cursor.row >= self.scroll_top && self.cursor.row <= self.scroll_bottom {
                            self.scroll_bottom
                        } else {
                            self.rows().saturating_sub(1)
                        };
                        self.cursor.row = (self.cursor.row + n).min(bottom);
                    }
                    CursorDirection::Forward => self.cursor.col = (self.cursor.col + n).min(self.cols().saturating_sub(1)),
                    CursorDirection::Back => self.cursor.col = self.cursor.col.saturating_sub(n),
                }
            }
            TerminalAction::CursorPosition { row, col } => {
                // CUP uses 1-based coordinates
                self.cursor.row = (*row as usize).saturating_sub(1).min(self.rows().saturating_sub(1));
                self.cursor.col = (*col as usize).saturating_sub(1).min(self.cols().saturating_sub(1));
            }
            TerminalAction::CursorCharAbsolute(col) => {
                self.cursor.col = (*col as usize).saturating_sub(1).min(self.cols().saturating_sub(1));
            }
            TerminalAction::CursorLineAbsolute(row) => {
                self.cursor.row = (*row as usize).saturating_sub(1).min(self.rows().saturating_sub(1));
            }
            TerminalAction::CursorNextLine(n) => {
                self.cursor.row = (self.cursor.row + *n as usize).min(self.rows().saturating_sub(1));
                self.cursor.col = 0;
            }
            TerminalAction::CursorPrevLine(n) => {
                self.cursor.row = self.cursor.row.saturating_sub(*n as usize);
                self.cursor.col = 0;
            }
            TerminalAction::SaveCursor => {
                self.cursor.saved = Some(SavedCursor {
                    row: self.cursor.row,
                    col: self.cursor.col,
                    attrs: self.cursor.attrs,
                    fg: self.cursor.fg,
                    bg: self.cursor.bg,
                });
            }
            TerminalAction::RestoreCursor => {
                if let Some(saved) = self.cursor.saved.clone() {
                    self.cursor.row = saved.row;
                    self.cursor.col = saved.col;
                    self.cursor.attrs = saved.attrs;
                    self.cursor.fg = saved.fg;
                    self.cursor.bg = saved.bg;
                }
            }
            TerminalAction::EraseInDisplay(mode) => self.action_erase_display(*mode),
            TerminalAction::EraseInLine(mode) => self.action_erase_line(*mode),
            TerminalAction::EraseCharacters(n) => {
                let cols = self.cols();
                let row = self.cursor.row;
                let col = self.cursor.col;
                let screen = self.active_screen_mut();
                if row < screen.rows.len() {
                    for c in col..(col + *n as usize).min(cols) {
                        screen.rows[row][c] = Cell::default();
                    }
                }
                self.mark_dirty(row);
            }
            TerminalAction::InsertLines(n) => {
                let n = *n as usize;
                let top = self.cursor.row;
                let bottom = self.scroll_bottom;
                for _ in 0..n {
                    self.active_screen_mut().scroll_down(top, bottom);
                }
                for r in top..bottom {
                    self.mark_dirty(r);
                }
            }
            TerminalAction::DeleteLines(n) => {
                let n = *n as usize;
                let top = self.cursor.row;
                let bottom = self.scroll_bottom;
                for _ in 0..n {
                    self.active_screen_mut().scroll_up(top, bottom);
                }
                for r in top..bottom {
                    self.mark_dirty(r);
                }
            }
            TerminalAction::InsertCharacters(n) => {
                let cols = self.cols();
                let row = self.cursor.row;
                let col = self.cursor.col;
                let screen = self.active_screen_mut();
                if row < screen.rows.len() && col < cols {
                    let n = (*n as usize).min(cols - col);
                    // Shift cells right
                    for c in (col + n..cols).rev() {
                        screen.rows[row][c] = screen.rows[row][c - n];
                    }
                    for c in col..col + n {
                        screen.rows[row][c] = Cell::default();
                    }
                }
                self.mark_dirty(row);
            }
            TerminalAction::DeleteCharacters(n) => {
                let cols = self.cols();
                let row = self.cursor.row;
                let col = self.cursor.col;
                let screen = self.active_screen_mut();
                if row < screen.rows.len() && col < cols {
                    let n = (*n as usize).min(cols - col);
                    for c in col..cols - n {
                        screen.rows[row][c] = screen.rows[row][c + n];
                    }
                    for c in cols - n..cols {
                        screen.rows[row][c] = Cell::default();
                    }
                }
                self.mark_dirty(row);
            }
            TerminalAction::ScrollUp(n) => {
                let top = self.scroll_top;
                let bottom = self.scroll_bottom;
                for _ in 0..*n {
                    self.active_screen_mut().scroll_up(top, bottom);
                }
                for r in top..bottom {
                    self.mark_dirty(r);
                }
            }
            TerminalAction::ScrollDown(n) => {
                let top = self.scroll_top;
                let bottom = self.scroll_bottom;
                for _ in 0..*n {
                    self.active_screen_mut().scroll_down(top, bottom);
                }
                for r in top..bottom {
                    self.mark_dirty(r);
                }
            }
            TerminalAction::SetScrollRegion { top, bottom } => {
                let rows = self.rows();
                let t = (*top as usize).saturating_sub(1);
                let b = if *bottom == 0 { rows } else { (*bottom as usize).min(rows) };
                if t < b {
                    self.scroll_top = t;
                    self.scroll_bottom = b;
                }
                self.cursor.row = 0;
                self.cursor.col = 0;
            }
            TerminalAction::SetAttribute(attr) => self.apply_sgr(attr),
            TerminalAction::SetMode(mode) => self.apply_set_mode(mode),
            TerminalAction::ResetMode(mode) => self.apply_reset_mode(mode),
            TerminalAction::Index => self.action_linefeed(),
            TerminalAction::ReverseIndex => {
                let top = self.scroll_top;
                let bottom = self.scroll_bottom;
                if self.cursor.row == top {
                    self.active_screen_mut().scroll_down(top, bottom);
                    for r in top..bottom {
                        self.mark_dirty(r);
                    }
                } else if self.cursor.row > 0 {
                    self.cursor.row -= 1;
                }
            }
            TerminalAction::NextLine => {
                self.action_linefeed();
                self.cursor.col = 0;
            }
            TerminalAction::FullReset => {
                let rows = self.rows();
                let cols = self.cols();
                self.primary = Screen::new(rows, cols);
                self.alternate = Screen::new(rows, cols);
                self.cursor = Cursor::default();
                self.using_alternate = false;
                self.scroll_top = 0;
                self.scroll_bottom = rows;
                self.selection = None;
                self.viewport_offset = 0;
                self.mark_all_dirty();
            }
            TerminalAction::SetTab | TerminalAction::ClearTab(_) |
            TerminalAction::TabForward(_) | TerminalAction::TabBackward(_) |
            TerminalAction::DesignateCharSet { .. } | TerminalAction::SetCursorStyle(_) |
            TerminalAction::OscDispatch { .. } | TerminalAction::DeviceStatusReport(_) |
            TerminalAction::SendDeviceAttributes | TerminalAction::CsiRaw { .. } |
            TerminalAction::EscRaw { .. } => {
                // These are forwarded via event bus or handled by higher layers
            }
        }
    }

    fn action_print(&mut self, c: char) {
        let rows = self.rows();
        let cols = self.cols();
        let char_width = c.width().unwrap_or(1).max(1) as u8;

        // Wide char at last column: wrap BEFORE printing so it doesn't split across lines
        if char_width == 2 && self.cursor.col == cols.saturating_sub(1) {
            // Erase the last column (it becomes a blank space) and wrap
            if self.cursor.row < rows {
                let row = self.cursor.row;
                let screen = self.active_screen_mut();
                screen.rows[row][cols - 1] = Cell::default();
                self.mark_dirty(row);
            }
            self.cursor.col = 0;
            self.action_linefeed();
        }

        // Standard auto-wrap: cursor past the right edge
        if self.cursor.col >= cols {
            self.cursor.col = 0;
            self.action_linefeed();
        }

        let row = self.cursor.row;
        let col = self.cursor.col;
        let fg = self.cursor.fg;
        let bg = self.cursor.bg;
        let attrs = self.cursor.attrs;

        if row < rows {
            // Clear companion cell if overwriting a wide character's left or right half
            self.clear_wide_char_companion(row, col);

            let screen = self.active_screen_mut();
            screen.rows[row][col] = Cell {
                c,
                fg,
                bg,
                attrs,
                hyperlink_id: None,
                width: char_width,
            };

            // Place spacer cell for wide characters
            if char_width == 2 && col + 1 < cols {
                // Clear companion if overwriting the spacer position too
                self.clear_wide_char_companion(row, col + 1);
                let screen = self.active_screen_mut();
                screen.rows[row][col + 1] = Cell {
                    c: ' ',
                    fg,
                    bg,
                    attrs,
                    hyperlink_id: None,
                    width: 0, // spacer/continuation cell
                };
            }

            self.mark_dirty(row);
        }

        self.cursor.col += char_width as usize;
    }

    /// When overwriting a cell that is part of a wide character, clear the companion cell
    /// to prevent rendering corruption (orphaned half of a wide char).
    fn clear_wide_char_companion(&mut self, row: usize, col: usize) {
        let cols = self.cols();
        let screen = self.active_screen_mut();
        if row >= screen.rows.len() || col >= cols {
            return;
        }
        let cell = screen.rows[row][col];
        if cell.width == 2 && col + 1 < cols {
            // Overwriting the left half of a wide char: clear the right (spacer) half
            screen.rows[row][col + 1] = Cell::default();
        } else if cell.width == 0 && col > 0 {
            // Overwriting the right (spacer) half: clear the left (primary) half
            screen.rows[row][col - 1] = Cell::default();
        }
    }

    fn action_linefeed(&mut self) {
        let top = self.scroll_top;
        let bottom = self.scroll_bottom;
        if self.cursor.row + 1 >= bottom {
            self.active_screen_mut().scroll_up(top, bottom);
            for r in top..bottom {
                self.mark_dirty(r);
            }
        } else {
            self.cursor.row += 1;
        }
    }

    fn action_erase_display(&mut self, mode: EraseMode) {
        let rows = self.rows();
        let cols = self.cols();
        let cursor_row = self.cursor.row;
        let cursor_col = self.cursor.col;
        let screen = self.active_screen_mut();
        match mode {
            EraseMode::ToEnd => {
                if cursor_row < rows {
                    for c in cursor_col..cols {
                        screen.rows[cursor_row][c] = Cell::default();
                    }
                    for r in cursor_row + 1..rows {
                        screen.rows[r] = vec![Cell::default(); cols];
                    }
                }
            }
            EraseMode::ToStart => {
                for r in 0..cursor_row {
                    screen.rows[r] = vec![Cell::default(); cols];
                }
                if cursor_row < rows {
                    for c in 0..=cursor_col.min(cols - 1) {
                        screen.rows[cursor_row][c] = Cell::default();
                    }
                }
            }
            EraseMode::All => {
                for r in 0..rows {
                    screen.rows[r] = vec![Cell::default(); cols];
                }
            }
        }
        self.mark_all_dirty();
    }

    fn action_erase_line(&mut self, mode: EraseMode) {
        let cols = self.cols();
        let row = self.cursor.row;
        let col = self.cursor.col;
        let screen = self.active_screen_mut();
        if row >= screen.rows.len() { return; }
        match mode {
            EraseMode::ToEnd => {
                for c in col..cols {
                    screen.rows[row][c] = Cell::default();
                }
            }
            EraseMode::ToStart => {
                for c in 0..=col.min(cols - 1) {
                    screen.rows[row][c] = Cell::default();
                }
            }
            EraseMode::All => {
                screen.rows[row] = vec![Cell::default(); cols];
            }
        }
        self.mark_dirty(row);
    }

    fn apply_sgr(&mut self, attr: &SgrAttribute) {
        match attr {
            SgrAttribute::Reset => {
                self.cursor.attrs = CellAttributes::empty();
                self.cursor.fg = Color::Default;
                self.cursor.bg = Color::Default;
            }
            SgrAttribute::Bold => self.cursor.attrs.insert(CellAttributes::BOLD),
            SgrAttribute::Dim => self.cursor.attrs.insert(CellAttributes::DIM),
            SgrAttribute::Italic => self.cursor.attrs.insert(CellAttributes::ITALIC),
            SgrAttribute::Underline => self.cursor.attrs.insert(CellAttributes::UNDERLINE),
            SgrAttribute::SlowBlink | SgrAttribute::RapidBlink => self.cursor.attrs.insert(CellAttributes::BLINK),
            SgrAttribute::Inverse => self.cursor.attrs.insert(CellAttributes::INVERSE),
            SgrAttribute::Hidden => self.cursor.attrs.insert(CellAttributes::HIDDEN),
            SgrAttribute::Strikethrough => self.cursor.attrs.insert(CellAttributes::STRIKETHROUGH),
            SgrAttribute::NoBold => self.cursor.attrs.remove(CellAttributes::BOLD),
            SgrAttribute::NoDim => self.cursor.attrs.remove(CellAttributes::DIM),
            SgrAttribute::NoItalic => self.cursor.attrs.remove(CellAttributes::ITALIC),
            SgrAttribute::NoUnderline => self.cursor.attrs.remove(CellAttributes::UNDERLINE),
            SgrAttribute::NoBlink => self.cursor.attrs.remove(CellAttributes::BLINK),
            SgrAttribute::NoInverse => self.cursor.attrs.remove(CellAttributes::INVERSE),
            SgrAttribute::NoHidden => self.cursor.attrs.remove(CellAttributes::HIDDEN),
            SgrAttribute::NoStrikethrough => self.cursor.attrs.remove(CellAttributes::STRIKETHROUGH),
            SgrAttribute::ForegroundColor(spec) => self.cursor.fg = color_from_spec(spec),
            SgrAttribute::BackgroundColor(spec) => self.cursor.bg = color_from_spec(spec),
            SgrAttribute::DefaultForeground => self.cursor.fg = Color::Default,
            SgrAttribute::DefaultBackground => self.cursor.bg = Color::Default,
        }
    }

    fn apply_set_mode(&mut self, mode: &TerminalMode) {
        match mode {
            TerminalMode::DecPrivate(1049) => {
                // Switch to alternate screen buffer
                self.using_alternate = true;
                self.mark_all_dirty();
            }
            TerminalMode::DecPrivate(47) | TerminalMode::DecPrivate(1047) => {
                self.using_alternate = true;
                self.mark_all_dirty();
            }
            _ => {} // Other modes handled by higher layers
        }
    }

    fn apply_reset_mode(&mut self, mode: &TerminalMode) {
        match mode {
            TerminalMode::DecPrivate(1049) => {
                // Switch back to primary screen buffer
                self.using_alternate = false;
                self.mark_all_dirty();
            }
            TerminalMode::DecPrivate(47) | TerminalMode::DecPrivate(1047) => {
                self.using_alternate = false;
                self.mark_all_dirty();
            }
            _ => {}
        }
    }
}

fn color_from_spec(spec: &ColorSpec) -> Color {
    match spec {
        ColorSpec::Named(idx) => Color::Named(*idx),
        ColorSpec::Indexed(idx) => Color::Indexed(*idx),
        ColorSpec::Rgb { r, g, b } => Color::Rgb { r: *r, g: *g, b: *b },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_and_cursor() {
        let mut grid = Grid::new(24, 80);
        grid.apply_action(&TerminalAction::Print('H'));
        grid.apply_action(&TerminalAction::Print('i'));
        assert_eq!(grid.active_screen().rows[0][0].c, 'H');
        assert_eq!(grid.active_screen().rows[0][1].c, 'i');
        assert_eq!(grid.cursor.col, 2);
    }

    #[test]
    fn test_linefeed_and_scroll() {
        let mut grid = Grid::new(3, 5);
        grid.cursor.row = 2;
        grid.apply_action(&TerminalAction::Linefeed);
        // Should have scrolled since cursor was at bottom
        assert_eq!(grid.cursor.row, 2);
        assert_eq!(grid.active_screen().scrollback_len(), 1);
    }

    #[test]
    fn test_erase_in_display() {
        let mut grid = Grid::new(3, 5);
        grid.apply_action(&TerminalAction::Print('X'));
        grid.apply_action(&TerminalAction::EraseInDisplay(EraseMode::All));
        assert_eq!(grid.active_screen().rows[0][0].c, ' ');
    }

    #[test]
    fn test_cursor_position() {
        let mut grid = Grid::new(24, 80);
        grid.apply_action(&TerminalAction::CursorPosition { row: 5, col: 10 });
        assert_eq!(grid.cursor.row, 4); // 1-based -> 0-based
        assert_eq!(grid.cursor.col, 9);
    }

    #[test]
    fn test_save_restore_cursor() {
        let mut grid = Grid::new(24, 80);
        grid.cursor.row = 5;
        grid.cursor.col = 10;
        grid.apply_action(&TerminalAction::SaveCursor);
        grid.cursor.row = 0;
        grid.cursor.col = 0;
        grid.apply_action(&TerminalAction::RestoreCursor);
        assert_eq!(grid.cursor.row, 5);
        assert_eq!(grid.cursor.col, 10);
    }

    #[test]
    fn test_resize() {
        let mut grid = Grid::new(24, 80);
        grid.resize(48, 120);
        assert_eq!(grid.rows(), 48);
        assert_eq!(grid.cols(), 120);
    }

    #[test]
    fn test_selection_text() {
        let mut grid = Grid::new(3, 10);
        for c in "Hello".chars() {
            grid.apply_action(&TerminalAction::Print(c));
        }
        grid.set_selection(0, 0, 0, 5);
        let text = grid.get_selection_text().unwrap();
        assert_eq!(text, "Hello");
    }

    #[test]
    fn test_sgr_bold() {
        let mut grid = Grid::new(24, 80);
        grid.apply_action(&TerminalAction::SetAttribute(SgrAttribute::Bold));
        grid.apply_action(&TerminalAction::Print('B'));
        assert!(grid.active_screen().rows[0][0].attrs.contains(CellAttributes::BOLD));
    }

    #[test]
    fn test_alternate_screen() {
        let mut grid = Grid::new(24, 80);
        grid.apply_action(&TerminalAction::Print('P')); // on primary
        grid.apply_action(&TerminalAction::SetMode(TerminalMode::DecPrivate(1049)));
        assert!(grid.using_alternate);
        grid.apply_action(&TerminalAction::ResetMode(TerminalMode::DecPrivate(1049)));
        assert!(!grid.using_alternate);
        assert_eq!(grid.active_screen().rows[0][0].c, 'P');
    }

    #[test]
    fn test_scroll_region() {
        let mut grid = Grid::new(10, 20);
        grid.apply_action(&TerminalAction::SetScrollRegion { top: 3, bottom: 7 });
        assert_eq!(grid.scroll_top, 2); // 1-based -> 0-based
        assert_eq!(grid.scroll_bottom, 7);
    }

    #[test]
    fn test_insert_delete_lines() {
        let mut grid = Grid::new(5, 10);
        for c in "ABCDE".chars() {
            grid.apply_action(&TerminalAction::Print(c));
        }
        grid.cursor.row = 0;
        grid.cursor.col = 0;
        grid.apply_action(&TerminalAction::InsertLines(1));
        // Row 0 should now be blank
        assert_eq!(grid.active_screen().rows[0][0].c, ' ');
    }

    #[test]
    fn test_auto_wrap() {
        let mut grid = Grid::new(3, 5);
        for c in "ABCDEF".chars() {
            grid.apply_action(&TerminalAction::Print(c));
        }
        // F should be on row 1, col 0
        assert_eq!(grid.active_screen().rows[1][0].c, 'F');
    }

    #[test]
    fn test_wide_char_cjk() {
        let mut grid = Grid::new(24, 80);
        // '中' is a CJK character with display width 2
        grid.apply_action(&TerminalAction::Print('中'));
        let screen = grid.active_screen();
        // Primary cell should have the character with width 2
        assert_eq!(screen.rows[0][0].c, '中');
        assert_eq!(screen.rows[0][0].width, 2, "wide char should have width 2");
        // Spacer cell at col 1 should have width 0
        assert_eq!(screen.rows[0][1].width, 0, "spacer cell should have width 0");
        // Cursor should advance by 2
        assert_eq!(grid.cursor.col, 2);
    }

    #[test]
    fn test_wide_char_wrap_at_last_column() {
        // Grid with 5 columns. Place cursor at col 4 (last col) and print wide char.
        // Wide char needs 2 cols, so it should wrap to next line.
        let mut grid = Grid::new(3, 5);
        for c in "ABCD".chars() {
            grid.apply_action(&TerminalAction::Print(c));
        }
        assert_eq!(grid.cursor.col, 4);
        // Now print a wide char — should wrap
        grid.apply_action(&TerminalAction::Print('中'));
        assert_eq!(grid.cursor.row, 1, "wide char should wrap to next row");
        assert_eq!(grid.cursor.col, 2, "cursor should be at col 2 after wide char");
        assert_eq!(grid.active_screen().rows[1][0].c, '中');
        assert_eq!(grid.active_screen().rows[1][0].width, 2);
        assert_eq!(grid.active_screen().rows[1][1].width, 0);
    }

    #[test]
    fn test_overwrite_wide_char_left_half() {
        let mut grid = Grid::new(24, 80);
        // Print wide char at col 0-1
        grid.apply_action(&TerminalAction::Print('中'));
        // Move cursor back to col 0 and overwrite left half
        grid.cursor.col = 0;
        grid.apply_action(&TerminalAction::Print('A'));
        let screen = grid.active_screen();
        assert_eq!(screen.rows[0][0].c, 'A');
        assert_eq!(screen.rows[0][0].width, 1);
        // Right half spacer should be cleared
        assert_eq!(screen.rows[0][1].c, ' ');
        assert_eq!(screen.rows[0][1].width, 1);
    }

    #[test]
    fn test_overwrite_wide_char_right_half() {
        let mut grid = Grid::new(24, 80);
        // Print wide char at col 0-1
        grid.apply_action(&TerminalAction::Print('中'));
        // Move cursor to col 1 and overwrite right half (spacer)
        grid.cursor.col = 1;
        grid.apply_action(&TerminalAction::Print('B'));
        let screen = grid.active_screen();
        // Left half (primary) should be cleared
        assert_eq!(screen.rows[0][0].c, ' ');
        assert_eq!(screen.rows[0][0].width, 1);
        // Col 1 should now be 'B'
        assert_eq!(screen.rows[0][1].c, 'B');
        assert_eq!(screen.rows[0][1].width, 1);
    }
}
