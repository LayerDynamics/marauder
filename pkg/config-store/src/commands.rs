//! Tauri command wrappers for config-store operations.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use marauder_event_bus::{read_or_log, write_or_log};
use serde_json::Value;

use crate::store::{ConfigStore, SharedConfigStore};

/// Tauri managed state wrapping the shared config store.
///
/// Uses an inner `RwLock<Option<SharedConfigStore>>` because the real config store
/// is created asynchronously during runtime boot, after Tauri state registration.
#[derive(Clone)]
pub struct TauriConfigStore {
    pub inner: Arc<RwLock<Option<SharedConfigStore>>>,
}

impl TauriConfigStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    /// Inject the real config store after runtime boot.
    pub fn inject(&self, store: SharedConfigStore) {
        *write_or_log(&self.inner, "config::inject") = Some(store);
    }
}

impl Default for TauriConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

fn with_store<F, T>(state: &TauriConfigStore, f: F) -> Result<T, String>
where
    F: FnOnce(&ConfigStore) -> Result<T, String>,
{
    let opt = read_or_log(&state.inner, "config::with_store");
    let store_arc = opt.as_ref().ok_or("Config store not initialized")?;
    let store = read_or_log(store_arc, "config::with_store::inner");
    f(&store)
}

fn with_store_mut<F, T>(state: &TauriConfigStore, f: F) -> Result<T, String>
where
    F: FnOnce(&mut ConfigStore) -> Result<T, String>,
{
    let opt = read_or_log(&state.inner, "config::with_store_mut");
    let store_arc = opt.as_ref().ok_or("Config store not initialized")?;
    let mut store = write_or_log(store_arc, "config::with_store_mut::inner");
    f(&mut store)
}

#[tauri::command]
pub fn config_cmd_get(
    state: tauri::State<'_, TauriConfigStore>,
    key: String,
) -> Result<Option<Value>, String> {
    with_store(&state, |store| Ok(store.get(&key).cloned()))
}

/// Keys the webview is allowed to write.
///
/// Security-sensitive keys (e.g. `terminal.shell`, `terminal.env`) are excluded
/// because the webview runs partially-trusted content (extension UI, themes) and
/// must not be able to control which executable is spawned or inject env vars.
const WEBVIEW_WRITABLE_PREFIXES: &[&str] = &[
    "font.",
    "cursor.",
    "window.",
    "theme.",
    "ui.",
    "terminal.scrollback",
    "terminal.bell",
    "keybindings.",
];

/// Returns `true` if `key` is safe for the webview to write.
fn is_webview_writable(key: &str) -> bool {
    WEBVIEW_WRITABLE_PREFIXES
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

#[tauri::command]
pub fn config_cmd_set(
    state: tauri::State<'_, TauriConfigStore>,
    key: String,
    value: Value,
) -> Result<(), String> {
    if !is_webview_writable(&key) {
        return Err(format!(
            "Config key '{key}' is not writable from the webview"
        ));
    }
    with_store_mut(&state, |store| {
        store.set(&key, value);
        Ok(())
    })
}

#[tauri::command]
pub fn config_cmd_keys(
    state: tauri::State<'_, TauriConfigStore>,
) -> Result<Vec<String>, String> {
    with_store(&state, |store| {
        Ok(store.keys().into_iter().map(|s| s.to_string()).collect())
    })
}

#[tauri::command]
pub fn config_cmd_save(
    state: tauri::State<'_, TauriConfigStore>,
    path: String,
) -> Result<(), String> {
    let requested = PathBuf::from(&path);

    // Reject paths without a filename component (bare directories, empty, etc.)
    let file_name = requested
        .file_name()
        .filter(|f| !f.is_empty())
        .ok_or("Config save path must include a filename")?;

    // Resolve to absolute path to prevent traversal via ../ or symlinks
    let canonical_requested = requested
        .canonicalize()
        .or_else(|_| {
            // File may not exist yet — canonicalize the parent directory instead
            requested
                .parent()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent directory"))
                .and_then(|parent| parent.canonicalize().map(|p| p.join(file_name)))
        })
        .map_err(|e| format!("Invalid config path: {e}"))?;

    // Only allow saves within ~/.config/marauder/
    let allowed_dir = dirs::config_dir()
        .ok_or("Could not determine user config directory")?
        .join("marauder");

    if !canonical_requested.starts_with(&allowed_dir) {
        return Err(format!(
            "Path traversal denied: config can only be saved within {}",
            allowed_dir.display()
        ));
    }

    with_store(&state, |store| {
        store.save_user_config(&canonical_requested).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn config_cmd_reload(
    state: tauri::State<'_, TauriConfigStore>,
) -> Result<(), String> {
    // Phase 1: read files with only a read lock (no blocking writers/readers long-term).
    let layers = with_store(&state, |store| {
        store.read_layers_from_disk().map_err(|e| e.to_string())
    })?;

    // Phase 2: apply the pre-read layers under a short write lock.
    with_store_mut(&state, |store| {
        store.apply_reload(layers);
        Ok(())
    })
}
