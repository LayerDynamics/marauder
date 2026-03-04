use std::io::{Read, Write};
use std::path::PathBuf;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::manager::PtyConfig;

/// Result of opening a PTY: the master, a writer, a reader, and the child process.
pub struct OpenPtyResult {
    pub master: Box<dyn MasterPty + Send>,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn Child + Send + Sync>,
}

/// Open a PTY pair and spawn the shell process.
/// Takes the reader and writer upfront so they're not consumed repeatedly.
pub fn open_pty(config: &PtyConfig) -> anyhow::Result<OpenPtyResult> {
    anyhow::ensure!(config.rows > 0, "rows must be > 0");
    anyhow::ensure!(config.cols > 0, "cols must be > 0");

    let pty_system = native_pty_system();

    let size = PtySize {
        rows: config.rows,
        cols: config.cols,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system.openpty(size)?;

    let mut cmd = CommandBuilder::new(&config.shell);
    for (key, val) in &config.env {
        cmd.env(key, val);
    }
    if config.cwd.exists() {
        cmd.cwd(&config.cwd);
    } else {
        tracing::warn!(cwd = %config.cwd.display(), "PTY cwd does not exist, using default");
    }

    let child = pair.slave.spawn_command(cmd)?;
    // Explicitly drop the slave side now that the child is spawned.
    // On some platforms, not closing it can cause read issues.
    drop(pair.slave);

    let reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;

    Ok(OpenPtyResult {
        master: pair.master,
        reader,
        writer,
        child,
    })
}

/// Resize a PTY master.
pub fn resize_master(master: &dyn MasterPty, rows: u16, cols: u16) -> anyhow::Result<()> {
    anyhow::ensure!(rows > 0, "rows must be > 0");
    anyhow::ensure!(cols > 0, "cols must be > 0");
    master.resize(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    Ok(())
}

/// Detect the default shell for the current user.
pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

/// Create a default PtyConfig using the user's shell and current directory.
pub fn default_config(rows: u16, cols: u16) -> PtyConfig {
    PtyConfig {
        shell: default_shell(),
        env: std::collections::HashMap::new(),
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
        rows,
        cols,
    }
}
