#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# Marauder Terminal Emulator — Installer
# ---------------------------------------------------------------------------

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_BIN="$HOME/.local/bin"
CONFIG_DIR="$HOME/.config/marauder"
FONTS_DIR="$CONFIG_DIR/fonts"
SHELL_INT_DIR="$CONFIG_DIR/shell-integrations"

# JetBrains Mono release URL (latest stable)
JETBRAINS_MONO_URL="https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()    { printf '\033[0;34m[info]\033[0m  %s\n' "$*"; }
success() { printf '\033[0;32m[ok]\033[0m    %s\n' "$*"; }
warn()    { printf '\033[0;33m[warn]\033[0m  %s\n' "$*"; }
error()   { printf '\033[0;31m[error]\033[0m %s\n' "$*" >&2; }
die()     { error "$*"; exit 1; }

require_cmd() {
    local cmd="$1"
    local hint="${2:-}"
    if ! command -v "$cmd" &>/dev/null; then
        if [[ -n "$hint" ]]; then
            die "Required command '$cmd' not found. $hint"
        else
            die "Required command '$cmd' not found. Please install it and re-run."
        fi
    fi
}

# ---------------------------------------------------------------------------
# OS / arch detection
# ---------------------------------------------------------------------------

detect_platform() {
    local os
    os="$(uname -s)"
    case "$os" in
        Darwin) OS="macos" ;;
        Linux)  OS="linux" ;;
        *)      die "Unsupported OS: $os. Marauder supports macOS and Linux." ;;
    esac

    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64)          ARCH="x86_64" ;;
        arm64 | aarch64) ARCH="aarch64" ;;
        *)               die "Unsupported architecture: $arch." ;;
    esac

    info "Detected platform: $OS / $ARCH"
}

# ---------------------------------------------------------------------------
# Dependency checks
# ---------------------------------------------------------------------------

check_dependencies() {
    info "Checking required dependencies..."

    require_cmd rustc "Install Rust via: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    require_cmd cargo "Install Rust via: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    require_cmd deno  "Install Deno via: curl -fsSL https://deno.land/install.sh | sh"
    require_cmd curl  "Install curl via your system package manager (apt, brew, etc.)"

    local rustc_version
    rustc_version="$(rustc --version)"
    info "  rustc: $rustc_version"

    local deno_version
    deno_version="$(deno --version | head -1)"
    info "  deno:  $deno_version"

    success "All dependencies found."
}

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

build_release() {
    info "Building Marauder from source (release mode)..."
    info "  This may take several minutes on first build."

    cd "$REPO_ROOT"

    if ! cargo build --release 2>&1; then
        die "cargo build --release failed. Check the output above for details."
    fi

    success "Build complete."
}

# ---------------------------------------------------------------------------
# Install binary
# ---------------------------------------------------------------------------

install_binary() {
    info "Installing binary to $INSTALL_BIN/marauder..."

    mkdir -p "$INSTALL_BIN"

    local tauri_bin="$REPO_ROOT/apps/marauder/src-tauri/target/release/marauder"
    local workspace_bin="$REPO_ROOT/target/release/marauder"

    local src_bin=""
    if [[ -f "$tauri_bin" ]]; then
        src_bin="$tauri_bin"
    elif [[ -f "$workspace_bin" ]]; then
        src_bin="$workspace_bin"
    else
        die "Built binary not found at '$tauri_bin' or '$workspace_bin'. Ensure cargo build --release succeeded."
    fi

    cp "$src_bin" "$INSTALL_BIN/marauder"
    chmod +x "$INSTALL_BIN/marauder"

    # Also install the launch wrapper script
    cp "$REPO_ROOT/bin/marauder.sh" "$INSTALL_BIN/marauder-run"
    chmod +x "$INSTALL_BIN/marauder-run"

    success "Binary installed at $INSTALL_BIN/marauder"

    # Warn if not in PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_BIN"; then
        warn "$INSTALL_BIN is not in your PATH."
        warn "Add the following to your shell rc file:"
        warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
}

# ---------------------------------------------------------------------------
# Config directory + default config
# ---------------------------------------------------------------------------

