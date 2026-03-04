use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use marauder_event_bus::EventBus;

use crate::layer::{ConfigError, ConfigLayer, LayerKind, unflatten_to_toml};

/// Shared reference to ConfigStore for cross-thread access.
pub type SharedConfigStore = Arc<RwLock<ConfigStore>>;

/// Layered configuration store with dot-notation key access.
pub struct ConfigStore {
    layers: Vec<ConfigLayer>,
    merged: HashMap<String, Value>,
    event_bus: Option<Arc<EventBus>>,
    /// Paths that were loaded (for reload).
    system_path: Option<PathBuf>,
    user_path: Option<PathBuf>,
    project_path: Option<PathBuf>,
}

impl ConfigStore {
    /// Create a new ConfigStore with defaults.
    pub fn new() -> Self {
        let defaults = ConfigLayer::from_defaults();
        let merged = defaults.values.clone();
        Self {
            layers: vec![defaults],
            merged,
            event_bus: None,
            system_path: None,
            user_path: None,
            project_path: None,
        }
    }

    /// Create with an event bus for change notifications.
    pub fn with_event_bus(event_bus: Arc<EventBus>) -> Self {
        let mut store = Self::new();
        store.event_bus = Some(event_bus);
        store
    }

    /// Load config from file system layers.
    pub fn load(
        &mut self,
        system: Option<&Path>,
        user: Option<&Path>,
        project: Option<&Path>,
    ) -> Result<(), ConfigError> {
        self.system_path = system.map(|p| p.to_path_buf());
        self.user_path = user.map(|p| p.to_path_buf());
        self.project_path = project.map(|p| p.to_path_buf());

        // Remove all file-based layers, keep defaults + extension + cli
        self.layers.retain(|l| matches!(l.kind, LayerKind::Default | LayerKind::Extension | LayerKind::Cli));

        // Load file layers
        let file_layers = [
            (system, LayerKind::System),
            (user, LayerKind::User),
            (project, LayerKind::Project),
        ];

        for (path_opt, kind) in &file_layers {
            if let Some(path) = path_opt {
                if let Some(layer) = ConfigLayer::from_toml_file(path, *kind)? {
                    self.layers.push(layer);
                }
            }
        }

        self.merge_layers();
        Ok(())
    }

    /// Reload config files from disk.
    pub fn reload(&mut self) -> Result<(), ConfigError> {
        let old_merged = self.merged.clone();
        let system = self.system_path.clone();
        let user = self.user_path.clone();
        let project = self.project_path.clone();
        self.load(
            system.as_deref(),
            user.as_deref(),
            project.as_deref(),
        )?;
        let changed = Self::diff_keys(&old_merged, &self.merged);
        self.publish_change_event(&changed);
        Ok(())
    }

