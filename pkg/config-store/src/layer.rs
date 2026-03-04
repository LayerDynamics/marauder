use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("failed to parse TOML: {0}")]
    TomlError(#[from] toml::de::Error),
    #[error("failed to serialize TOML: {0}")]
    TomlSerError(#[from] toml::ser::Error),
    #[error("failed to deserialize value for key '{key}': {source}")]
    DeserializeError {
        key: String,
        source: serde_json::Error,
    },
    #[error("key not found: {0}")]
    KeyNotFound(String),
    #[error("watcher error: {0}")]
    WatcherError(String),
}

/// Priority order for config layers (higher ordinal = higher priority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum LayerKind {
    Default = 0,
    System = 1,
    User = 2,
    Project = 3,
    Extension = 4,
    Cli = 5,
}

/// A single config layer with its values and optional source path.
#[derive(Debug, Clone)]
pub struct ConfigLayer {
    pub kind: LayerKind,
    pub values: HashMap<String, Value>,
    pub path: Option<PathBuf>,
}

impl ConfigLayer {
    /// Create an empty layer.
    pub fn new(kind: LayerKind) -> Self {
        Self {
            kind,
            values: HashMap::new(),
            path: None,
        }
    }

    /// Load a layer from a TOML file. Returns Ok(None) if the file doesn't exist.
    pub fn from_toml_file(path: &std::path::Path, kind: LayerKind) -> Result<Option<Self>, ConfigError> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(ConfigError::IoError(e)),
        };
        let table: toml::Value = toml::from_str(&content)?;
        let mut values = HashMap::new();
        flatten_toml(&table, "", &mut values);
        Ok(Some(Self {
            kind,
            values,
            path: Some(path.to_path_buf()),
        }))
    }
}

/// Recursively flatten a TOML value into dot-notation keys with serde_json Values.
pub fn flatten_toml(value: &toml::Value, prefix: &str, out: &mut HashMap<String, Value>) {
    match value {
        toml::Value::Table(table) => {
            for (key, val) in table {
                let full_key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_toml(val, &full_key, out);
            }
        }
        toml::Value::Array(arr) => {
            let json_arr: Vec<Value> = arr.iter().map(toml_to_json).collect();
            out.insert(prefix.to_string(), Value::Array(json_arr));
        }
        other => {
            out.insert(prefix.to_string(), toml_to_json(other));
        }
    }
}

/// Convert a toml::Value to a serde_json::Value.
fn toml_to_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(s) if s == "__null__" => Value::Null,
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => Value::Number((*i).into()),
        toml::Value::Float(f) => {
            serde_json::Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(arr) => Value::Array(arr.iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => {
            let map: serde_json::Map<String, Value> = table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect();
            Value::Object(map)
        }
    }
}

/// Unflatten dot-notation keys back into nested toml::Value::Table for serialization.
pub fn unflatten_to_toml(values: &HashMap<String, Value>) -> toml::Value {
    let mut root = toml::map::Map::new();
    for (key, val) in values {
        let parts: Vec<&str> = key.split('.').collect();
        insert_nested(&mut root, &parts, json_to_toml(val));
    }
    toml::Value::Table(root)
}

fn insert_nested(table: &mut toml::map::Map<String, toml::Value>, parts: &[&str], value: toml::Value) {
    if parts.len() == 1 {
        table.insert(parts[0].to_string(), value);
        return;
    }
    let entry = table
        .entry(parts[0])
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    // If the existing entry is not a table (leaf collides with nested key), promote it to a table
    if !entry.is_table() {
        tracing::warn!(
            "config key collision: '{}' is a leaf value but also has nested children; promoting to table",
            parts[0]
        );
        *entry = toml::Value::Table(toml::map::Map::new());
    }
    if let toml::Value::Table(ref mut sub) = entry {
        insert_nested(sub, &parts[1..], value);
    }
}

fn json_to_toml(value: &Value) -> toml::Value {
    match value {
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                toml::Value::String(n.to_string())
            }
        }
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Array(arr) => toml::Value::Array(arr.iter().map(json_to_toml).collect()),
        Value::Object(map) => {
            let mut table = toml::map::Map::new();
            for (k, v) in map {
                table.insert(k.clone(), json_to_toml(v));
            }
            toml::Value::Table(table)
        }
        // TOML has no null type; use a sentinel string to preserve round-trip semantics.
        Value::Null => toml::Value::String("__null__".to_string()),
    }
}
