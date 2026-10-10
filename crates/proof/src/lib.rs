//! WordCraft proofing.
//!
//! - **Spelling** against the public-domain Moby word list (~160k words, the same list that
//!   drives hyphenation), with inflections (`-s`, `-es`, `-ed`, `-ing`, `-ly`, `'s`…), a user
//!   dictionary and ignore lists; suggestions by edit distance.
//! - **Grammar**: [Harper](harper)'s rule set (agreement, word choice, typos…) on top of our own
//!   checks: repeated words, `a`/`an`, capital letter after a sentence end, spaces before
//!   punctuation, doubled spaces, he/she/it with a plain verb ("it do"), informal spellings ("I
//!   dun think").
//! - **Hyphenation** ([`hyphen`]): dictionary, Liang patterns, heuristic.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod dict;
pub mod harper;
pub mod hyphen;
pub mod patterns;

use std::collections::HashSet;
use std::sync::{OnceLock, RwLock};

/// A problem found in text: byte range, kind, message and suggestions.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub start: usize,
    pub end: usize,
    pub kind: IssueKind,
    pub message: String,
    pub suggestions: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    Spelling,
    Grammar,
}

fn user_words() -> &'static RwLock<HashSet<String>> {
    static U: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
    U.get_or_init(|| RwLock::new(HashSet::new()))
}

/// Add a word to the user dictionary ("Add to Dictionary").
pub fn add_word(w: &str) {
    user_words().write().unwrap_or_else(|e| e.into_inner()).insert(w.to_lowercase());
}

/// Words in the user dictionary.
pub fn user_dictionary() -> Vec<String> {
    let mut v: Vec<String> = user_words().read().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
    v.sort();
    v
}

/// Common words the source list lacks (modern vocabulary, contractions).
const COMMON: &str = "a an the and or but if of to in on at by for with from as is am are was were be been being it its we us our you your he him his she her they them their i me my mine this that these those there here what which who whom whose when where why how all any both each few more most other some such no nor not only own same so than too very can will just should now do does did done has have had having may might must shall would could up down out over under again further then once off into onto upon about above below between through during before after until while because per via vs etc ok yes yeah oh ah hi hey bye one two three four five six seven eight nine ten first last next new old get got go goes went gone see saw seen say said make made take took come came give gave know knew think thought look want need use used find found tell told ask asked feel felt try left keep let put mean set run show turn move live play pay hear hold bring begin seem help talk start might";

const EXTRA: &[&str] = &[
    "don't",
    "doesn't",
    "didn't",
    "can't",
    "won't",
    "isn't",
    "aren't",
    "wasn't",
    "weren't",
    "shouldn't",
    "wouldn't",
    "couldn't",
    "i'm",
    "i've",
    "i'd",
    "i'll",
    "you're",
    "you've",
    "you'll",
    "you'd",
    "we're",
    "we've",
    "we'll",
    "they're",
    "they've",
    "they'll",
    "it's",
    "that's",
    "there's",
    "here's",
    "what's",
    "let's",
    "email",
    "emails",
    "online",
    "website",
    "websites",
    "internet",
    "software",
    "app",
    "apps",
    "smartphone",
    "blog",
    "login",
    "logout",
    "username",
    "wifi",
    "toolbar",
    "workflow",
    "workflows",
    "dataset",
    "datasets",
    "metadata",
    "startup",
    "startups",
    "covid",
    "selfie",
    "podcast",
    "podcasts",
    "hashtag",
    "wordcraft",
    "artcraft",
    "discord",
    "github",
    "rust",
    "ok",
    "okay",
    "unlabelled",
    "labelled",
    "colour",
    "colours",
    "favourite",
    "centre",
    "organise",
];

fn known_core(w: &str) -> bool {
    let d = dict::Dictionary::en_us();
    if d.get(w).is_some() || EXTRA.contains(&w) || COMMON.split(' ').any(|c| c == w) {
        return true;
    }
    if user_words().read().unwrap_or_else(|e| e.into_inner()).contains(w) {
        return true;
    }
    // Inflections.
    let stems: [(&str, &[&str]); 12] = [
        ("'s", &[""]),
        ("s", &[""]),
        ("es", &["", "e"]),
        ("ies", &["y"]),
        ("ed", &["", "e"]),
        ("ied", &["y"]),
        ("ing", &["", "e"]),
        ("ly", &["", "le"]),
        ("ily", &["y"]),
        ("er", &["", "e"]),
        ("est", &["", "e"]),
        ("ness", &[""]),
    ];
    for (suf, adds) in stems {
        if let Some(stem) = w.strip_suffix(suf)
            && stem.len() >= 2
        {
            for a in adds {
                let s = format!("{stem}{a}");
                if d.get(&s).is_some() || EXTRA.contains(&s.as_str()) {
                    return true;
                }
            }
            // Doubled consonant: running → run, stopped → stop.
            let b = stem.as_bytes();
            if (suf == "ing" || suf == "ed" || suf == "er" || suf == "est")
                && b.len() >= 3
                && b.last() == b.get(b.len() - 2)
                && let Some(s) = stem.get(..stem.len() - 1)
                && d.get(s).is_some()
            {
                return true;
            }
        }
    }
    // Prefixes.
    for pre in ["un", "re", "non", "pre", "over", "under", "co", "mis"] {
        if let Some(rest) = w.strip_prefix(pre)
            && rest.len() >= 3
            && d.get(rest).is_some()
        {
            return true;
        }
    }
    false
}

