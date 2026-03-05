//! deno_core #[op2] ops for the VT parser in embedded mode.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use deno_core::op2;
use deno_core::OpState;
use marauder_event_bus::lock_or_log;

use crate::actions::TerminalAction;
use crate::performer::MarauderParser;

/// Error type for parser ops.
#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[error("{0}")]
pub struct ParserOpError(String);

/// State: map of handle_id → parser instance.
type ParserMap = Arc<Mutex<HashMap<u32, MarauderParser>>>;
type NextParserId = Arc<Mutex<u32>>;

/// Map of handle → Arc<Mutex<MarauderParser>> for live-shared parsers from the runtime.
type SharedParserMap = Arc<Mutex<HashMap<u32, Arc<Mutex<MarauderParser>>>>>;

fn init_parser_state(state: &mut OpState) {
    state.put::<ParserMap>(Arc::new(Mutex::new(HashMap::new())));
    state.put::<NextParserId>(Arc::new(Mutex::new(1)));
    state.put::<SharedParserMap>(Arc::new(Mutex::new(HashMap::new())));
}

/// Inject a shared parser from the real runtime into OpState.
pub fn inject_shared_parser(state: &mut OpState, handle: u32, parser: Arc<Mutex<MarauderParser>>) {
    let shared = state.borrow::<SharedParserMap>().clone();
    lock_or_log(&shared, "parser::inject_shared").insert(handle, parser);
}

fn with_parser<R>(
    state: &mut OpState,
    handle: u32,
    f: impl FnOnce(&mut MarauderParser) -> R,
) -> Result<R, ParserOpError> {
    // Check shared parsers first (live runtime parsers)
    let shared = state.borrow::<SharedParserMap>().clone();
    let shared_map = lock_or_log(&shared, "parser::with_parser shared_map");
    if let Some(parser_arc) = shared_map.get(&handle) {
        let parser_arc = parser_arc.clone();
        drop(shared_map);
        let mut parser = lock_or_log(&parser_arc, "parser::with_parser instance");
        return Ok(f(&mut parser));
    }
    drop(shared_map);

    // Fall back to local parser map
    let map = state.borrow::<ParserMap>().clone();
    let mut map = lock_or_log(&map, "parser::with_parser local_map");
    let parser = map
        .get_mut(&handle)
        .ok_or_else(|| ParserOpError(format!("invalid parser handle: {handle}")))?;
    Ok(f(parser))
}

// ---------------------------------------------------------------------------
// Private impl helpers — plain Rust functions testable without #[op2] rewrite.
// ---------------------------------------------------------------------------

fn parser_create_impl(state: &mut OpState) -> Result<u32, ParserOpError> {
    let id_rc = state.borrow::<NextParserId>().clone();
    let mut id = lock_or_log(&id_rc, "parser::create next_id");
    let handle = *id;
    *id = id.checked_add(1).ok_or_else(|| ParserOpError("parser handle ID overflow".to_string()))?;
    drop(id);

    let map = state.borrow::<ParserMap>().clone();
    lock_or_log(&map, "parser::create insert").insert(handle, MarauderParser::new());
    Ok(handle)
}

fn parser_feed_impl(
    state: &mut OpState,
    handle: u32,
    data: &[u8],
) -> Result<Vec<TerminalAction>, ParserOpError> {
    with_parser(state, handle, |parser| {
        let mut actions = Vec::new();
        parser.feed(data, |action| actions.push(action));
        actions
    })
}

fn parser_reset_impl(state: &mut OpState, handle: u32) -> Result<(), ParserOpError> {
    with_parser(state, handle, |parser| {
        *parser = MarauderParser::new();
    })
}

fn parser_destroy_impl(state: &mut OpState, handle: u32) -> Result<(), ParserOpError> {
    // Remove from shared parsers first
    {
        let shared = state.borrow::<SharedParserMap>().clone();
        lock_or_log(&shared, "parser::destroy shared").remove(&handle);
    }
    // Then from local map
    let map = state.borrow::<ParserMap>().clone();
    lock_or_log(&map, "parser::destroy local").remove(&handle);
    Ok(())
}

// ---------------------------------------------------------------------------
// #[op2] public interface — each delegates to the corresponding _impl.
// ---------------------------------------------------------------------------

/// Create a new parser, returns handle ID.
#[op2(fast)]
#[smi]
pub fn op_parser_create(state: &mut OpState) -> Result<u32, ParserOpError> {
    parser_create_impl(state)
}

/// Feed bytes into a parser and return all resulting actions as JSON.
#[op2]
#[serde]
pub fn op_parser_feed(
    state: &mut OpState,
    #[smi] handle: u32,
    #[buffer] data: &[u8],
) -> Result<Vec<TerminalAction>, ParserOpError> {
    parser_feed_impl(state, handle, data)
}

