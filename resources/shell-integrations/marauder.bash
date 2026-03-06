# Marauder terminal emulator — Bash shell integration
# Provides: CWD reporting (OSC 7), shell zones (OSC 133), prompt marking

# Guard against double-sourcing
[[ -n "$__MARAUDER_BASH_LOADED" ]] && return
__MARAUDER_BASH_LOADED=1

# Identify this terminal to child processes
export TERM_PROGRAM=marauder
export TERM_PROGRAM_VERSION=0.1.0

# ---------------------------------------------------------------------------
# OSC helpers
# ---------------------------------------------------------------------------

__marauder_emit_osc7() {
  local host cwd
  host="${HOSTNAME:-$(hostname -f 2>/dev/null || hostname)}"
  cwd="${PWD}"
  # Minimal percent-encoding for spaces
  cwd="${cwd// /%20}"
  printf '\033]7;file://%s%s\033\\' "${host}" "${cwd}"
}

__marauder_emit_prompt_start()    { printf '\033]133;A\033\\'; }
__marauder_emit_prompt_end()      { printf '\033]133;B\033\\'; }
__marauder_emit_cmd_start()       { printf '\033]133;C\033\\'; }
__marauder_emit_cmd_finished() {
  local code="${1:-0}"
  printf '\033]133;D;%s\033\\' "${code}"
}

# ---------------------------------------------------------------------------
# Preexec emulation via DEBUG trap
# ---------------------------------------------------------------------------
# Bash does not have a built-in preexec hook. We use the DEBUG trap, which
# fires before every simple command. We gate it with __marauder_cmd_pending so
# we only emit OSC 133;C once per interactive command (not for every pipeline
# stage or subshell).

__marauder_cmd_pending=0
__marauder_last_hist=""

__marauder_debug_trap() {
  # Only act on interactive input, not on functions sourced during prompt setup
  if [[ "${BASH_COMMAND}" != "__marauder_"* ]] && \
     [[ "${BASH_COMMAND}" != ":" ]]              && \
     [[ "$__marauder_cmd_pending" -eq 0 ]]; then
    local current_hist
    current_hist="$(HISTTIMEFORMAT= builtin history 1 2>/dev/null)"
    if [[ "${current_hist}" != "${__marauder_last_hist}" ]]; then
      __marauder_last_hist="${current_hist}"
      __marauder_cmd_pending=1
      __marauder_emit_cmd_start
    fi
  fi
}

trap '__marauder_debug_trap' DEBUG

# ---------------------------------------------------------------------------
# PROMPT_COMMAND — runs before each prompt
# ---------------------------------------------------------------------------

__marauder_prompt_command() {
  local exit_code=$?
  # Reset pending flag so next command triggers preexec again
  __marauder_cmd_pending=0

  # OSC 133;D — command finished with exit code
  __marauder_emit_cmd_finished "${exit_code}"

  # OSC 7 — report current directory
  __marauder_emit_osc7

  # OSC 133;A — prompt start (emitted here, before PS1 is displayed)
  __marauder_emit_prompt_start
}

# Prepend to any existing PROMPT_COMMAND
if [[ -n "$PROMPT_COMMAND" ]]; then
  # Avoid adding a trailing semicolon if PROMPT_COMMAND already ends with one
  PROMPT_COMMAND="__marauder_prompt_command; ${PROMPT_COMMAND%;};"
else
  PROMPT_COMMAND="__marauder_prompt_command"
fi

# ---------------------------------------------------------------------------
# Prompt suffix — OSC 133;B marks end of prompt / start of user input
# ---------------------------------------------------------------------------
# Append a non-printing sequence to PS1 so the terminal knows where input begins.
# \[ ... \] wraps zero-width sequences in Bash prompt strings.
if [[ "${PS1}" != *"133;B"* ]]; then
  PS1="${PS1}"$'\[\033]133;B\033\\\]'
fi
