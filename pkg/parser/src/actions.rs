use serde::{Deserialize, Serialize};

/// SGR (Select Graphic Rendition) attribute types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SgrAttribute {
    Reset,
    Bold,
    Dim,
    Italic,
    Underline,
    SlowBlink,
    RapidBlink,
    Inverse,
    Hidden,
    Strikethrough,
    NoBold,
    NoDim,
    NoItalic,
    NoUnderline,
    NoBlink,
    NoInverse,
    NoHidden,
    NoStrikethrough,
    ForegroundColor(ColorSpec),
    BackgroundColor(ColorSpec),
    DefaultForeground,
    DefaultBackground,
}

/// Color specification for SGR attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorSpec {
    /// Named ANSI color index (0-7 normal, 8-15 bright).
    Named(u8),
    /// 256-color palette index.
    Indexed(u8),
    /// True color RGB.
    Rgb { r: u8, g: u8, b: u8 },
}

/// Erase target for ED/EL sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EraseMode {
    /// Erase from cursor to end.
    ToEnd,
    /// Erase from start to cursor.
    ToStart,
    /// Erase entire line/screen.
    All,
}

/// Cursor movement direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorDirection {
    Up,
    Down,
    Forward,
    Back,
}

/// Terminal mode (set/reset via SM/RM and DECSET/DECRST).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalMode {
    /// DEC mode (e.g., ?25 = cursor visible, ?1049 = alt screen).
    DecPrivate(u16),
    /// ANSI mode (e.g., 4 = insert mode).
    Ansi(u16),
}

/// Character set designations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharSet {
    Ascii,
    DecLineDrawing,
    Uk,
}

/// All possible terminal actions produced by the VT parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TerminalAction {
    /// Print a character at the current cursor position.
    Print(char),
    /// Execute a C0/C1 control code (BEL, BS, HT, LF, VT, FF, CR, SO, SI).
    Execute(u8),

    // -- Cursor movement --
    /// Move cursor in a direction by N cells (CUU, CUD, CUF, CUB).
    CursorMove { direction: CursorDirection, count: u32 },
    /// Move cursor to absolute position (CUP/HVP).
    CursorPosition { row: u32, col: u32 },
    /// Move cursor to column N (CHA).
    CursorCharAbsolute(u32),
    /// Move cursor to line N (VPA).
    CursorLineAbsolute(u32),
    /// Move cursor to next line start, N lines down (CNL).
    CursorNextLine(u32),
    /// Move cursor to prev line start, N lines up (CPL).
    CursorPrevLine(u32),
    /// Save cursor position (DECSC / ESC 7).
    SaveCursor,
    /// Restore cursor position (DECRC / ESC 8).
    RestoreCursor,

    // -- Erase --
    /// Erase in display (ED).
    EraseInDisplay(EraseMode),
    /// Erase in line (EL).
    EraseInLine(EraseMode),
    /// Erase N characters at cursor (ECH).
    EraseCharacters(u32),

    // -- Insert/Delete --
    /// Insert N blank lines at cursor row (IL).
    InsertLines(u32),
    /// Delete N lines at cursor row (DL).
    DeleteLines(u32),
    /// Insert N blank characters at cursor (ICH).
    InsertCharacters(u32),
    /// Delete N characters at cursor (DCH).
    DeleteCharacters(u32),

    // -- Scroll --
    /// Scroll up N lines (SU).
    ScrollUp(u32),
    /// Scroll down N lines (SD).
    ScrollDown(u32),
    /// Set scrolling region (DECSTBM).
    SetScrollRegion { top: u32, bottom: u32 },

    // -- Attributes --
    /// Set graphic rendition attribute (SGR).
    SetAttribute(SgrAttribute),

    // -- Modes --
    /// Set terminal mode (SM / DECSET).
    SetMode(TerminalMode),
    /// Reset terminal mode (RM / DECRST).
    ResetMode(TerminalMode),

    // -- Tabs --
    /// Horizontal tab set (HTS).
    SetTab,
    /// Tab clear (TBC).
    ClearTab(u32),
    /// Move forward N tab stops (CHT).
    TabForward(u32),
    /// Move backward N tab stops (CBT).
    TabBackward(u32),

    // -- Character sets --
    /// Designate character set for G0/G1/G2/G3.
    DesignateCharSet { slot: u8, charset: CharSet },

    // -- DEC private --
    /// Reverse index — move cursor up, scrolling if at top (RI / ESC M).
    ReverseIndex,
    /// Index — move cursor down, scrolling if at bottom (IND / ESC D).
    Index,
    /// Next line — move cursor to start of next line (NEL / ESC E).
    NextLine,
    /// Reset terminal to initial state (RIS / ESC c).
    FullReset,
    /// Set cursor style (DECSCUSR).
    SetCursorStyle(u32),

    // -- OSC --
    /// OSC dispatch: operating system command (title, color, hyperlink, etc.).
    OscDispatch { command: u32, data: String },

    // -- Device status --
    /// Device status report (DSR).
    DeviceStatusReport(u32),
    /// Send device attributes (DA).
    SendDeviceAttributes,

    // -- Misc --
    /// Bell (BEL, ^G) — distinct from Execute for event bus notification.
    Bell,
    /// Linefeed / newline — distinct from Execute for explicit handling.
    Linefeed,
    /// Carriage return.
    CarriageReturn,
    /// Backspace.
    Backspace,
    /// Tab.
    Tab,

    // -- Raw fallback --
    /// CSI dispatch not matched to a specific action (raw params preserved).
    CsiRaw { params: Vec<u16>, intermediates: Vec<u8>, action: char },
    /// ESC dispatch not matched to a specific action.
    EscRaw { intermediates: Vec<u8>, action: u8 },
}