/// Is `word` spelled correctly? Numbers, single letters, ALL-CAPS acronyms, URLs and words with
/// digits are accepted.
pub fn is_correct(word: &str) -> bool {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
    let w = w.trim_matches('\'');
    if w.chars().count() <= 1 || w.chars().any(|c| c.is_ascii_digit()) || !w.chars().any(char::is_alphabetic) {
        return true;
    }
    if w.chars().all(|c| !c.is_lowercase()) && w.chars().count() <= 6 {
        return true;
    }
    if !w.is_ascii() && !w.chars().any(|c| c.is_ascii_alphabetic()) {
        return true; // other scripts: no dictionary
    }
    let lower = w.to_lowercase().replace('’', "'");
    if known_core(&lower) {
        return true;
    }
    // Hyphenated compounds: each part.
    if lower.contains('-') {
        return lower.split('-').all(|p| p.is_empty() || known_core(p));
    }
    false
}

/// Spelling suggestions for a misspelled word (best first, at most `max`).
pub fn suggest(word: &str, max: usize) -> Vec<String> {
    let lower = word.to_lowercase();
    let n = lower.chars().count();
    if n == 0 || n > 40 {
        return Vec::new();
    }
    let d = dict::Dictionary::en_us();
    let mut scored: Vec<(usize, i64, String)> = Vec::new();
    let target: Vec<char> = lower.chars().collect();
    for (w, _) in d.iter() {
        let wn = w.chars().count();
        if wn + 2 < n || wn > n + 2 {
            continue;
        }
        let dist = damerau(&target, w);
        if dist <= 2 {
            let same_first = w.chars().next() == target.first().copied();
            scored.push((dist, if same_first { 0 } else { 1 }, w.to_string()));
        }
    }
    scored.sort_by(|a, b| (a.0, a.1, a.2.len().abs_diff(n)).cmp(&(b.0, b.1, b.2.len().abs_diff(n))).then(a.2.cmp(&b.2)));
    let cap = word.chars().next().is_some_and(char::is_uppercase);
    let upper = word.chars().all(|c| !c.is_lowercase());
    scored
        .into_iter()
        .take(max)
        .map(|(_, _, w)| {
            if upper && n > 1 {
                w.to_uppercase()
            } else if cap {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or(w)
            } else {
                w
            }
        })
        .collect()
}

