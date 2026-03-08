# Code Review: Marauder

## Summary

Marauder is an ambitious GPU-accelerated terminal emulator with a well-structured three-layer architecture (Rust native → Deno runtime → Tauri webview). The codebase demonstrates strong architectural vision with clean separation of concerns across ~15 crates. However, several critical issues around memory safety, unbounded growth, blocking GPU operations, and disconnected code paths need attention before production readiness.

## Findings

### Critical

<!-- - **Unbounded cold-tier memory growth** (`pkg/grid/src/scrollback.rs`) — Cold tier accumulates LZ4-compressed blocks indefinitely with no eviction policy or maximum size. A long-running session will eventually exhaust memory. Add a configurable cold tier cap with LRU or oldest-first eviction. -->

<!-- - **O(N) warm tier row access** (`pkg/grid/src/scrollback.rs`) — `WarmTier::get_row()` reads the file line-by-line from the start to reach the target row index. For warm tiers approaching 1M rows, this is prohibitively slow. Add a seek index (byte offset per row or per chunk) for O(1) access. -->

<!-- - **Unbounded SixelDecoder buffer** (`pkg/parser/src/sixel.rs`) — `put()` appends to `self.data` with no size cap. A malicious or buggy terminal stream can send unbounded DCS data, causing OOM. Add a maximum data size (e.g., 64MB) and discard on overflow. -->

<!-- - **Base64 decoder silently corrupts on invalid input** (`pkg/parser/src/iterm2.rs`) — The custom base64 decoder maps invalid characters to 0 instead of returning an error. This silently produces corrupt image data. Return `None` from `parse_iterm2_image` on invalid base64. -->

<!-- - **Blocking GPU readbacks** (`pkg/compute/src/engine.rs`) — `pollster::block_on(buffer.slice(..).map_async())` blocks the calling thread waiting for GPU completion. In a frame-critical path this destroys performance. Use async readback or double-buffered results. -->

<!-- - **Eval injection risk in `deno_call_op`** (`apps/marauder/src-tauri/src/lib.rs`) — If op names or arguments are constructed from untrusted input and passed to the Deno runtime, code injection is possible. Validate/whitelist op names before execution. -->

<!-- - **`UserDataWrapper` unsoundness** (`pkg/event-bus/src/ffi.rs`) — Wrapping raw `*mut c_void` in a `Send + Sync` newtype to pass user data across threads is unsound if the pointed-to data isn't actually thread-safe. Document the safety contract or use `Arc<Mutex<>>`. -->

### High

<!-- - **ImageManager disconnected from render pipeline** (`pkg/renderer/src/images.rs`) — `ImageManager` is defined and exported but never instantiated in `Renderer` or called during frame rendering. The entire image pipeline (Card 3/4) is dead code until wired into `renderer.rs`. Complete the integration: add `image_manager` field to `Renderer`, call `build_instances()` in the render loop, create the image render pipeline. -->

<!-- - **Atlas has no eviction or multi-page strategy** (`pkg/renderer/src/atlas.rs`) — When the 1024×1024 atlas fills up, `pack_glyph()` returns `None` and glyphs silently disappear. For sessions with many Unicode characters or font changes, this will cause rendering gaps. Implement atlas page overflow (allocate additional textures) or LRU eviction of cold glyphs. -->

<!-- - **`flush_warm_to_cold` loads entire warm tier into memory** (`pkg/grid/src/scrollback.rs`) — `drain_all()` reads every warm row into a `Vec<Row>` before compressing. For a 1M-row warm tier this defeats the purpose of disk-backed storage. Stream rows in fixed-size batches (e.g., 10K rows) directly from file to cold blocks. -->

<!-- - **No `Drop`/cleanup for `ColdTier`** (`pkg/grid/src/scrollback.rs`) — Cold blocks hold compressed data in memory. If `TieredScrollback` is dropped without calling `cleanup()`, the warm tier temp file is removed but cold memory is only freed by normal drop. Document that `cleanup()` should be called for secure clearing, or implement `Drop` with `zeroize`. -->

<!-- - **Sixel color palette unbounded** (`pkg/parser/src/sixel.rs`) — Color definitions use `HashMap<u16, [u8; 3]>` with no limit. The Sixel spec allows color registers 0-65535; a stream defining all of them wastes memory. Cap at a reasonable limit (e.g., 4096 colors). -->

<!-- - **No error propagation from performer to caller** (`pkg/parser/src/performer.rs`) — Parser errors (invalid sixel, malformed iTerm2) are silently swallowed via `tracing::warn`. Consider collecting errors so callers can react (e.g., show a broken-image indicator). -->

### Medium

<!-- - **`search_scrollback_batched` re-uploads cells every call** (`pkg/compute/src/engine.rs`) — Each batch search creates a new GPU buffer, writes cells, dispatches, and reads back. For iterating over many cold blocks this is a lot of GPU buffer churn. Consider reusing a staging buffer. -->