/// Reset a parser to initial state.
#[op2(fast)]
pub fn op_parser_reset(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<(), ParserOpError> {
    parser_reset_impl(state, handle)
}

/// Destroy a parser instance.
#[op2(fast)]
pub fn op_parser_destroy(
    state: &mut OpState,
    #[smi] handle: u32,
) -> Result<(), ParserOpError> {
    parser_destroy_impl(state, handle)
}

deno_core::extension!(
    marauder_parser_ext,
    ops = [
        op_parser_create,
        op_parser_feed,
        op_parser_reset,
        op_parser_destroy,
    ],
    state = |state| init_parser_state(state),
);

/// Build the deno_core Extension for parser ops.
pub fn parser_extension() -> deno_core::Extension {
    marauder_parser_ext::init()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deno_core::OpState;

    fn make_state() -> OpState {
        let mut state = OpState::new(None);
        init_parser_state(&mut state);
        state
    }

    #[test]
    fn test_init_state() {
        let state = make_state();
        // All three state entries must be present — borrow would panic if missing.
        let _map = state.borrow::<ParserMap>();
        let _next = state.borrow::<NextParserId>();
        let _shared = state.borrow::<SharedParserMap>();

        // Initial next ID must be 1.
        let next = state.borrow::<NextParserId>().clone();
        let id = *next.lock().unwrap();
        assert_eq!(id, 1, "NextParserId should start at 1");

        // Both maps must be empty.
        assert!(
            state.borrow::<ParserMap>().lock().unwrap().is_empty(),
            "ParserMap should be empty on init"
        );
        assert!(
            state.borrow::<SharedParserMap>().lock().unwrap().is_empty(),
            "SharedParserMap should be empty on init"
        );
    }

    #[test]
    fn test_create_returns_incrementing_handles() {
        let mut state = make_state();
        let h1 = parser_create_impl(&mut state).expect("first create");
        let h2 = parser_create_impl(&mut state).expect("second create");
        assert_eq!(h1, 1, "first handle should be 1");
        assert_eq!(h2, 2, "second handle should be 2");

        // Both should be in the local map.
        let map = state.borrow::<ParserMap>().clone();
        let locked = map.lock().unwrap();
        assert!(locked.contains_key(&h1));
        assert!(locked.contains_key(&h2));
    }

    #[test]
    fn test_feed_invalid_handle() {
        let mut state = make_state();
        let result = parser_feed_impl(&mut state, 999, b"hello");
        assert!(result.is_err(), "feeding an invalid handle must return Err");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("999"),
            "error message should contain the bad handle id"
        );
    }

    #[test]
    fn test_reset_invalid_handle() {
        let mut state = make_state();
        let result = parser_reset_impl(&mut state, 42);
        assert!(result.is_err(), "resetting an invalid handle must return Err");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("42"),
            "error message should contain the bad handle id"
        );
    }

    #[test]
    fn test_destroy_removes_parser() {
        let mut state = make_state();
        let handle = parser_create_impl(&mut state).expect("create");
        parser_destroy_impl(&mut state, handle).expect("destroy");

        // After destroy, feeding should fail.
        let result = parser_feed_impl(&mut state, handle, b"hi");
        assert!(
            result.is_err(),
            "feeding a destroyed parser should return Err"
        );
    }

    #[test]
    fn test_feed_produces_actions() {
        let mut state = make_state();
        let handle = parser_create_impl(&mut state).expect("create");
        let actions = parser_feed_impl(&mut state, handle, b"hello").expect("feed");

        // "hello" is five plain ASCII printable chars → five Print actions.
        assert_eq!(actions.len(), 5, "expected 5 Print actions for \"hello\"");
        let chars: Vec<char> = actions
            .iter()
            .map(|a| match a {
                TerminalAction::Print(c) => *c,
                other => panic!("expected Print, got {:?}", other),
            })
            .collect();
        assert_eq!(chars, vec!['h', 'e', 'l', 'l', 'o']);
    }

    #[test]
    fn test_inject_shared_parser() {
        let mut state = make_state();
        let parser = Arc::new(Mutex::new(MarauderParser::new()));
        inject_shared_parser(&mut state, 100, parser);

        // Feeding through the injected handle should work.
        let actions = parser_feed_impl(&mut state, 100, b"hi").expect("feed shared");
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], TerminalAction::Print('h')));
        assert!(matches!(actions[1], TerminalAction::Print('i')));
    }

    #[test]
    fn test_destroy_shared_parser() {
        let mut state = make_state();
        let parser = Arc::new(Mutex::new(MarauderParser::new()));
        inject_shared_parser(&mut state, 200, parser);

        // Sanity-check: feeding works before destroy.
        parser_feed_impl(&mut state, 200, b"x").expect("pre-destroy feed");

        parser_destroy_impl(&mut state, 200).expect("destroy shared");

        // After destroy the handle must be gone from SharedParserMap.
        let shared = state.borrow::<SharedParserMap>().clone();
        assert!(
            !shared.lock().unwrap().contains_key(&200),
            "shared parser should be removed after destroy"
        );

        // Feeding must now fail.
        let result = parser_feed_impl(&mut state, 200, b"x");
        assert!(result.is_err(), "feeding after destroy should fail");
    }

    #[test]
    fn test_handle_overflow() {
        let mut state = make_state();
        // Force the ID counter to u32::MAX so the next increment overflows.
        {
            let next = state.borrow::<NextParserId>().clone();
            *next.lock().unwrap() = u32::MAX;
        }
        let result = parser_create_impl(&mut state);
        assert!(result.is_err(), "create at u32::MAX must return overflow Err");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("overflow"),
            "error message should mention overflow, got: {msg}"
        );
    }
}
