# Marauder terminal emulator — Fish shell integration
# Provides: CWD reporting (OSC 7), shell zones (OSC 133), prompt marking
#
# Source this file from ~/.config/fish/conf.d/marauder.fish or call:
#   source (marauder --print-fish-integration)

# Guard against double-sourcing
if set -q __MARAUDER_FISH_LOADED
    exit
end
set -g __MARAUDER_FISH_LOADED 1

# Identify this terminal to child processes
set -gx TERM_PROGRAM marauder
set -gx TERM_PROGRAM_VERSION 0.1.0

# ---------------------------------------------------------------------------
# OSC helpers (defined as functions so they can be composed)
# ---------------------------------------------------------------------------

function __marauder_emit_osc7
    set -l host (hostname 2>/dev/null; or echo "localhost")
    set -l cwd (string replace --all ' ' '%20' -- $PWD)
    printf '\033]7;file://%s%s\033\\' $host $cwd
end

function __marauder_emit_prompt_start
    printf '\033]133;A\033\\'
end

function __marauder_emit_prompt_end
    printf '\033]133;B\033\\'
end

function __marauder_emit_cmd_start
    printf '\033]133;C\033\\'
end

function __marauder_emit_cmd_finished
    set -l code $argv[1]
    if test -z "$code"
        set code 0
    end
    printf '\033]133;D;%s\033\\' $code
end

# ---------------------------------------------------------------------------
# Event handlers
# ---------------------------------------------------------------------------

# fish_prompt fires whenever the prompt is about to be displayed.
# Emit OSC 7 (CWD) and OSC 133;A (prompt start) here.
function __marauder_prompt --on-event fish_prompt
    __marauder_emit_osc7
    __marauder_emit_prompt_start
    # OSC 133;B (prompt end) must be emitted at the end of the prompt string.
    # Because we cannot modify the user's fish_prompt function here, we append
    # it via a wrapper if it has not already been inserted.
    #
    # Emit prompt_end immediately after prompt_start so the terminal marks the
    # correct region. Users who want precise prompt-end marking should emit
    # OSC 133;B themselves at the end of their fish_prompt function.
    __marauder_emit_prompt_end
end

# fish_preexec fires after the user submits a command but before it runs.
# $argv[1] contains the raw command string.
function __marauder_preexec --on-event fish_preexec
    __marauder_emit_cmd_start
end

# fish_postexec fires after a command finishes.
# $argv[2] is the exit status of the last command.
function __marauder_postexec --on-event fish_postexec
    set -l exit_code $argv[2]
    if test -z "$exit_code"
        set exit_code 0
    end
    __marauder_emit_cmd_finished $exit_code
end