<!-- - **Hardcoded workgroup sizes in shaders** (`resources/shaders/`) — All compute shaders use `@workgroup_size(256)`. This may not be optimal for all GPUs. Consider querying device limits and adjusting, or at minimum document the assumption. -->

<!-- - **JSON serialization at FFI boundary for cells** (`pkg/compute/src/ffi.rs`) — `compute_search_scrollback` takes cells as JSON, requiring serialization/deserialization of potentially large cell arrays. Use raw byte buffers with `GpuCell` repr(C) layout instead. -->

<!-- - **`visible_row` Cow indirection** (`pkg/grid/src/grid.rs`) — Returning `Cow<[Cell]>` from `visible_row` is correct but adds a branch on every cell access in the hot render path. Profile to ensure this doesn't regress frame times. -->

<!-- - **Temp file path predictability** (`pkg/grid/src/scrollback.rs`) — Warm tier uses `std::env::temp_dir()` + PID-based naming, which is predictable. Use `tempfile` crate or add random suffix for security. -->

<!-- - **Event bridge uses hardcoded event type array** (`apps/marauder/src-tauri/src/event_bridge.rs`) — `BRIDGE_EVENT_TYPES` is a manually maintained list. Adding a new event type requires remembering to update this array. Generate it from the `EventType` enum or use a wildcard subscription. -->

<!-- - **`get_or_insert_auto` always inserts into default atlas first** (`pkg/renderer/src/atlas.rs`) — For color emoji, it first tries the R8 atlas, fails, then tries RGBA. This wasted work happens on every new emoji glyph. Check `is_color_emoji` first to skip the R8 attempt. -->

### Low

<!-- - **Inconsistent error types across crates** — Some crates use `thiserror` enums, others use `anyhow::Result`, and FFI boundaries return error codes or null. Consider standardizing on `thiserror` for library crates. -->

<!-- - **Magic numbers in scrollback** (`pkg/grid/src/scrollback.rs`) — Hot capacity default 10000, cold block size 10000, warm max 1000000 are hardcoded constants. Extract to named constants with documentation. -->

<!-- - **Sixel HLS-to-RGB conversion unverified** (`pkg/parser/src/sixel.rs`) — The HLS conversion implementation should be validated against reference implementations. Edge cases around hue wrapping and saturation=0 may produce incorrect colors. -->

<!-- - **Image shader lacks DPI scaling** (`resources/shaders/image.wgsl`) — The image vertex shader doesn't account for display scale factor. Images will appear at wrong size on HiDPI displays. -->

<!-- - **Missing `#[cfg(test)]` on test modules** — Some test modules lack `#[cfg(test)]` annotation, meaning test-only code compiles in release builds. -->

## Strengths

- **Clean crate separation** — Each `pkg/*` crate has a focused responsibility with minimal cross-crate coupling. The dependency graph flows cleanly downward.

- **Triple export pattern** — Consistent FFI (`extern "C"`), `#[op2]`, and `deno_bindgen` exports across crates enable flexible integration modes.

- **GPU-first architecture** — Sharing `wgpu::Device` and `Queue` between renderer and compute, with zero-copy cell buffer access, is a strong design for performance.

- **Comprehensive test coverage** — 217 tests across the workspace, with particularly thorough coverage of the parser (54 tests) and grid (39 tests) crates.

- **Tiered scrollback design** — The hot/warm/cold tier concept is architecturally sound and addresses real memory constraints for long-running terminals.

- **Event bus design** — Typed pub/sub with interceptor support and async bridging to Deno/Tauri is well-suited to the multi-layer architecture.

- **Shader organization** — WGSL shaders are cleanly separated by concern (background, text, cursor, search, etc.) with consistent uniform/binding conventions.

## Recommendations

1. **Fix critical memory safety issues first** — Bound the SixelDecoder buffer, fix base64 error handling, add cold tier eviction, and address `UserDataWrapper` unsoundness.
2. **Wire ImageManager into the render pipeline** — Currently dead code; complete the integration to deliver Cards 3/4 functionality.
3. **Replace blocking GPU readbacks with async** — Use `map_async` with callback or double-buffered approach to avoid stalling the render thread.
4. **Add warm tier seek index** — Replace O(N) line scanning with byte-offset index for O(1) row access.
5. **Stream warm-to-cold flush** — Process in batches rather than loading entire warm tier into memory.
6. **Replace JSON cell serialization at FFI** — Use repr(C) byte buffers for cell data in `compute_search_scrollback`.
7. **Add atlas overflow strategy** — Multi-page atlas or LRU eviction to handle large glyph sets.
8. **Audit all FFI boundaries for injection** — Especially `deno_call_op` and any path where untrusted input reaches Deno eval.
