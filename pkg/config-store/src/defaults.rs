use serde_json::Value;
use std::collections::HashMap;

use crate::layer::{ConfigLayer, LayerKind};

impl ConfigLayer {
    /// Create a layer with hardcoded defaults.
    pub fn from_defaults() -> Self {
        let mut values = HashMap::new();

        // Terminal
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        values.insert("terminal.shell".into(), Value::String(shell));
        values.insert("terminal.scrollback".into(), Value::Number(10000.into()));

        // Font
        values.insert("font.family".into(), Value::String("monospace".into()));
        values.insert("font.size".into(), Value::Number(14.into()));
        values.insert(
            "font.line_height".into(),
            serde_json::Number::from_f64(1.2)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );

        // Cursor
        values.insert("cursor.style".into(), Value::String("block".into()));
        values.insert("cursor.blink".into(), Value::Bool(true));

        // Window
        values.insert(
            "window.opacity".into(),
            serde_json::Number::from_f64(1.0)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
        values.insert("window.decorations".into(), Value::Bool(true));

        // Terminal dimensions
        values.insert("terminal.rows".into(), Value::Number(24.into()));
        values.insert("terminal.cols".into(), Value::Number(80.into()));

        Self {
            kind: LayerKind::Default,
            values,
            path: None,
        }
    }
}
