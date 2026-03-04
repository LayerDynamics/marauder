use crate::actions::TerminalAction;

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
    pub fn feed<F: FnMut(TerminalAction)>(&mut self, _bytes: &[u8], _callback: F) {
        // Full implementation in parser card
    }
}

impl Default for MarauderParser {
    fn default() -> Self {
        Self::new()
    }
}
