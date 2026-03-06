#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# Marauder Terminal Emulator — Uninstaller
# ---------------------------------------------------------------------------

INSTALL_BIN="$HOME/.local/bin"
CONFIG_DIR="$HOME/.config/marauder"
ZSHRC="$HOME/.zshrc"
BASHRC="$HOME/.bashrc"
FISH_CONFIG="$HOME/.config/fish/config.fish"

# Shell integration patterns to strip from rc files
MARAUDER_INTEGRATION_PATTERN="marauder"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()    { printf '\033[0;34m[info]\033[0m  %s\n' "$*"; }
success() { printf '\033[0;32m[ok]\033[0m    %s\n' "$*"; }
warn()    { printf '\033[0;33m[warn]\033[0m  %s\n' "$*"; }
error()   { printf '\033[0;31m[error]\033[0m %s\n' "$*" >&2; }
die()     { error "$*"; exit 1; }

confirm() {
    local prompt="$1"
    local response
    read -r -p "$prompt [y/N] " response
    case "$response" in
        [yY] | [yY][eE][sS]) return 0 ;;
        *) return 1 ;;
    esac
}

# ---------------------------------------------------------------------------
# Remove binary
# ---------------------------------------------------------------------------

remove_binary() {
    local removed=false

    local main_bin="$INSTALL_BIN/marauder"
    if [[ -f "$main_bin" ]]; then
        info "Removing binary: $main_bin"
        rm -f "$main_bin"
        success "Removed $main_bin"
        removed=true
    else
        info "Binary not found at $main_bin — already removed or never installed."
    fi

    local wrapper_bin="$INSTALL_BIN/marauder-run"
    if [[ -f "$wrapper_bin" ]]; then
        info "Removing launch wrapper: $wrapper_bin"
        rm -f "$wrapper_bin"
        success "Removed $wrapper_bin"
        removed=true
    fi

    local server_bin="$INSTALL_BIN/marauder-server"
    if [[ -f "$server_bin" ]]; then
        info "Removing server binary: $server_bin"
        rm -f "$server_bin"
        success "Removed $server_bin"
        removed=true
    fi

    # Also check system install paths
    for sys_bin in /usr/local/bin/marauder /usr/local/bin/marauder-server; do
        if [[ -f "$sys_bin" ]]; then
            info "Removing system binary: $sys_bin"
            if rm -f "$sys_bin" 2>/dev/null; then
                success "Removed $sys_bin"
                removed=true
            else
                warn "Could not remove $sys_bin — try running with sudo."
            fi
        fi
    done

    if [[ "$removed" == false ]]; then
        warn "No Marauder binaries found to remove."
    fi
}

# ---------------------------------------------------------------------------
# Remove config directory
# ---------------------------------------------------------------------------

remove_config_dir() {
    if [[ ! -d "$CONFIG_DIR" ]]; then
        info "Config directory not found at $CONFIG_DIR — nothing to remove."
        return
    fi

    echo ""
    warn "The config directory contains your settings, themes, fonts, and shell integrations:"
    warn "  $CONFIG_DIR"
    echo ""

    if confirm "Remove config directory $CONFIG_DIR?"; then
        rm -rf "$CONFIG_DIR"
        success "Removed config directory: $CONFIG_DIR"
    else
        info "Config directory kept at $CONFIG_DIR"
    fi
}

# ---------------------------------------------------------------------------
# Strip shell integration lines from an rc file
# ---------------------------------------------------------------------------

strip_rc_lines() {
    local rc_file="$1"
    local shell_name="$2"

    if [[ ! -f "$rc_file" ]]; then
        return
    fi

    # Check if the file contains any marauder references before modifying
    if ! grep -q "$MARAUDER_INTEGRATION_PATTERN" "$rc_file" 2>/dev/null; then
        info "No Marauder integration lines found in $rc_file"
        return
    fi

    info "Removing Marauder integration lines from $rc_file ($shell_name)..."

    local backup="${rc_file}.marauder-uninstall-bak"
    cp "$rc_file" "$backup"

    # Remove lines that source marauder shell integrations or export marauder vars
    # Patterns to strip:
    #   source "...marauder..."
    #   source '...marauder...'
    #   . "...marauder..."
    #   export MARAUDER_...
    #   # marauder ... (comments left by the installer)
    local tmp_file
    tmp_file="$(mktemp)"

    grep -v -E \
        "(source|\.)[[:space:]]+[\"']?[^\"']*marauder[^\"']*[\"']?|export[[:space:]]+MARAUDER_|#[[:space:]]*marauder" \
        "$rc_file" > "$tmp_file" || true

    mv "$tmp_file" "$rc_file"

    success "Cleaned $rc_file (backup saved to $backup)"
}

# ---------------------------------------------------------------------------
# Remove shell integrations from all rc files
# ---------------------------------------------------------------------------

remove_shell_integrations() {
    info "Scanning shell rc files for Marauder integration lines..."

    strip_rc_lines "$ZSHRC"       "zsh"
    strip_rc_lines "$BASHRC"      "bash"

    # fish uses a different syntax — strip source lines referencing marauder
    if [[ -f "$FISH_CONFIG" ]]; then
        if grep -q "$MARAUDER_INTEGRATION_PATTERN" "$FISH_CONFIG" 2>/dev/null; then
            info "Removing Marauder integration lines from $FISH_CONFIG (fish)..."
            local fish_backup="${FISH_CONFIG}.marauder-uninstall-bak"
            cp "$FISH_CONFIG" "$fish_backup"
            local tmp_fish
            tmp_fish="$(mktemp)"
            grep -v -E \
                "source[[:space:]]+[\"']?[^\"']*marauder|set[[:space:]]+-x[[:space:]]+MARAUDER_|#[[:space:]]*marauder" \
                "$FISH_CONFIG" > "$tmp_fish" || true
            mv "$tmp_fish" "$FISH_CONFIG"
            success "Cleaned $FISH_CONFIG (backup saved to $fish_backup)"
        else
            info "No Marauder integration lines found in $FISH_CONFIG"
        fi
    fi
}

# ---------------------------------------------------------------------------
# Print summary
# ---------------------------------------------------------------------------

print_summary() {
    echo ""
    echo "----------------------------------------------------------------------"
    success "Marauder uninstall complete."
    echo "----------------------------------------------------------------------"
    echo ""
    echo "Items that may still remain (not managed by this script):"
    echo "  - Cargo build artifacts: target/"
    echo "    Remove with: cargo clean"
    echo "  - PATH entries (e.g. \$HOME/.local/bin) added manually to rc files"
    echo "  - Shell rc backup files: *.marauder-uninstall-bak"
    echo ""
    echo "To remove backup files:"
    echo "  rm -f ~/.zshrc.marauder-uninstall-bak"
    echo "  rm -f ~/.bashrc.marauder-uninstall-bak"
    echo "  rm -f ~/.config/fish/config.fish.marauder-uninstall-bak"
    echo ""
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
    echo ""
    echo "  Marauder Terminal Emulator — Uninstaller"
    echo "  =========================================="
    echo ""
    warn "This will remove Marauder binaries and shell integrations from your system."
    echo ""

    if ! confirm "Are you sure you want to uninstall Marauder?"; then
        info "Uninstall cancelled."
        exit 0
    fi

    echo ""

    remove_binary
    echo ""
    remove_shell_integrations
    echo ""
    remove_config_dir

    print_summary
}

main "$@"
