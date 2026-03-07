#!/usr/bin/env bash
# marauder-ext — Extension management CLI wrapper
# Usage: marauder-ext <subcommand> [args...]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

exec deno run \
  --allow-read \
  --allow-write \
  --allow-run \
  --allow-env \
  "$PROJECT_DIR/lib/cli/ext.ts" \
  "$@"
