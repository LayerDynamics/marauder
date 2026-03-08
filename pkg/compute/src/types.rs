use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

/// GPU-side cell representation. Must match WGSL struct layout.
/// 24 bytes per cell, tightly packed.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, Serialize, Deserialize)]
pub struct GpuCell {
    /// Unicode codepoint of the character.
    pub codepoint: u32,
    /// Foreground color packed as RGBA (one byte each).
    pub fg_packed: u32,
    /// Background color packed as RGBA (one byte each).
    pub bg_packed: u32,
    /// Cell attribute flags (bold, italic, underline, etc.).
    pub flags: u32,
    /// Row index in the grid.
    pub row: u32,
    /// Column index in the grid.
    pub col: u32,
}

/// Uniform buffer for search compute shader. Must match WGSL layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SearchParams {
    pub pattern_len: u32,
    pub total_rows: u32,
    pub cols: u32,
    pub max_results: u32,
}

/// Uniform buffer for URL detection compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct UrlDetectParams {
    pub total_rows: u32,
    pub cols: u32,
    pub row_start: u32,
    pub row_end: u32,
    pub max_results: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

/// Uniform buffer for selection extraction compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SelectionParams {
    pub start_row: u32,
    pub start_col: u32,
    pub end_row: u32,
    pub end_col: u32,
    pub cols: u32,
    pub max_output: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

/// Uniform buffer for highlight compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct HighlightParams {
    pub total_rows: u32,
    pub cols: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

/// A search match result returned from the GPU.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub row: u32,
    pub col: u32,
    pub length: u32,
}

/// A detected URL position returned from the GPU.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlMatch {
    pub row: u32,
    pub start_col: u32,
    pub end_col: u32,
}

/// Semantic highlight categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HighlightCategory {
    None,
    Number,
    FilePath,
    Flag,
    Operator,
    /// Custom category from user/extension highlight rules.
    Custom(String),
}

impl HighlightCategory {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Number,
            2 => Self::FilePath,
            3 => Self::Flag,
            4 => Self::Operator,
            _ => Self::None,
        }
    }

    /// Convert to a numeric index for color table lookup.
    /// Built-in categories map to 0-4; Custom maps to 5.
    pub fn to_index(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Number => 1,
            Self::FilePath => 2,
            Self::Flag => 3,
            Self::Operator => 4,
            Self::Custom(_) => 5,
        }
    }
}

/// A highlight result for a cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightResult {
    pub row: u32,
    pub col: u32,
    pub category: HighlightCategory,
}

/// Bundled results from a single frame's compute pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComputeFrameResults {
    /// Search matches (empty if no active search pattern).
    pub search_results: Vec<SearchResult>,
    /// Detected URLs in visible rows.
    pub url_matches: Vec<UrlMatch>,
    /// Semantic highlights for visible cells.
    pub highlight_results: Vec<HighlightResult>,
}

/// A highlight rule provided by the user/extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightRule {
    pub pattern: String,
    pub category: String,
    pub color: String,
}

/// NFA state transition for GPU regex search.
/// Each state has 256 possible transitions (one per byte value, mapped to codepoint classes).
/// The GPU shader walks the NFA consuming cell codepoints.
#[derive(Debug, Clone)]
pub struct RegexNfa {
    /// Flattened transition table: transitions[state * 256 + input_class] = next_state_bitmask.
    /// Each entry is a bitmask of possible next states (Thompson NFA allows multiple).
    pub transitions: Vec<u32>,
    /// Bitmask of accept states.
    pub accept_mask: u32,
    /// Total number of NFA states (max 32 for bitmask representation).
    pub num_states: u32,
}

