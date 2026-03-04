use crate::actions::*;

/// VT parser wrapping the `vte` crate.
pub struct MarauderParser {
    parser: vte::Parser,
}

impl MarauderParser {
    pub fn new() -> Self {
        Self {
            parser: vte::Parser::new(),
        }
    }

    /// Feed bytes into the parser, invoking the callback for each action.
    pub fn feed<F: FnMut(TerminalAction)>(&mut self, bytes: &[u8], mut callback: F) {
        let mut performer = MarauderPerformer {
            callback: &mut callback,
        };
        self.parser.advance(&mut performer, bytes);
    }
}

impl Default for MarauderParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Implements `vte::Perform` to convert VTE callbacks into `TerminalAction` variants.
struct MarauderPerformer<'a, F: FnMut(TerminalAction)> {
    callback: &'a mut F,
}

impl<'a, F: FnMut(TerminalAction)> MarauderPerformer<'a, F> {
    fn emit(&mut self, action: TerminalAction) {
        (self.callback)(action);
    }
}

impl<'a, F: FnMut(TerminalAction)> vte::Perform for MarauderPerformer<'a, F> {
    fn print(&mut self, c: char) {
        self.emit(TerminalAction::Print(c));
    }

    fn execute(&mut self, byte: u8) {
        // Map well-known C0 codes to specific actions for clearer handling
        match byte {
            0x07 => self.emit(TerminalAction::Bell),
            0x08 => self.emit(TerminalAction::Backspace),
            0x09 => self.emit(TerminalAction::Tab),
            0x0A | 0x0B | 0x0C => self.emit(TerminalAction::Linefeed),
            0x0D => self.emit(TerminalAction::CarriageReturn),
            _ => self.emit(TerminalAction::Execute(byte)),
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        // DCS hook — not commonly needed for basic terminal emulation
    }

    fn put(&mut self, _byte: u8) {
        // DCS put — not commonly needed for basic terminal emulation
    }

    fn unhook(&mut self) {
        // DCS unhook
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        let _ = bell_terminated;
        let command = if !params.is_empty() {
            // First param is typically the OSC command number
            std::str::from_utf8(params[0])
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0)
        } else {
            0
        };

        let data = if params.len() > 1 {
            params[1..]
                .iter()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .collect::<Vec<_>>()
                .join(";")
        } else {
            String::new()
        };

        self.emit(TerminalAction::OscDispatch { command, data });
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, action: char) {
        let raw_params: Vec<u16> = params.iter().flat_map(|sub| sub.iter().copied()).collect();
        let p = |idx: usize, default: u32| -> u32 {
            raw_params.get(idx).copied().map(|v| if v == 0 { default } else { v as u32 }).unwrap_or(default)
        };

        let is_dec_private = intermediates.first() == Some(&b'?');

        match action {
            // Cursor movement
            'A' => self.emit(TerminalAction::CursorMove { direction: CursorDirection::Up, count: p(0, 1) }),
            'B' => self.emit(TerminalAction::CursorMove { direction: CursorDirection::Down, count: p(0, 1) }),
            'C' => self.emit(TerminalAction::CursorMove { direction: CursorDirection::Forward, count: p(0, 1) }),
            'D' => self.emit(TerminalAction::CursorMove { direction: CursorDirection::Back, count: p(0, 1) }),
            'E' => self.emit(TerminalAction::CursorNextLine(p(0, 1))),
            'F' => self.emit(TerminalAction::CursorPrevLine(p(0, 1))),
            'G' => self.emit(TerminalAction::CursorCharAbsolute(p(0, 1))),
            'd' => self.emit(TerminalAction::CursorLineAbsolute(p(0, 1))),
            'H' | 'f' => self.emit(TerminalAction::CursorPosition { row: p(0, 1), col: p(1, 1) }),

            // Erase
            'J' => {
                let mode = match p(0, 0) {
                    1 => EraseMode::ToStart,
                    2 | 3 => EraseMode::All,
                    _ => EraseMode::ToEnd,
                };
                self.emit(TerminalAction::EraseInDisplay(mode));
            }
            'K' => {
                let mode = match p(0, 0) {
                    1 => EraseMode::ToStart,
                    2 => EraseMode::All,
                    _ => EraseMode::ToEnd,
                };
                self.emit(TerminalAction::EraseInLine(mode));
            }
            'X' => self.emit(TerminalAction::EraseCharacters(p(0, 1))),

            // Insert/Delete
            'L' => self.emit(TerminalAction::InsertLines(p(0, 1))),
            'M' => self.emit(TerminalAction::DeleteLines(p(0, 1))),
            '@' => self.emit(TerminalAction::InsertCharacters(p(0, 1))),
            'P' => self.emit(TerminalAction::DeleteCharacters(p(0, 1))),

            // Scroll
            'S' => self.emit(TerminalAction::ScrollUp(p(0, 1))),
            'T' => self.emit(TerminalAction::ScrollDown(p(0, 1))),

            // Scrolling region
            'r' if !is_dec_private => {
                self.emit(TerminalAction::SetScrollRegion { top: p(0, 1), bottom: p(1, 0) });
            }

            // Modes
            'h' => {
                for &param in &raw_params {
                    let mode = if is_dec_private {
                        TerminalMode::DecPrivate(param)
                    } else {
                        TerminalMode::Ansi(param)
                    };
                    self.emit(TerminalAction::SetMode(mode));
                }
            }
            'l' => {
                for &param in &raw_params {
                    let mode = if is_dec_private {
                        TerminalMode::DecPrivate(param)
                    } else {
                        TerminalMode::Ansi(param)
                    };
                    self.emit(TerminalAction::ResetMode(mode));
                }
            }

            // SGR — Select Graphic Rendition
            'm' => self.dispatch_sgr(&raw_params),

            // Tabs
            'I' => self.emit(TerminalAction::TabForward(p(0, 1))),
            'Z' => self.emit(TerminalAction::TabBackward(p(0, 1))),
            'g' => self.emit(TerminalAction::ClearTab(p(0, 0))),

            // Device status
            'n' => self.emit(TerminalAction::DeviceStatusReport(p(0, 0))),
            'c' if intermediates.is_empty() => self.emit(TerminalAction::SendDeviceAttributes),

            // Cursor style (DECSCUSR) — CSI Ps SP q
            'q' if intermediates == [b' '] => self.emit(TerminalAction::SetCursorStyle(p(0, 0))),

            // Unrecognized — preserve raw params for extensions
            _ => self.emit(TerminalAction::CsiRaw {
                params: raw_params,
                intermediates: intermediates.to_vec(),
                action,
            }),
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates, byte) {
            ([], b'7') => self.emit(TerminalAction::SaveCursor),
            ([], b'8') => self.emit(TerminalAction::RestoreCursor),
            ([], b'D') => self.emit(TerminalAction::Index),
            ([], b'E') => self.emit(TerminalAction::NextLine),
            ([], b'M') => self.emit(TerminalAction::ReverseIndex),
            ([], b'H') => self.emit(TerminalAction::SetTab),
            ([], b'c') => self.emit(TerminalAction::FullReset),
            ([b'('], ch) => {
                let charset = match ch {
                    b'0' => CharSet::DecLineDrawing,
                    b'A' => CharSet::Uk,
                    _ => CharSet::Ascii,
                };
                self.emit(TerminalAction::DesignateCharSet { slot: 0, charset });
            }
            ([b')'], ch) => {
                let charset = match ch {
                    b'0' => CharSet::DecLineDrawing,
                    b'A' => CharSet::Uk,
                    _ => CharSet::Ascii,
                };
                self.emit(TerminalAction::DesignateCharSet { slot: 1, charset });
            }
            _ => self.emit(TerminalAction::EscRaw {
                intermediates: intermediates.to_vec(),
                action: byte,
            }),
        }
    }
}