setup_config_dir() {
    info "Setting up config directory at $CONFIG_DIR..."

    mkdir -p "$CONFIG_DIR"
    mkdir -p "$FONTS_DIR"
    mkdir -p "$SHELL_INT_DIR"

    local config_file="$CONFIG_DIR/config.toml"
    if [[ -f "$config_file" ]]; then
        warn "Config file already exists at $config_file — skipping default creation."
    else
        cat > "$config_file" <<'TOML'
# Marauder Terminal Emulator — Default Configuration
# Full documentation: https://github.com/LayerDynamics/marauder/docs/Configuration.md

[shell]
# Shell to launch. Defaults to $SHELL env var, then /bin/zsh, then /bin/bash.
# program = "/bin/zsh"
args = []

[terminal]
scrollback_lines = 10000
cols = 80
rows = 24

[font]
family = "JetBrains Mono"
size = 13.0
line_height = 1.2
# Additional fallback families (system fonts)
fallback = ["Menlo", "Monaco", "Consolas", "monospace"]

[renderer]
# Target frame rate (frames per second)
fps = 120
# Cursor style: "block" | "underline" | "bar"
cursor_style = "block"
cursor_blink = true

[theme]
# Built-in themes: "dark", "light", "dracula", "catppuccin-mocha"
name = "dark"

[keybindings]
# Keybinding overrides (see docs/Keybindings.md for full list)

[extensions]
# Extensions to load on startup
enabled = ["theme-default", "status-bar"]
TOML
        success "Default config written to $config_file"
    fi
}

# ---------------------------------------------------------------------------
# Shell integrations
# ---------------------------------------------------------------------------

install_shell_integrations() {
    local src_dir="$REPO_ROOT/resources/shell-integrations"

    if [[ ! -d "$src_dir" ]]; then
        warn "Shell integrations source directory not found at $src_dir — skipping."
        return
    fi

    local file_count
    file_count="$(find "$src_dir" -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d ' ')"
    if [[ "$file_count" -eq 0 ]]; then
        warn "No shell integration files found in $src_dir — skipping."
        return
    fi

    info "Copying shell integrations to $SHELL_INT_DIR..."
    cp "$src_dir/"* "$SHELL_INT_DIR/"
    success "Shell integrations installed ($file_count files)."
}

# ---------------------------------------------------------------------------
# JetBrains Mono font
# ---------------------------------------------------------------------------

install_font() {
    local zip_path="/tmp/JetBrainsMono.zip"
    local extract_dir="/tmp/JetBrainsMono-extract"

    info "Downloading JetBrains Mono font..."

    if ! curl -fsSL --retry 3 --retry-delay 2 -o "$zip_path" "$JETBRAINS_MONO_URL"; then
        warn "Failed to download JetBrains Mono. Skipping font install."
        warn "You can download it manually from: $JETBRAINS_MONO_URL"
        return
    fi

    info "Extracting font..."
    rm -rf "$extract_dir"
    mkdir -p "$extract_dir"

    if ! unzip -q "$zip_path" -d "$extract_dir"; then
        warn "Failed to extract font archive. Skipping font install."
        rm -f "$zip_path"
        return
    fi

    # Copy .ttf files into the fonts dir
    local ttf_count=0
    while IFS= read -r -d '' ttf; do
        cp "$ttf" "$FONTS_DIR/"
        ttf_count=$((ttf_count + 1))
    done < <(find "$extract_dir" -name "*.ttf" -print0 2>/dev/null)

    if [[ $ttf_count -eq 0 ]]; then
        warn "No .ttf files found in font archive. Skipping font install."
    else
        success "Installed $ttf_count JetBrains Mono font files to $FONTS_DIR"
    fi

    rm -f "$zip_path"
    rm -rf "$extract_dir"
}

# ---------------------------------------------------------------------------
# Post-install instructions
# ---------------------------------------------------------------------------

print_post_install() {
    echo ""
    echo "----------------------------------------------------------------------"
    success "Marauder installed successfully!"
    echo "----------------------------------------------------------------------"
    echo ""
    echo "  Binary:  $INSTALL_BIN/marauder"
    echo "  Config:  $CONFIG_DIR/config.toml"
    echo "  Fonts:   $FONTS_DIR"
    echo ""
    echo "Shell integration (optional but recommended):"
    echo ""

    if [[ -f "$SHELL_INT_DIR/marauder.zsh" ]]; then
        echo "  zsh — add to ~/.zshrc:"
        echo "    source \"$SHELL_INT_DIR/marauder.zsh\""
        echo ""
    fi

    if [[ -f "$SHELL_INT_DIR/marauder.bash" ]]; then
        echo "  bash — add to ~/.bashrc:"
        echo "    source \"$SHELL_INT_DIR/marauder.bash\""
        echo ""
    fi

    if [[ -f "$SHELL_INT_DIR/marauder.fish" ]]; then
        echo "  fish — add to ~/.config/fish/config.fish:"
        echo "    source \"$SHELL_INT_DIR/marauder.fish\""
        echo ""
    fi

    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_BIN"; then
        echo "  PATH — add to your shell rc file:"
        echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
        echo ""
    fi

    echo "Run 'marauder --help' to get started."
    echo ""
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
    echo ""
    echo "  Marauder Terminal Emulator — Installer"
    echo "  ======================================="
    echo ""

    detect_platform
    check_dependencies
    build_release
    install_binary
    setup_config_dir
    install_shell_integrations
    install_font
    print_post_install
}

main "$@"