impl RegexNfa {
    /// Compile a simple regex pattern into an NFA.
    ///
    /// Supports: `.`, `*`, `+`, `?`, `|`, `[abc]`, `[a-z]`, `^`, `$`,
    /// `\d`, `\w`, `\s`, literal characters, and `\` escaping.
    ///
    /// Returns `None` for unsupported features (backrefs, lookahead, etc.)
    /// or patterns requiring more than 32 NFA states.
    pub fn compile(pattern: &str) -> Option<Self> {
        // Thompson NFA construction
        let chars: Vec<char> = pattern.chars().collect();
        if chars.is_empty() {
            return None;
        }

        // Simple NFA builder: state 0 = start, incrementing
        let mut transitions: Vec<Vec<u32>> = Vec::new(); // transitions[state][input] = bitmask of next states
        let mut next_state = 0u32;

        let alloc_state = |transitions: &mut Vec<Vec<u32>>, next: &mut u32| -> u32 {
            let s = *next;
            *next += 1;
            transitions.push(vec![0u32; 256]);
            s
        };

        let start = alloc_state(&mut transitions, &mut next_state);
        let _current_states: u32 = 1 << start; // conceptual: bitmask of current states

        // Build a simple sequential NFA: each char class creates a state transition
        let mut prev_state = start;
        let mut state_before_last = start; // state before the most recent element (for quantifier bypass)
        let mut i = 0;
        let mut accept_state = start;
        // When set, the next element also adds transitions from this state (for ? bypass).
        let mut also_from: Option<u32> = None;
        // Alternation: collect accept states from completed alternatives (for |).
        let mut alt_accept_states: Vec<u32> = Vec::new();

        while i < chars.len() {
            if next_state >= 32 {
                return None; // Too many states for bitmask
            }

            let c = chars[i];
            match c {
                '.' => {
                    let ns = alloc_state(&mut transitions, &mut next_state);
                    // Match any printable character (32..=126)
                    for input in 32u16..=126 {
                        transitions[prev_state as usize][input as usize] |= 1 << ns;
                    }
                    // Apply bypass from ? quantifier
                    if let Some(af) = also_from.take() {
                        for input in 32u16..=126 {
                            transitions[af as usize][input as usize] |= 1 << ns;
                        }
                    }
                    state_before_last = prev_state;
                    prev_state = ns;
                    accept_state = ns;
                }
                '\\' if i + 1 < chars.len() => {
                    i += 1;
                    let ns = alloc_state(&mut transitions, &mut next_state);
                    let sources: &[u32] = if let Some(af) = also_from.take() {
                        &[prev_state, af]
                    } else {
                        &[prev_state]
                    };
                    for &src in sources {
                        match chars[i] {
                            'd' => {
                                for input in b'0'..=b'9' {
                                    transitions[src as usize][input as usize] |= 1 << ns;
                                }
                            }
                            'w' => {
                                for input in b'a'..=b'z' { transitions[src as usize][input as usize] |= 1 << ns; }
                                for input in b'A'..=b'Z' { transitions[src as usize][input as usize] |= 1 << ns; }
                                for input in b'0'..=b'9' { transitions[src as usize][input as usize] |= 1 << ns; }
                                transitions[src as usize][b'_' as usize] |= 1 << ns;
                            }
                            's' => {
                                for &input in &[b' ', b'\t', b'\n', b'\r'] {
                                    transitions[src as usize][input as usize] |= 1 << ns;
                                }
                            }
                            other => {
                                let cp = other as u32;
                                if cp < 256 {
                                    transitions[src as usize][cp as usize] |= 1 << ns;
                                }
                            }
                        }
                    }
                    state_before_last = prev_state;
                    prev_state = ns;
                    accept_state = ns;
                }
                '[' => {
                    // Character class [abc] or [a-z]
                    let ns = alloc_state(&mut transitions, &mut next_state);
                    i += 1;
                    let negate = i < chars.len() && chars[i] == '^';
                    if negate { i += 1; }
                    let mut class_set = [false; 256];
                    let mut found_close = false;
                    while i < chars.len() && chars[i] != ']' {
                        if i + 2 < chars.len() && chars[i + 1] == '-' {
                            let lo = chars[i] as u32;
                            let hi = chars[i + 2] as u32;
                            for v in lo..=hi {
                                if v < 256 { class_set[v as usize] = true; }
                            }
                            i += 3;
                        } else {
                            let v = chars[i] as u32;
                            if v < 256 { class_set[v as usize] = true; }
                            i += 1;
                        }
                    }
                    if i < chars.len() && chars[i] == ']' {
                        found_close = true;
                    }
                    if !found_close {
                        return None; // Unterminated character class
                    }
                    if negate {
                        for (v, set) in class_set.iter_mut().enumerate() {
                            if v >= 32 && v <= 126 { *set = !*set; }
                        }
                    }
                    for (v, &set) in class_set.iter().enumerate() {
                        if set {
                            transitions[prev_state as usize][v] |= 1 << ns;
                        }
                    }
                    // Apply bypass from ? quantifier
                    if let Some(af) = also_from.take() {
                        for (v, &set) in class_set.iter().enumerate() {
                            if set {
                                transitions[af as usize][v] |= 1 << ns;
                            }
                        }
                    }
                    state_before_last = prev_state;
                    prev_state = ns;
                    accept_state = ns;
                }
                '*' => {
                    also_from = None; // quantifier supersedes any pending bypass
                    // Thompson NFA: X* = zero or more of X
                    // Merge prev_state back into state_before_last:
                    // - Redirect all transitions that targeted prev_state to target
                    //   state_before_last instead (making the quantified element a self-loop)
                    // - This gives bypass (zero occurrences) and repetition (self-loop)
                    let sbl = state_before_last;
                    let prev = prev_state;
                    if sbl != prev {
                        let prev_bit = 1u32 << prev;
                        let sbl_bit = 1u32 << sbl;
                        for state_row in transitions.iter_mut() {
                            for input in 0..256 {
                                if state_row[input] & prev_bit != 0 {
                                    state_row[input] = (state_row[input] & !prev_bit) | sbl_bit;
                                }
                            }
                        }
                    }
                    prev_state = sbl;
                    accept_state = sbl;
                }
                '+' => {
                    also_from = None; // quantifier supersedes any pending bypass
                    // Thompson NFA: X+ = one or more of X
                    // After matching X (reaching prev_state), allow matching X again
                    // by copying state_before_last's outgoing transitions onto prev_state.
                    let sbl = state_before_last as usize;
                    let prev = prev_state as usize;
                    for input in 0..256 {
                        if transitions[sbl][input] != 0 {
                            transitions[prev][input] |= transitions[sbl][input];
                        }
                    }
                }
                '?' => {
                    // Thompson NFA: X? = zero or one of X
                    // Don't merge states (that would create a self-loop like *).
                    // Instead, mark that the next element should also add transitions
                    // from state_before_last (bypass path for zero occurrences).
                    also_from = Some(state_before_last);
                    // state_before_last is also an accept state if we're at end of pattern
                    accept_state = prev_state;
                }
                '|' => {
                    // Alternation: save current alternative's accept state,
                    // reset to start so the next alternative builds from the
                    // same start state. Accept mask will include all alternatives.
                    alt_accept_states.push(accept_state);
                    also_from = None;
                    prev_state = start;
                    state_before_last = start;
                    accept_state = start;
                }
                '^' | '$' => {
                    // Anchors: skip for GPU (we match within rows)
                }
                _ => {
                    // Literal character
                    let ns = alloc_state(&mut transitions, &mut next_state);
                    let cp = c as u32;
                    if cp < 256 {
                        transitions[prev_state as usize][cp as usize] |= 1 << ns;
                        // Apply bypass from ? quantifier
                        if let Some(af) = also_from.take() {
                            transitions[af as usize][cp as usize] |= 1 << ns;
                        }
                    }
                    state_before_last = prev_state;
                    prev_state = ns;
                    accept_state = ns;
                }
            }
            i += 1;
        }

        if next_state > 32 {
            return None;
        }

        // Flatten transition table
        let mut flat = Vec::with_capacity(next_state as usize * 256);
        for state_transitions in &transitions {
            flat.extend_from_slice(state_transitions);
        }

        // Build accept mask from all alternatives and the final accept state
        let mut accept_mask = 1u32 << accept_state;
        for &alt_accept in &alt_accept_states {
            accept_mask |= 1u32 << alt_accept;
        }
        // If ? was the last quantifier, also_from's state is also an accept state
        if let Some(af) = also_from {
            accept_mask |= 1u32 << af;
        }

        Some(Self {
            transitions: flat,
            accept_mask,
            num_states: next_state,
        })
    }
}