impl<'a, F: FnMut(TerminalAction)> MarauderPerformer<'a, F> {
    /// Parse SGR parameter sequence into individual `SetAttribute` actions.
    fn dispatch_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.emit(TerminalAction::SetAttribute(SgrAttribute::Reset));
            return;
        }

        let mut i = 0;
        while i < params.len() {
            let attr = match params[i] {
                0 => SgrAttribute::Reset,
                1 => SgrAttribute::Bold,
                2 => SgrAttribute::Dim,
                3 => SgrAttribute::Italic,
                4 => SgrAttribute::Underline,
                5 => SgrAttribute::SlowBlink,
                6 => SgrAttribute::RapidBlink,
                7 => SgrAttribute::Inverse,
                8 => SgrAttribute::Hidden,
                9 => SgrAttribute::Strikethrough,
                21 => SgrAttribute::NoBold,
                22 => SgrAttribute::NoDim,
                23 => SgrAttribute::NoItalic,
                24 => SgrAttribute::NoUnderline,
                25 => SgrAttribute::NoBlink,
                27 => SgrAttribute::NoInverse,
                28 => SgrAttribute::NoHidden,
                29 => SgrAttribute::NoStrikethrough,
                // Foreground colors
                30..=37 => SgrAttribute::ForegroundColor(ColorSpec::Named((params[i] - 30) as u8)),
                38 => {
                    if let Some(color) = self.parse_extended_color(params, &mut i) {
                        SgrAttribute::ForegroundColor(color)
                    } else {
                        i += 1;
                        continue;
                    }
                }
                39 => SgrAttribute::DefaultForeground,
                // Background colors
                40..=47 => SgrAttribute::BackgroundColor(ColorSpec::Named((params[i] - 40) as u8)),
                48 => {
                    if let Some(color) = self.parse_extended_color(params, &mut i) {
                        SgrAttribute::BackgroundColor(color)
                    } else {
                        i += 1;
                        continue;
                    }
                }
                49 => SgrAttribute::DefaultBackground,
                // Bright foreground
                90..=97 => SgrAttribute::ForegroundColor(ColorSpec::Named((params[i] - 90 + 8) as u8)),
                // Bright background
                100..=107 => SgrAttribute::BackgroundColor(ColorSpec::Named((params[i] - 100 + 8) as u8)),
                _ => {
                    i += 1;
                    continue;
                }
            };
            self.emit(TerminalAction::SetAttribute(attr));
            i += 1;
        }
    }

    /// Parse extended color (38;5;N or 38;2;R;G;B), advancing `i` past consumed params.
    fn parse_extended_color(&mut self, params: &[u16], i: &mut usize) -> Option<ColorSpec> {
        if *i + 1 >= params.len() {
            return None;
        }
        match params[*i + 1] {
            5 => {
                // 256-color: 38;5;N
                if *i + 2 < params.len() {
                    let idx = params[*i + 2] as u8;
                    *i += 2;
                    Some(ColorSpec::Indexed(idx))
                } else {
                    None
                }
            }
            2 => {
                // True color: 38;2;R;G;B
                if *i + 4 < params.len() {
                    let r = params[*i + 2] as u8;
                    let g = params[*i + 3] as u8;
                    let b = params[*i + 4] as u8;
                    *i += 4;
                    Some(ColorSpec::Rgb { r, g, b })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_and_execute() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        parser.feed(b"AB\x07\x0D", |a| actions.push(a));
        assert!(matches!(actions[0], TerminalAction::Print('A')));
        assert!(matches!(actions[1], TerminalAction::Print('B')));
        assert!(matches!(actions[2], TerminalAction::Bell));
        assert!(matches!(actions[3], TerminalAction::CarriageReturn));
    }

    #[test]
    fn test_cursor_movement() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        // CSI 5 A = cursor up 5
        parser.feed(b"\x1b[5A", |a| actions.push(a));
        match &actions[0] {
            TerminalAction::CursorMove { direction: CursorDirection::Up, count: 5 } => {}
            other => panic!("Expected CursorMove Up 5, got {:?}", other),
        }
    }

    #[test]
    fn test_cursor_position() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        // CSI 10;20H = cursor to row 10, col 20
        parser.feed(b"\x1b[10;20H", |a| actions.push(a));
        match &actions[0] {
            TerminalAction::CursorPosition { row: 10, col: 20 } => {}
            other => panic!("Expected CursorPosition(10,20), got {:?}", other),
        }
    }

    #[test]
    fn test_sgr_colors() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        // CSI 38;2;255;128;0m = true color foreground
        parser.feed(b"\x1b[38;2;255;128;0m", |a| actions.push(a));
        match &actions[0] {
            TerminalAction::SetAttribute(SgrAttribute::ForegroundColor(ColorSpec::Rgb { r: 255, g: 128, b: 0 })) => {}
            other => panic!("Expected ForegroundColor RGB, got {:?}", other),
        }
    }

    #[test]
    fn test_erase_in_display() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        parser.feed(b"\x1b[2J", |a| actions.push(a));
        assert!(matches!(actions[0], TerminalAction::EraseInDisplay(EraseMode::All)));
    }

    #[test]
    fn test_esc_save_restore_cursor() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        parser.feed(b"\x1b7\x1b8", |a| actions.push(a));
        assert!(matches!(actions[0], TerminalAction::SaveCursor));
        assert!(matches!(actions[1], TerminalAction::RestoreCursor));
    }

    #[test]
    fn test_set_mode_dec_private() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        // CSI ?25h = show cursor
        parser.feed(b"\x1b[?25h", |a| actions.push(a));
        match &actions[0] {
            TerminalAction::SetMode(TerminalMode::DecPrivate(25)) => {}
            other => panic!("Expected SetMode DecPrivate(25), got {:?}", other),
        }
    }

    #[test]
    fn test_osc_title() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        // OSC 0;My Title BEL
        parser.feed(b"\x1b]0;My Title\x07", |a| actions.push(a));
        match &actions[0] {
            TerminalAction::OscDispatch { command: 0, data } => {
                assert_eq!(data, "My Title");
            }
            other => panic!("Expected OscDispatch, got {:?}", other),
        }
    }

    #[test]
    fn test_scroll_region() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        parser.feed(b"\x1b[5;20r", |a| actions.push(a));
        match &actions[0] {
            TerminalAction::SetScrollRegion { top: 5, bottom: 20 } => {}
            other => panic!("Expected SetScrollRegion, got {:?}", other),
        }
    }

    #[test]
    fn test_sgr_reset() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        parser.feed(b"\x1b[0m", |a| actions.push(a));
        assert!(matches!(actions[0], TerminalAction::SetAttribute(SgrAttribute::Reset)));
    }

    #[test]
    fn test_insert_delete_lines() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        parser.feed(b"\x1b[3L\x1b[2M", |a| actions.push(a));
        assert!(matches!(actions[0], TerminalAction::InsertLines(3)));
        assert!(matches!(actions[1], TerminalAction::DeleteLines(2)));
    }

    #[test]
    fn test_linefeed_variants() {
        let mut parser = MarauderParser::new();
        let mut actions = Vec::new();
        parser.feed(b"\x0A\x0B\x0C", |a| actions.push(a));
        assert!(matches!(actions[0], TerminalAction::Linefeed));
        assert!(matches!(actions[1], TerminalAction::Linefeed));
        assert!(matches!(actions[2], TerminalAction::Linefeed));
    }
}
