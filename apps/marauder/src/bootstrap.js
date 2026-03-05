// Marauder Deno runtime bootstrap
// Ops are available via Deno.core.ops.*
//
// When running in embedded Tauri mode, the primary runtime's subsystems
// (event bus, PTY manager, config store, grids, parsers) are injected into
// OpState by lib.rs. JS ops operate on the REAL state — not phantom copies.
//
// The config store is pre-registered at handle 0.
// Pane grids/parsers are pre-registered at handle = pane_id.
// Use subsystem ops directly rather than op_runtime_create.
((globalThis) => {
  const core = Deno.core;

  // Check if the primary runtime is attached (managed by Rust)
  const primaryAttached = core.ops.op_runtime_is_primary_attached() === 1;

  // Expose a marauder namespace for convenience
  globalThis.marauder = {
    // Whether the primary runtime is managed by Rust (true in Tauri mode)
    primaryAttached,

    // Event bus ops — always connected to the real bus when injected
    eventBus: {
      publish: (eventType, payloadJson) => core.ops.op_event_bus_publish(eventType, payloadJson),
      subscriberCount: (eventType) => core.ops.op_event_bus_subscriber_count(eventType),
      subscribe: (eventType) => core.ops.op_event_bus_subscribe(eventType),
      unsubscribe: (handle) => core.ops.op_event_bus_unsubscribe(handle),
      poll: (handle) => core.ops.op_event_bus_poll(handle),
      addInterceptor: (priority) => core.ops.op_event_bus_add_interceptor(priority),
      removeInterceptor: (handle) => core.ops.op_event_bus_remove_interceptor(handle),
    },
    // PTY ops — connected to the real PTY manager when injected
    pty: {
      create: (shell, cwd, rows, cols) => core.ops.op_pty_create(shell, cwd, rows, cols),
      write: (paneId, data) => core.ops.op_pty_write(paneId, data),
      read: (paneId, maxBytes) => core.ops.op_pty_read(paneId, maxBytes),
      resize: (paneId, rows, cols) => core.ops.op_pty_resize(paneId, rows, cols),
      close: (paneId) => core.ops.op_pty_close(paneId),
      getPid: (paneId) => core.ops.op_pty_get_pid(paneId),
      wait: (paneId) => core.ops.op_pty_wait(paneId),
      count: () => core.ops.op_pty_count(),
    },
    // Parser ops — shared parsers are at handle = pane_id
    parser: {
      create: () => core.ops.op_parser_create(),
      feed: (handle, data) => core.ops.op_parser_feed(handle, data),
      reset: (handle) => core.ops.op_parser_reset(handle),
      destroy: (handle) => core.ops.op_parser_destroy(handle),
    },
    // Grid ops — shared grids are at handle = pane_id
    grid: {
      create: (rows, cols) => core.ops.op_grid_create(rows, cols),
      applyAction: (handle, action) => core.ops.op_grid_apply_action(handle, action),
      getCell: (handle, row, col) => core.ops.op_grid_get_cell(handle, row, col),
      getCursor: (handle) => core.ops.op_grid_get_cursor(handle),
      resize: (handle, rows, cols) => core.ops.op_grid_resize(handle, rows, cols),
      getDirtyRows: (handle) => core.ops.op_grid_get_dirty_rows(handle),
      clearDirty: (handle) => core.ops.op_grid_clear_dirty(handle),
      select: (handle, sr, sc, er, ec) => core.ops.op_grid_select(handle, sr, sc, er, ec),
      getSelectionText: (handle) => core.ops.op_grid_get_selection_text(handle),
      scrollViewport: (handle, offset) => core.ops.op_grid_scroll_viewport(handle, offset),
      destroy: (handle) => core.ops.op_grid_destroy(handle),
    },
    // Config store ops — primary config at handle 0 when injected
    config: {
      create: () => core.ops.op_config_create(),
      load: (handle, sys, usr, prj) => core.ops.op_config_load(handle, sys, usr, prj),
      get: (handle, key) => core.ops.op_config_get(handle, key),
      set: (handle, key, value) => core.ops.op_config_set(handle, key, value),
      save: (handle, path) => core.ops.op_config_save(handle, path),
      reload: (handle) => core.ops.op_config_reload(handle),
      keys: (handle) => core.ops.op_config_keys(handle),
      destroy: (handle) => core.ops.op_config_destroy(handle),
    },
    // Runtime ops — in Tauri mode, use subsystem ops directly instead of creating runtimes
    runtime: {
      isPrimaryAttached: () => core.ops.op_runtime_is_primary_attached() === 1,
      create: () => core.ops.op_runtime_create(),
      boot: (handle) => core.ops.op_runtime_boot(handle),
      shutdown: (handle) => core.ops.op_runtime_shutdown(handle),
      createPane: (handle) => core.ops.op_runtime_create_pane(handle),
      closePane: (handle, paneId) => core.ops.op_runtime_close_pane(handle, paneId),
      writeToPane: (handle, paneId, data) => core.ops.op_runtime_write_to_pane(handle, paneId, data),
      resizePane: (handle, paneId, rows, cols) => core.ops.op_runtime_resize_pane(handle, paneId, rows, cols),
      paneIds: (handle) => core.ops.op_runtime_pane_ids(handle),
      state: (handle) => core.ops.op_runtime_state(handle),
      destroy: (handle) => core.ops.op_runtime_destroy(handle),
    },
  };

  if (primaryAttached) {
    core.print("[marauder] Deno runtime initialized (shared state from primary runtime)\n");
  } else {
    core.print("[marauder] Deno runtime initialized (standalone mode)\n");
  }
})(globalThis);