/// Optimal string alignment distance, early-exit at 3.
fn damerau(a: &[char], b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut prev2 = vec![0usize; m + 1];
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        if let Some(c) = cur.get_mut(0) {
            *c = i;
        }
        let mut row_min = i;
        for j in 1..=m {
            let (ai, bj) = (a.get(i - 1), b.get(j - 1));
            let cost = usize::from(ai != bj);
            let mut v = (prev.get(j).copied().unwrap_or(9) + 1)
                .min(cur.get(j - 1).copied().unwrap_or(9) + 1)
                .min(prev.get(j - 1).copied().unwrap_or(9) + cost);
            if i > 1 && j > 1 && a.get(i - 1) == b.get(j - 2) && a.get(i - 2) == b.get(j - 1) {
                v = v.min(prev2.get(j - 2).copied().unwrap_or(9) + 1);
            }
            if let Some(c) = cur.get_mut(j) {
                *c = v;
            }
            row_min = row_min.min(v);
        }
        if row_min > 2 {
            return 3;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev.get(m).copied().unwrap_or(9)
}

/// Words in `text`: (byte start, byte end).
pub fn words(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let is_w = |c: char| c.is_alphanumeric() || c == '\'' || c == '’' || c == '-';
    for (i, c) in text.char_indices() {
        match (start, is_w(c)) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    // Trim surrounding apostrophes/hyphens.
    out.into_iter()
        .filter_map(|(a, b)| {
            let w = text.get(a..b)?;
            let lead = w.len() - w.trim_start_matches(['\'', '’', '-']).len();
            let trail = w.len() - w.trim_end_matches(['\'', '’', '-']).len();
            (a + lead < b - trail).then_some((a + lead, b - trail))
        })
        .collect()
}

/// Spelling issues in a paragraph of text (no suggestions; ask [`suggest`] on demand).
pub fn check_spelling(text: &str) -> Vec<Issue> {
    let mut v = Vec::new();
    for (a, b) in words(text) {
        let Some(w) = text.get(a..b) else { continue };
        // Skip URLs/emails.
        let ts = text.get(..a).and_then(|t| t.rfind(char::is_whitespace)).map(|i| i + 1).unwrap_or(0);
        let te = text.get(b..).and_then(|t| t.find(char::is_whitespace)).map(|i| b + i).unwrap_or(text.len());
        let token = text.get(ts..te).unwrap_or("");
        if token.contains("://") || token.contains('@') || token.starts_with("www.") {
            continue;
        }
        if !is_correct(w) {
            v.push(Issue { start: a, end: b, kind: IssueKind::Spelling, message: "Possible spelling mistake".into(), suggestions: Vec::new() });
        }
    }
    v
}

/// Grammar issues in a paragraph of text.
pub fn check_grammar(text: &str) -> Vec<Issue> {
    let mut v = Vec::new();
    let ws = words(text);
    for pair in ws.windows(2) {
        let [(a0, b0), (a1, b1)] = pair else { continue };
        let (Some(w0), Some(w1), Some(gap)) = (text.get(*a0..*b0), text.get(*a1..*b1), text.get(*b0..*a1)) else { continue };
        if gap.trim().is_empty()
            && w0.eq_ignore_ascii_case(w1)
            && w0.chars().all(char::is_alphabetic)
            && !matches!(w0.to_lowercase().as_str(), "had" | "that" | "bye" | "so")
        {
            v.push(Issue {
                start: *a0,
                end: *b1,
                kind: IssueKind::Grammar,
                message: format!("Repeated word: \"{w1}\""),
                suggestions: vec![w0.to_string()],
            });
        }
        if gap == " " && (w0 == "a" || w0 == "A") && w1.chars().next().is_some_and(|c| "aeiouAEIOU".contains(c)) && !starts_consonant_sound(w1) {
            v.push(Issue {
                start: *a0,
                end: *b0,
                kind: IssueKind::Grammar,
                message: "Use \"an\" before a vowel sound".into(),
                suggestions: vec![if w0 == "A" { "An" } else { "an" }.into()],
            });
        }
        if gap == " "
            && (w0 == "an" || w0 == "An")
            && w1.chars().next().is_some_and(|c| c.is_alphabetic() && !"aeiouAEIOUhH".contains(c))
            && !w1.chars().all(|c| c.is_uppercase())
        {
            v.push(Issue {
                start: *a0,
                end: *b0,
                kind: IssueKind::Grammar,
                message: "Use \"a\" before a consonant sound".into(),
                suggestions: vec![if w0 == "An" { "A" } else { "a" }.into()],
            });
        }
    }
    // Space before punctuation; doubled spaces.
    for (i, _) in text.match_indices(" ,").chain(text.match_indices(" .")).chain(text.match_indices(" ;")) {
        if text.get(i + 2..).is_some_and(|r| r.starts_with('.') || r.starts_with(',')) {
            continue; // "..." or numbers like " .5"
        }
        v.push(Issue {
            start: i,
            end: i + 2,
            kind: IssueKind::Grammar,
            message: "Remove the space before the punctuation".into(),
            suggestions: vec![text.get(i + 1..i + 2).unwrap_or("").to_string()],
        });
    }
    for (i, _) in text.match_indices("  ") {
        if i > 0 && text.get(..i).is_some_and(|t| t.ends_with(' ')) {
            continue;
        }
        v.push(Issue { start: i, end: i + 2, kind: IssueKind::Grammar, message: "Extra space".into(), suggestions: vec![" ".into()] });
    }
    // Sentence start capital.
    let mut after_end = true;
    for (a, b) in &ws {
        let Some(w) = text.get(*a..*b) else { continue };
        if after_end && w.chars().next().is_some_and(char::is_lowercase) && w.chars().all(char::is_alphabetic) {
            let fixed: String = w.chars().next().map(|f| f.to_uppercase().chain(w.chars().skip(1)).collect()).unwrap_or_default();
            v.push(Issue {
                start: *a,
                end: *b,
                kind: IssueKind::Grammar,
                message: "Capitalize the first word of a sentence".into(),
                suggestions: vec![fixed],
            });
        }
        let tail = text.get(*b..).unwrap_or("");
        let next_non_space = tail.trim_start_matches(['"', '”', ')', '\'']).chars().next();
        after_end = matches!(next_non_space, Some('.' | '!' | '?')) && tail.trim_start_matches(['.', '!', '?', '"', '”', ')']).starts_with(' ');
    }
    v.extend(check_agreement(text, &ws));
    // Harper's findings, except where one of ours already covers that text.
    let ours: Vec<(usize, usize)> = v.iter().map(|i| (i.start, i.end)).collect();
    v.extend(harper::check(text).into_iter().filter(|h| !ours.iter().any(|&(a, b)| a < h.end && h.start < b)));
    v.sort_by_key(|i| i.start);
    v
}

/// Plain verbs and their third-person singular forms ("it do" → "it does").
const THIRD_PERSON: &[(&str, &str)] = &[
    ("do", "does"),
    ("have", "has"),
    ("go", "goes"),
    ("say", "says"),
    ("want", "wants"),
    ("need", "needs"),
    ("like", "likes"),
    ("know", "knows"),
    ("think", "thinks"),
    ("make", "makes"),
    ("take", "takes"),
    ("come", "comes"),
    ("get", "gets"),
    ("seem", "seems"),
    ("look", "looks"),
    ("work", "works"),
];

/// Words before "he/she/it" that take a plain verb after it ("does it work", "let it go", "saw
/// it come").
const BARE_VERB_BEFORE: &[&str] = &[
    "do",
    "does",
    "did",
    "will",
    "would",
    "can",
    "could",
    "shall",
    "should",
    "may",
    "might",
    "must",
    "to",
    "let",
    "lets",
    "make",
    "makes",
    "made",
    "help",
    "helps",
    "helped",
    "see",
    "sees",
    "saw",
    "watch",
    "watched",
    "hear",
    "heard",
    "feel",
    "felt",
    "have",
    "had",
    "not",
    "doesn't",
    "didn't",
    "won't",
    "wouldn't",
    "can't",
    "couldn't",
    "shouldn't",
    "mustn't",
    "if",
    "lest",
    "that",
    "whether",
];

/// Agreement and informal spellings the other checks miss: "it do" → "it does", "I dun think" →
/// "I don't think", "like dis." → "like this.".
fn check_agreement(text: &str, ws: &[(usize, usize)]) -> Vec<Issue> {
    let word = |i: usize| ws.get(i).and_then(|&(a, b)| text.get(a..b));
    // Only a single space between two words counts as "next to" each other.
    let adjacent = |i: usize| match (ws.get(i), ws.get(i + 1)) {
        (Some(&(_, b0)), Some(&(a1, _))) => text.get(b0..a1) == Some(" "),
        _ => false,
    };
    let mut v = Vec::new();
    for i in 0..ws.len() {
        let (Some(w), Some(&(a, b))) = (word(i), ws.get(i)) else { continue };
        let lw = w.to_lowercase();
        // he/she/it + plain verb, unless something before it calls for the plain verb.
        if matches!(lw.as_str(), "he" | "she" | "it")
            && adjacent(i)
            && !i.checked_sub(1).and_then(word).is_some_and(|p| BARE_VERB_BEFORE.contains(&p.to_lowercase().as_str()))
            && let (Some(next), Some(&(na, nb))) = (word(i + 1), ws.get(i + 1))
            && let Some((_, third)) = THIRD_PERSON.iter().find(|(plain, _)| *plain == next)
        {
            v.push(Issue {
                start: na,
                end: nb,
                kind: IssueKind::Grammar,
                message: format!("Use \"{third}\" with \"{w}\""),
                suggestions: vec![(*third).to_string()],
            });
        }
        // "I/we/you/they dun": informal "don't" (or "done").
        if lw == "dun" && i.checked_sub(1).and_then(word).is_some_and(|p| matches!(p.to_lowercase().as_str(), "i" | "we" | "you" | "they")) {
            v.push(Issue {
                start: a,
                end: b,
                kind: IssueKind::Grammar,
                message: "Informal spelling".into(),
                suggestions: vec!["don't".into(), "done".into()],
            });
        }
        // "dis" with no word after it ("like dis.") is "this"; "dis him" is the verb.
        if lw == "dis" && !adjacent(i) {
            let fixed = if w.starts_with('D') { "This" } else { "this" };
            v.push(Issue { start: a, end: b, kind: IssueKind::Grammar, message: "Informal spelling".into(), suggestions: vec![fixed.into()] });
        }
    }
    v
}

fn starts_consonant_sound(w: &str) -> bool {
    let l = w.to_lowercase();
    ["uni", "use", "usu", "eu", "one", "once", "ubiq", "uti", "ure", "uro"].iter().any(|p| l.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What grammar flags in `t`: (flagged text, first suggestion).
    fn grammar(t: &str) -> Vec<(String, String)> {
        check_grammar(t)
            .into_iter()
            .map(|i| (t.get(i.start..i.end).unwrap_or("").to_string(), i.suggestions.first().cloned().unwrap_or_default()))
            .collect()
    }

    #[test]
    fn grammar_catches_agreement_and_informal_spellings() {
        let g = grammar("I dun think it do be like it dis.");
        assert_eq!(g, [("dun".into(), "don't".into()), ("do".into(), "does".into()), ("dis".into(), "this".into())]);
    }

    #[test]
    fn grammar_from_harper() {
        assert!(grammar("He don't know.").contains(&("don't".into(), "doesn't".into())));
        assert!(grammar("I could of done it.").contains(&("could of".into(), "could have".into())));
        assert!(grammar("Sit with teh dog.").iter().any(|(w, s)| w == "teh" && s == "the"));
    }

    #[test]
    fn grammar_leaves_correct_text_alone() {
        for t in [
            "Does it work? Let it go. I saw it come. It works.",
            "They dis him at every turn.",
            "She has a dog, and he likes it.",
            "Whether it work or not, we go on.",
            "I think we should go now.",
        ] {
            assert_eq!(grammar(t), Vec::<(String, String)>::new(), "{t}");
        }
    }

    #[test]
    fn grammar_ranges_are_bytes_on_char_boundaries() {
        // Harper counts chars: multi-byte text before the issue must map to the right bytes.
        let t = "Café über alles — he don't know.";
        let i = check_grammar(t).into_iter().find(|i| t.get(i.start..i.end) == Some("don't"));
        assert!(i.is_some(), "{:?}", grammar(t));
    }

    #[test]
    fn our_checks_win_over_harper_on_the_same_text() {
        // One issue for a repeated word, not ours and Harper's.
        assert_eq!(grammar("It was the the end.").len(), 1);
    }

    #[test]
    fn dictionary_loads() {
        assert!(dict::Dictionary::en_us().len() > 100_000);
    }

    #[test]
    fn spelling() {
        for w in [
            "if",
            "we",
            "hello",
            "world",
            "running",
            "studios",
            "don't",
            "Thursday",
            "NASA",
            "2026",
            "co-operate",
            "unhappy",
            "colour",
            "it's",
            "stopped",
        ] {
            assert!(is_correct(w), "{w}");
        }
        for w in ["helo", "wrold", "sentense", "recieve"] {
            assert!(!is_correct(w), "{w}");
        }
        let s = suggest("recieve", 5);
        assert!(s.contains(&"receive".to_string()), "{s:?}");
        assert!(suggest("Wrold", 3).contains(&"World".to_string()));
    }

    #[test]
    fn spelling_issues_in_text() {
        let t = "This sentense has a mispelled word, see https://example.com/xyzq.";
        let v = check_spelling(t);
        let bad: Vec<&str> = v.iter().map(|i| &t[i.start..i.end]).collect();
        assert_eq!(bad, vec!["sentense", "mispelled"]);
    }

    #[test]
    fn grammar_issues() {
        let t = "this is is a apple , and an car.  Done";
        let v = check_grammar(t);
        let msgs: Vec<&str> = v.iter().map(|i| i.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("Repeated")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("\"an\"")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("\"a\" before")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("space before")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("Capitalize")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("Extra space")), "{msgs:?}");
        assert!(check_grammar("A university is an hour away.").is_empty(), "{:?}", check_grammar("A university is an hour away."));
    }

    #[test]
    fn user_dictionary_works() {
        assert!(!is_correct("zxqwordcrafty"));
        add_word("zxqwordcrafty");
        assert!(is_correct("zxqwordcrafty"));
    }

    #[test]
    fn hyphenation() {
        let l = hyphen::Limits::default();
        assert!(hyphen::hyphenate_word("hyphenation", &l).matches('-').count() >= 2);
    }

    proptest::proptest! {
        #[test]
        fn never_panics(s in "\\PC{0,80}") {
            let _ = check_spelling(&s);
            let _ = check_grammar(&s);
            let _ = suggest(&s, 3);
            let _ = hyphen::hyphen_points(&s, &hyphen::Limits::default());
        }
    }
}
