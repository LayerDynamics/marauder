use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// A request from the webview to the deno_core JsRuntime thread.
pub enum DenoRequest {
    /// Evaluate arbitrary JS code and return the result as a string.
    Eval {
        code: String,
        reply: oneshot::Sender<Result<String, String>>,
    },
    /// Call a registered `#[op2]` by name with JSON args array.
    CallOp {
        op_name: String,
        args: Vec<serde_json::Value>,
        reply: oneshot::Sender<Result<String, String>>,
    },
}

/// Managed Tauri state that holds the sending half of the IPC channel.
pub struct DenoBridge {
    pub(crate) tx: mpsc::Sender<DenoRequest>,
}

/// Op name prefixes allowed for `deno_call_op`. Rejects arbitrary code execution.
const ALLOWED_OP_PREFIXES: &[&str] = &[
    "op_pty_",
    "op_grid_",
    "op_event_bus_",
    "op_parser_",
    "op_config_",
    "op_runtime_",
    "op_renderer_",
    "op_compute_",
    "op_daemon_",
    "op_ipc_",
];

/// Maximum time to wait for the Deno runtime to reply before returning an error.
const DENO_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Await a oneshot reply with a timeout so a hung Deno runtime doesn't block
/// the Tauri command (and thus the webview) indefinitely.
async fn await_reply(rx: oneshot::Receiver<Result<String, String>>) -> Result<String, String> {
    match tokio::time::timeout(DENO_REPLY_TIMEOUT, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => Err(format!("Deno runtime dropped the reply channel: {}", e)),
        Err(_) => Err("Deno runtime did not respond within 30s".to_string()),
    }
}

fn is_op_allowed(op_name: &str) -> bool {
    op_name.starts_with("op_")
        && ALLOWED_OP_PREFIXES.iter().any(|prefix| op_name.starts_with(prefix))
        && op_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Tauri command: evaluate JS code in the embedded deno_core JsRuntime.
///
/// **Security**: This command executes arbitrary JavaScript with full access to all
/// registered ops (PTY, filesystem, config). It is only available in debug builds
/// to prevent privilege escalation if the webview is ever compromised.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn deno_eval(
    state: tauri::State<'_, DenoBridge>,
    code: String,
) -> Result<String, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    state
        .tx
        .send(DenoRequest::Eval {
            code,
            reply: reply_tx,
        })
        .await
        .map_err(|_| "Deno runtime thread is not running".to_string())?;

    await_reply(reply_rx).await
}

/// Stub for release builds — always returns an error.
#[cfg(not(debug_assertions))]
#[tauri::command]
pub async fn deno_eval(
    _state: tauri::State<'_, DenoBridge>,
    _code: String,
) -> Result<String, String> {
    Err("deno_eval is disabled in release builds".to_string())
}

/// Tauri command: call a registered op by name with a JSON args array.
///
/// Accepts a `Vec<serde_json::Value>` directly from Tauri's JSON deserialization,
/// eliminating the need for manual comma-separated string building and validation.
#[tauri::command]
pub async fn deno_call_op(
    state: tauri::State<'_, DenoBridge>,
    op_name: String,
    args: Vec<serde_json::Value>,
) -> Result<String, String> {
    if !is_op_allowed(&op_name) {
        return Err(format!("Op '{}' is not in the allowlist", op_name));
    }

    let (reply_tx, reply_rx) = oneshot::channel();
    state
        .tx
        .send(DenoRequest::CallOp {
            op_name,
            args,
            reply: reply_tx,
        })
        .await
        .map_err(|_| "Deno runtime thread is not running".to_string())?;

    await_reply(reply_rx).await
}

/// Default keybinding map — used when no config override exists.
/// Must stay in sync with lib/ui/user/config.ts DEFAULT_KEYBINDINGS.
fn default_keybindings() -> &'static [(&'static str, &'static str)] {
    &[
        ("Ctrl+T", "new-tab"),
        ("Ctrl+W", "close-tab"),
        ("Ctrl+Tab", "next-tab"),
        ("Ctrl+Shift+Tab", "prev-tab"),
        ("Ctrl+Shift+N", "split-pane"),
        ("Ctrl+Shift+W", "close-pane"),
        ("Ctrl+Shift+Right", "focus-next"),
        ("Ctrl+Shift+Left", "focus-prev"),
        ("Ctrl+Shift+P", "command-palette"),
        ("Ctrl+Shift+F", "search"),
        ("Ctrl+Plus", "font-size-increase"),
        ("Ctrl+Minus", "font-size-decrease"),
        ("Ctrl+0", "font-size-reset"),
    ]
}

/// Tauri command: resolve a key sequence to a UI action via config keybindings.
///
/// Returns `{ action: string | null }`. Null means the key should pass through to PTY.
/// First checks the config store for user overrides (`keybindings.<key>`), then falls
/// back to the built-in default map. Runs entirely in Rust — no Deno Eval needed.
#[tauri::command]
pub async fn resolve_keybinding(
    state: tauri::State<'_, DenoBridge>,
    key_seq: String,
) -> Result<serde_json::Value, String> {
    // Phase 1: Try config store via CallOp (works in all builds)
    let config_key = format!("keybindings.{}", key_seq);
    let (reply_tx, reply_rx) = oneshot::channel();
    let config_result = state
        .tx
        .send(DenoRequest::CallOp {
            op_name: "op_config_get".into(),
            args: vec![serde_json::Value::String(config_key)],
            reply: reply_tx,
        })
        .await;

    if config_result.is_ok() {
        if let Ok(Ok(Ok(value))) = tokio::time::timeout(Duration::from_millis(500), reply_rx).await {
            // Config returned a value — parse it
            let trimmed = value.trim().trim_matches('"');
            if !trimmed.is_empty() && trimmed != "null" && trimmed != "undefined" {
                let action: String = serde_json::from_str(&value)
                    .unwrap_or_else(|_| trimmed.to_string());
                return Ok(serde_json::json!({ "action": action }));
            }
        }
    }

    // Phase 2: Check built-in defaults (pure Rust, no Deno needed)
    for &(key, action) in default_keybindings() {
        if key == key_seq {
            return Ok(serde_json::json!({ "action": action }));
        }
    }

    Ok(serde_json::json!({ "action": null }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_allowlist() {
        assert!(is_op_allowed("op_pty_create"));
        assert!(is_op_allowed("op_grid_get_cell"));
        assert!(is_op_allowed("op_event_bus_publish"));
        assert!(is_op_allowed("op_parser_create"));
        assert!(is_op_allowed("op_config_get"));
        assert!(is_op_allowed("op_runtime_boot"));
        assert!(is_op_allowed("op_renderer_render_frame"));
        assert!(is_op_allowed("op_compute_search"));
        assert!(is_op_allowed("op_daemon_start"));
        assert!(is_op_allowed("op_ipc_connect"));
        assert!(!is_op_allowed("op_evil"));
        assert!(!is_op_allowed("eval"));
        assert!(!is_op_allowed("op_pty_create; rm -rf /"));
        assert!(!is_op_allowed(""));
    }

}
