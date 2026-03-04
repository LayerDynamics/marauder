use serde::{Deserialize, Serialize};

/// All possible terminal actions produced by the VT parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TerminalAction {
    /// Print a character at the current cursor position.
    Print(char),
    /// Execute a C0/C1 control code.
    Execute(u8),
}