    /// Get a value by dot-notation key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.merged.get(key)
    }

    /// Get a typed value by key.
    pub fn get_typed<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        let value = self.merged.get(key).ok_or_else(|| ConfigError::KeyNotFound(key.to_string()))?;
        serde_json::from_value(value.clone()).map_err(|e| ConfigError::DeserializeError {
            key: key.to_string(),
            source: e,
        })
    }

    /// Set a value in the CLI override layer (highest priority).
    pub fn set(&mut self, key: &str, value: Value) {
        // Find or create CLI layer
        let cli_layer = self
            .layers
            .iter_mut()
            .find(|l| l.kind == LayerKind::Cli);
        match cli_layer {
            Some(layer) => {
                layer.values.insert(key.to_string(), value);
            }
            None => {
                let mut layer = ConfigLayer::new(LayerKind::Cli);
                layer.values.insert(key.to_string(), value);
                self.layers.push(layer);
            }
        }
        let old_merged = std::mem::take(&mut self.merged);
        self.merge_layers();
        let changed = Self::diff_keys(&old_merged, &self.merged);
        self.publish_change_event(&changed);
    }

    /// Save the user layer to a TOML file.
    pub fn save_user_config(&self, path: &Path) -> Result<(), ConfigError> {
        let user_layer = self.layers.iter().find(|l| l.kind == LayerKind::User);
        let values = match user_layer {
            Some(layer) => &layer.values,
            None => return Ok(()), // Nothing to save
        };

        let toml_value = unflatten_to_toml(values);
        let content = toml::to_string_pretty(&toml_value)?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Rebuild the merged map from all layers in priority order.
    fn merge_layers(&mut self) {
        // Sort layers by kind priority
        self.layers.sort_by_key(|l| l.kind);
        self.merged.clear();
        for layer in &self.layers {
            for (key, value) in &layer.values {
                self.merged.insert(key.clone(), value.clone());
            }
        }
    }

    /// Compute which keys differ between two merged maps.
    fn diff_keys(old: &HashMap<String, Value>, new: &HashMap<String, Value>) -> Vec<String> {
        let mut changed = Vec::new();
        for (key, new_val) in new {
            match old.get(key) {
                Some(old_val) if old_val == new_val => {}
                _ => changed.push(key.clone()),
            }
        }
        // Keys removed
        for key in old.keys() {
            if !new.contains_key(key) {
                changed.push(key.clone());
            }
        }
        changed
    }

    /// Publish a ConfigChanged event if an event bus is available.
    fn publish_change_event(&self, changed_keys: &[String]) {
        if let Some(ref bus) = self.event_bus {
            use marauder_event_bus::{Event, EventType};
            let event = Event::new(
                EventType::ConfigChanged,
                serde_json::json!({ "changed_keys": changed_keys }),
            );
            bus.publish(event);
        }
    }

    /// Get all paths being watched.
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(ref p) = self.system_path { paths.push(p.clone()); }
        if let Some(ref p) = self.user_path { paths.push(p.clone()); }
        if let Some(ref p) = self.project_path { paths.push(p.clone()); }
        paths
    }

    /// Get all merged keys.
    pub fn keys(&self) -> Vec<&str> {
        self.merged.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_defaults_populated() {
        let store = ConfigStore::new();
        assert!(store.get("terminal.scrollback").is_some());
        assert!(store.get("font.family").is_some());
        assert!(store.get("cursor.style").is_some());
        assert!(store.get("window.opacity").is_some());
        assert_eq!(
            store.get("font.family").unwrap(),
            &Value::String("monospace".into())
        );
    }

    #[test]
    fn test_get_typed() {
        let store = ConfigStore::new();
        let scrollback: u32 = store.get_typed("terminal.scrollback").unwrap();
        assert_eq!(scrollback, 10000);

        let blink: bool = store.get_typed("cursor.blink").unwrap();
        assert!(blink);
    }

    #[test]
    fn test_set_cli_override() {
        let mut store = ConfigStore::new();
        assert_eq!(
            store.get_typed::<u32>("terminal.scrollback").unwrap(),
            10000
        );

        store.set("terminal.scrollback", Value::Number(5000.into()));
        assert_eq!(
            store.get_typed::<u32>("terminal.scrollback").unwrap(),
            5000
        );
    }

    #[test]
    fn test_layer_override() {
        let dir = TempDir::new().unwrap();
        let user_config = dir.path().join("config.toml");
        {
            let mut f = std::fs::File::create(&user_config).unwrap();
            writeln!(f, "[font]\nsize = 18\n\n[terminal]\nscrollback = 20000").unwrap();
        }

        let mut store = ConfigStore::new();
        store.load(None, Some(&user_config), None).unwrap();

        // User layer overrides default
        assert_eq!(store.get_typed::<u32>("font.size").unwrap(), 18);
        assert_eq!(store.get_typed::<u32>("terminal.scrollback").unwrap(), 20000);

        // Default still present for non-overridden keys
        assert_eq!(
            store.get("cursor.style").unwrap(),
            &Value::String("block".into())
        );

        // CLI override beats user
        store.set("font.size", Value::Number(22.into()));
        assert_eq!(store.get_typed::<u32>("font.size").unwrap(), 22);
    }

    #[test]
    fn test_flatten_nested_toml() {
        use crate::layer::flatten_toml;

        let toml_str = r#"
[terminal]
shell = "/bin/zsh"
scrollback = 5000

[font]
family = "Fira Code"
size = 16

[font.ligatures]
enabled = true
"#;
        let table: toml::Value = toml::from_str(toml_str).unwrap();
        let mut out = HashMap::new();
        flatten_toml(&table, "", &mut out);

        assert_eq!(out["terminal.shell"], Value::String("/bin/zsh".into()));
        assert_eq!(out["terminal.scrollback"], Value::Number(5000.into()));
        assert_eq!(out["font.family"], Value::String("Fira Code".into()));
        assert_eq!(out["font.size"], Value::Number(16.into()));
        assert_eq!(out["font.ligatures.enabled"], Value::Bool(true));
    }

    #[test]
    fn test_key_not_found() {
        let store = ConfigStore::new();
        assert!(store.get("nonexistent.key").is_none());
        assert!(store.get_typed::<String>("nonexistent.key").is_err());
    }

    #[test]
    fn test_save_user_config() {
        let dir = TempDir::new().unwrap();
        let user_config = dir.path().join("config.toml");
        {
            let mut f = std::fs::File::create(&user_config).unwrap();
            writeln!(f, "[font]\nsize = 20").unwrap();
        }

        let mut store = ConfigStore::new();
        store.load(None, Some(&user_config), None).unwrap();

        let save_path = dir.path().join("saved.toml");
        store.save_user_config(&save_path).unwrap();

        let content = std::fs::read_to_string(&save_path).unwrap();
        assert!(content.contains("size = 20"));
    }
}
