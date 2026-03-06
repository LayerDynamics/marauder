# Marauder terminal emulator — Zsh shell integration
# Provides: CWD reporting (OSC 7), shell zones (OSC 133), prompt marking

# Guard against double-sourcing
[[ -n "$__MARAUDER_ZSH_LOADED" ]] && return
__MARAUDER_ZSH_LOADED=1

# Identify this terminal to child processes
export TERM_PROGRAM=marauder
export TERM_PROGRAM_VERSION=0.1.0

# ---------------------------------------------------------------------------
# OSC helpers
# ---------------------------------------------------------------------------

# Emit OSC 7: report current working directory
__marauder_emit_osc7() {
  local host cwd
  host="${HOST:-$(hostname -f 2>/dev/null || hostname)}"
  cwd="${PWD}"
  # Percent-encode spaces in path (minimal encoding — terminals decode the rest)
  cwd="${cwd// /%20}"
  printf '\033]7;file://%s%s\033\\' "${host}" "${cwd}"
}

# OSC 133 zone markers
__marauder_emit_prompt_start()    { printf '\033]133;A\033\\'; }
__marauder_emit_prompt_end()      { printf '\033]133;B\033\\'; }
__marauder_emit_cmd_start()       { printf '\033]133;C\033\\'; }
__marauder_emit_cmd_finished() {
  local code="${1:-0}"
  printf '\033]133;D;%s\033\\' "${code}"
}

# ---------------------------------------------------------------------------
# Hook functions
# ---------------------------------------------------------------------------

# precmd: runs before each prompt is displayed
__marauder_precmd() {
  local exit_code=$?
  __marauder_emit_cmd_finished "${exit_code}"
  __marauder_emit_osc7
}

# preexec: runs after the user presses Enter but before the command runs
__marauder_preexec() {
  __marauder_emit_cmd_start
}

# ---------------------------------------------------------------------------
# Prompt wrapping — inject OSC 133;A before PS1
# ---------------------------------------------------------------------------

# Wrap the existing prompt so OSC 133;A appears before visible prompt text and
# OSC 133;B appears after (marking the end of the prompt / start of user input).
# We use a zsh prompt substitution escape (%{...%}) so the sequences are
# treated as zero-width and do not corrupt line editing.
__marauder_wrap_prompt() {
  local ps_start ps_end
  ps_start=$'%{\033]133;A\033\\%}'
  ps_end=$'%{\033]133;B\033\\%}'
  # Only wrap once
  [[ "${PS1}" == *"133;A"* ]] && return
  PS1="${ps_start}${PS1}${ps_end}"
}

# ---------------------------------------------------------------------------
# Install hooks
# ---------------------------------------------------------------------------

# Register precmd and preexec hooks (arrays are the idiomatic zsh mechanism)
precmd_functions+=(__marauder_precmd)
preexec_functions+=(__marauder_preexec)

# Wrap prompt after all other init has run (use precmd for timing safety)
__marauder_init_prompt() {
  __marauder_wrap_prompt
  # Remove this one-shot hook
  precmd_functions=(${precmd_functions:#__marauder_init_prompt})
}
precmd_functions+=(__marauder_init_prompt)
