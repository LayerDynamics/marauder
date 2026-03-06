#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# Marauder Terminal Emulator — Launch Wrapper
# ---------------------------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

VERSION="0.1.0"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()  { printf '\033[0;34m[info]\033[0m  %s\n' "$*"; }
warn()  { printf '\033[0;33m[warn]\033[0m  %s\n' "$*"; }
error() { printf '\033[0;31m[error]\033[0m %s\n' "$*" >&2; }
die()   { error "$*"; exit 1; }

# ---------------------------------------------------------------------------
# Locate shared libraries (dylibs / .so files)
# ---------------------------------------------------------------------------

resolve_lib_dir() {
    # If the caller already set MARAUDER_LIB_DIR, trust it.
    if [[ -n "${MARAUDER_LIB_DIR:-}" ]]; then
        if [[ -d "$MARAUDER_LIB_DIR" ]]; then
            return
        else
            warn "MARAUDER_LIB_DIR is set but does not exist: $MARAUDER_LIB_DIR"
        fi
    fi

    # Check Tauri-specific release dir first (Tauri places libs next to the binary)
    local tauri_release="$REPO_ROOT/apps/marauder/src-tauri/target/release"
    local workspace_release="$REPO_ROOT/target/release"
    local workspace_debug="$REPO_ROOT/target/debug"
    local system_lib="/usr/local/lib/marauder"

    if [[ -d "$tauri_release" ]]; then
        export MARAUDER_LIB_DIR="$tauri_release"
    elif [[ -d "$workspace_release" ]]; then
        export MARAUDER_LIB_DIR="$workspace_release"
    elif [[ -d "$workspace_debug" ]]; then
        warn "Release build not found; falling back to debug libraries."
        export MARAUDER_LIB_DIR="$workspace_debug"
    elif [[ -d "$system_lib" ]]; then
        export MARAUDER_LIB_DIR="$system_lib"
    else
        warn "Could not locate Marauder shared libraries. Set MARAUDER_LIB_DIR if needed."
        export MARAUDER_LIB_DIR=""
    fi

    if [[ -n "$MARAUDER_LIB_DIR" ]]; then
        info "Using library directory: $MARAUDER_LIB_DIR"
    fi
}

# ---------------------------------------------------------------------------
# Locate the main binary
# ---------------------------------------------------------------------------

resolve_main_binary() {
    # Tauri puts the binary alongside its resources
    local tauri_bin="$REPO_ROOT/apps/marauder/src-tauri/target/release/marauder"
    local workspace_bin="$REPO_ROOT/target/release/marauder"
    local local_install="$HOME/.local/bin/marauder"
    local system_bin="/usr/local/bin/marauder"

    for candidate in "$tauri_bin" "$workspace_bin" "$local_install" "$system_bin"; do
        if [[ -x "$candidate" ]]; then
            MARAUDER_BIN="$candidate"
            return
        fi
    done

    die "Marauder binary not found. Run bin/install.sh first, or build with: cargo build --release"
}

# ---------------------------------------------------------------------------
# Locate the headless server binary
# ---------------------------------------------------------------------------

resolve_server_binary() {
    local tauri_server="$REPO_ROOT/apps/marauder/src-tauri/target/release/marauder-server"
    local workspace_server="$REPO_ROOT/target/release/marauder-server"
    local local_install="$HOME/.local/bin/marauder-server"
    local system_bin="/usr/local/bin/marauder-server"

    for candidate in "$tauri_server" "$workspace_server" "$local_install" "$system_bin"; do
        if [[ -x "$candidate" ]]; then
            SERVER_BIN="$candidate"
            return
        fi
    done

    die "marauder-server binary not found. Build with: cargo build --release -p marauder-server"
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

HEADLESS=false
DEBUG=false
EXTRA_ARGS=()

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --headless)
                HEADLESS=true
                shift
                ;;
            --debug)
                DEBUG=true
                shift
                ;;
            --version | -v)
                echo "marauder $VERSION"
                exit 0
                ;;
            --help | -h)
                print_help
                exit 0
                ;;
            --)
                shift
                EXTRA_ARGS+=("$@")
                break
                ;;
            *)
                EXTRA_ARGS+=("$1")
                shift
                ;;
        esac
    done
}

print_help() {
    cat <<EOF
marauder — GPU-accelerated terminal emulator

USAGE:
    marauder [OPTIONS] [-- EXTRA_ARGS...]

OPTIONS:
    --headless      Run in headless multiplexer server mode (no GUI)
    --debug         Enable debug logging (sets RUST_LOG=debug)
    --version, -v   Print version and exit
    --help, -h      Print this help message and exit

ENVIRONMENT:
    MARAUDER_LIB_DIR    Path to directory containing Marauder shared libraries
    RUST_LOG            Log level filter (e.g. marauder=debug, info, warn)

EXAMPLES:
    marauder                        Launch the GUI terminal emulator
    marauder --headless             Start the background multiplexer daemon
    marauder --debug                Launch with verbose debug logging
    MARAUDER_LIB_DIR=/custom/path marauder

Version: $VERSION
EOF
}

# ---------------------------------------------------------------------------
# Logging setup
# ---------------------------------------------------------------------------

setup_logging() {
    if [[ "$DEBUG" == true ]]; then
        export RUST_LOG="${RUST_LOG:-marauder=debug,warn}"
        info "Debug logging enabled (RUST_LOG=$RUST_LOG)"
    else
        # Set a sensible default only if the user has not already set it
        export RUST_LOG="${RUST_LOG:-marauder=info}"
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
    parse_args "$@"
    setup_logging
    resolve_lib_dir

    if [[ "$HEADLESS" == true ]]; then
        resolve_server_binary
        info "Starting Marauder in headless server mode..."
        exec "$SERVER_BIN" "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"
    else
        resolve_main_binary
        exec "$MARAUDER_BIN" "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"
    fi
}

main "$@"
