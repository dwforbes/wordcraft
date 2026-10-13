//! Grammar from [Harper](https://github.com/automattic/harper) (Apache-2.0): agreement, word
//! choice ("could of"), typos ("teh"), punctuation and the rest of its curated rule set.
//!
//! Spelling stays with our own checker (user dictionary, Ignore All), so Harper's spell check is
//! off. Harper's embedded tagger and chunker models were trained on corpora under
//! non-commercial share-alike terms; see `ATTRIBUTION.md`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use harper_core::linting::{LintGroup, LintKind, Linter, Suggestion};
use harper_core::spell::FstDictionary;
use harper_core::{Dialect, Document};

use crate::{Issue, IssueKind};

/// Longer paragraphs are left alone: Harper's time grows with the paragraph.
const MAX_BYTES: usize = 20_000;

/// Times Harper may be rebuilt after a panic before grammar falls back to the built-in checks.
const MAX_REBUILDS: u32 = 3;

/// Results are kept per paragraph text (the squiggles and F7 check the same paragraphs, and
/// every new layout checks them all again), up to this many bytes of text.
const CACHE_BYTES: usize = 4 << 20;

struct State {
    group: Option<LintGroup>,
    rebuilds: u32,
    cache: HashMap<String, Vec<Issue>>,
    cache_bytes: usize,
}

/// Resident memory the last start of Harper added (native; 0 when unknown).
static LOAD_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn resident() -> Option<usize> {
    #[cfg(not(target_arch = "wasm32"))]
    return memory_stats::memory_stats().map(|m| m.physical_mem);
    #[cfg(target_arch = "wasm32")]
    None
}

fn build() -> Option<LintGroup> {
    let before = resident();
    let built = std::panic::catch_unwind(|| {
        let mut g = LintGroup::new_curated(FstDictionary::curated(), Dialect::American);
        g.config.set_rule_enabled("SpellCheck", false);
        g
    });
    if built.is_err() {
        log::error!("Harper failed to start; grammar falls back to the built-in checks");
    }
    // Other threads allocate meanwhile, so this is an estimate.
    if let (Some(a), Some(b)) = (before, resident()) {
        LOAD_BYTES.store(b.saturating_sub(a), std::sync::atomic::Ordering::Relaxed);
    }
    built.ok()
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State { group: build(), rebuilds: 0, cache: HashMap::new(), cache_bytes: 0 }))
}

/// Grammar checker memory.
#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub loaded: bool,
    /// Resident memory starting it took (estimated; 0 when unknown).
    pub load_bytes: usize,
    /// Paragraphs with cached results, and the bytes of their text.
    pub cached: usize,
    pub cache_bytes: usize,
}

/// What the grammar checker holds.
pub fn usage() -> Usage {
    // Not started yet: don't start it just to report on it.
    let Some(st) = STATE.get() else { return Usage::default() };
    let st = st.lock().unwrap_or_else(|e| e.into_inner());
    Usage {
        loaded: st.group.is_some(),
        load_bytes: if st.group.is_some() { LOAD_BYTES.load(std::sync::atomic::Ordering::Relaxed) } else { 0 },
        cached: st.cache.len(),
        cache_bytes: st.cache_bytes,
    }
}

/// Forget cached results (the checker itself stays loaded).
pub fn clear_cache() {
    if let Some(st) = STATE.get() {
        let mut st = st.lock().unwrap_or_else(|e| e.into_inner());
        st.cache.clear();
        st.cache_bytes = 0;
    }
}

/// Harper's issues in a paragraph of text, as byte ranges. Empty if Harper is unavailable.
pub fn check(text: &str) -> Vec<Issue> {
    if text.trim().is_empty() || text.len() > MAX_BYTES {
        return Vec::new();
    }
    let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = st.cache.get(text) {
        return hit.clone();
    }
    if st.group.is_none() && st.rebuilds < MAX_REBUILDS {
        st.rebuilds += 1;
        st.group = build();
    }
    let Some(group) = st.group.as_mut() else { return Vec::new() };
    // A rule that panics on some input mustn't take the document down with it.
    let lints = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| group.lint(&Document::new_plain_english_curated(text))));
    let lints = match lints {
        Ok(l) => l,
        Err(_) => {
            log::warn!("Harper panicked on a {}-byte paragraph; skipped", text.len());
            // Its caches may be half-updated: rebuild it next time.
            st.group = None;
            return Vec::new();
        }
    };
    // Harper's spans count chars; ours are bytes.
    let bytes: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
    let issues: Vec<Issue> = lints
        .into_iter()
        .filter(|l| l.lint_kind != LintKind::Spelling)
        .filter_map(|l| {
            let (start, end) = (*bytes.get(l.span.start)?, *bytes.get(l.span.end)?);
            let found = text.get(start..end)?;
            if start >= end {
                return None;
            }
            let suggestions = l
                .suggestions
                .iter()
                .map(|s| match s {
                    Suggestion::ReplaceWith(c) => c.iter().collect(),
                    Suggestion::InsertAfter(c) => found.chars().chain(c.iter().copied()).collect(),
                    Suggestion::Remove => String::new(),
                })
                .collect();
            Some(Issue { start, end, kind: IssueKind::Grammar, message: l.message, suggestions, lang: crate::ProofLang::En })
        })
        .collect();
    if st.cache_bytes.saturating_add(text.len()) > CACHE_BYTES {
        st.cache.clear();
        st.cache_bytes = 0;
    }
    st.cache_bytes = st.cache_bytes.saturating_add(text.len());
    st.cache.insert(text.to_string(), issues.clone());
    issues
}
