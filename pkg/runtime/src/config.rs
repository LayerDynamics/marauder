use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Serialize, Deserialize};

/// Configuration for bootstrapping the runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Shell to spawn (defaults to $SHELL or /bin/sh).
    pub shell: String,
    /// Environment variables for PTY sessions.
    pub env: HashMap<String, String>,
    /// Working directory for new PTY sessions.
    pub cwd: PathBuf,
    /// Initial terminal height (rows).
    pub rows: u16,
    /// Initial terminal width (columns).
    pub cols: u16,
    /// System config file path.
    pub system_config: Option<PathBuf>,
    /// User config file path.
    pub user_config: Option<PathBuf>,
    /// Project config file path.
    pub project_config: Option<PathBuf>,
    /// Enable file watching for config changes.
    pub watch_config: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        Self {
            shell,
            env: HashMap::new(),
            cwd,
            rows: 24,
            cols: 80,
            system_config: None,
            user_config: Some(dirs_config_path()),
            project_config: None,
            watch_config: true,
        }
    }
}

/// Default user config path, respecting XDG_CONFIG_HOME.
///
/// Resolution order:
/// 1. `$XDG_CONFIG_HOME/marauder/config.toml` (if set)
/// 2. `$HOME/.config/marauder/config.toml`
/// 3. `/etc/marauder/config.toml` (fallback)
fn dirs_config_path() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("marauder/config.toml");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/marauder/config.toml");
    }
    PathBuf::from("/etc/marauder/config.toml")
}