/// Uniform buffer for regex search compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RegexSearchParams {
    pub total_rows: u32,
    pub cols: u32,
    pub num_states: u32,
    pub accept_mask: u32,
    pub max_results: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

/// Uniform buffer for diff compute shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DiffParams {
    pub rows_a: u32,
    pub rows_b: u32,
    pub cols: u32,
    pub _pad: u32,
}

/// Result of a diff operation on two grid snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub row: u32,
    pub kind: DiffKind,
}

/// Kind of change in a diff result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffKind {
    /// Row is unchanged.
    Same,
    /// Row was changed (content differs).
    Changed,
    /// Row was added (only in snapshot B).
    Added,
    /// Row was removed (only in snapshot A).
    Removed,
}

impl DiffKind {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Changed,
            2 => Self::Added,
            3 => Self::Removed,
            _ => Self::Same,
        }
    }
}

/// Pack RGBA bytes into a u32: R in MSB, A in LSB.
pub fn pack_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (r as u32) << 24 | (g as u32) << 16 | (b as u32) << 8 | (a as u32)
}

/// Default foreground color (white) as packed RGBA.
pub const DEFAULT_FG_PACKED: u32 = 0xFF_FF_FF_FF; // white, opaque
/// Default background color (black) as packed RGBA.
pub const DEFAULT_BG_PACKED: u32 = 0x00_00_00_FF; // black, opaque
