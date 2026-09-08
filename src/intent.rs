/// Query meaning: prefixes, aliases, and close misspellings.
/// "we" → weather, "weatr" → weather, "firfox" → firefox when that word is in the lexicon.
/// Multi-word sentences match on tokens and short phrases: "what's the weather like today?"
/// hits Weather; "what's on my calendar today?" hits Calendar.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentKind {
    Weather,
    Time,
    Calendar,
    Email,
    Gif,
    Web,
    Ask,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub kind: IntentKind,
    pub score: u32,
    pub via: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meaning {
    pub intents: Vec<Hit>,
    pub corrected: Option<String>,
    pub expansions: Vec<String>,
}

impl Meaning {
    pub fn has(&self, kind: IntentKind) -> bool {
        self.intents.iter().any(|hit| hit.kind == kind)
    }

    pub fn tool_intent(&self) -> bool {
        self.intents.iter().any(|hit| {
            matches!(
                hit.kind,
                IntentKind::Weather
                    | IntentKind::Time
                    | IntentKind::Calendar
                    | IntentKind::Email
                    | IntentKind::Gif
            )
        })
    }
}

/// Prefix length for whole-query / first-token matching (Raycast-style `we` → weather).
const INTENTS: &[(&str, IntentKind, usize)] = &[
    ("weather", IntentKind::Weather, 2),
    ("forecast", IntentKind::Weather, 4),
    ("temperature", IntentKind::Weather, 4),
    ("wx", IntentKind::Weather, 2),
    ("rain", IntentKind::Weather, 4),
    ("raining", IntentKind::Weather, 4),
    ("sunny", IntentKind::Weather, 4),
    ("snow", IntentKind::Weather, 4),
    ("snowing", IntentKind::Weather, 4),
    ("time", IntentKind::Time, 3),
    ("clock", IntentKind::Time, 3),
    ("now", IntentKind::Time, 3),
    ("calendar", IntentKind::Calendar, 4),
    ("agenda", IntentKind::Calendar, 4),
    ("schedule", IntentKind::Calendar, 4),
    ("meetings", IntentKind::Calendar, 4),
    ("email", IntentKind::Email, 4),
    ("inbox", IntentKind::Email, 4),
    ("unread", IntentKind::Email, 4),
    ("mail", IntentKind::Email, 4),
    ("gif", IntentKind::Gif, 3),
    ("gifs", IntentKind::Gif, 3),
    ("giphy", IntentKind::Gif, 4),
    ("tenor", IntentKind::Gif, 4),
    ("search", IntentKind::Web, 5),
    ("lookup", IntentKind::Web, 5),
];

const PHRASES: &[(&str, IntentKind)] = &[
    ("on my calendar", IntentKind::Calendar),
    ("my calendar", IntentKind::Calendar),
    ("my agenda", IntentKind::Calendar),
    ("my schedule", IntentKind::Calendar),
    ("my meetings", IntentKind::Calendar),
    ("my inbox", IntentKind::Email),
    ("my email", IntentKind::Email),
    ("my mail", IntentKind::Email),
    ("going to rain", IntentKind::Weather),
    ("will it rain", IntentKind::Weather),
    ("is it raining", IntentKind::Weather),
    ("search the web", IntentKind::Web),
];

/// Tokens that must be the whole query (or a lone first token) — too noisy in sentences.
const WHOLE_ONLY: &[&str] = &["now", "wx", "we", "mail"];

/// Built-in concept words only. App and file names come from the live catalog.
const LEXICON: &[&str] = &[
    "weather",
    "forecast",
    "temperature",
    "clipboard",
    "settings",
    "snippets",
    "windows",
    "screenshot",
    "calendar",
    "calculator",
    "inbox",
    "email",
];

pub fn resolve(query: &str) -> Meaning {
    resolve_with(query, &[])
}

pub fn resolve_with(query: &str, extra_lexicon: &[&str]) -> Meaning {
    let q = query.trim().to_ascii_lowercase();
    let mut meaning = Meaning {
        intents: Vec::new(),
        corrected: None,
        expansions: Vec::new(),
    };
    if q.is_empty() || q.starts_with(['>', '$', '=', '/', '~', ';', '?']) {
        return meaning;
    }

    let tokens = tokenize(&q);
    let single = tokens.len() <= 1;

    for (word, kind, min_len) in INTENTS {
        if let Some(score) = match_intent(&q, &tokens, word, *min_len, single) {
            meaning.intents.push(Hit {
                kind: *kind,
                score,
                via: (*word).to_string(),
            });
        }
    }
    for (phrase, kind) in PHRASES {
        if q.contains(phrase) {
            meaning.intents.push(Hit {
                kind: *kind,
                score: 16_000,
                via: (*phrase).to_string(),
            });
        }
    }
    if looks_like_question(&q) {
        if !meaning.has(IntentKind::Ask) {
            meaning.intents.push(Hit {
                kind: IntentKind::Ask,
                score: 7_000,
                via: "question".into(),
            });
        }
        if !meaning.tool_intent() && !meaning.has(IntentKind::Web) {
            meaning.intents.push(Hit {
                kind: IntentKind::Web,
                score: 5_500,
                via: "question".into(),
            });
        }
    }

    meaning
        .intents
        .sort_by_key(|hit| std::cmp::Reverse(hit.score));
    meaning.intents.dedup_by(|a, b| a.kind == b.kind);

    if meaning.corrected.is_none() && q.chars().count() >= 4 && single {
        meaning.corrected =
            swap_in_lexicon(&q, extra_lexicon).or_else(|| suggest(&q, extra_lexicon));
    }
    if let Some(corrected) = meaning.corrected.clone()
        && corrected != q
    {
        meaning.expansions.push(corrected);
    }
    for hit in &meaning.intents {
        if hit.via != q && !meaning.expansions.contains(&hit.via) && !hit.via.contains(' ') {
            meaning.expansions.push(hit.via.clone());
        }
    }
    meaning
}

pub fn looks_like_question(query: &str) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return false;
    }
    if q.ends_with('?') {
        return true;
    }
    let head = q.split_whitespace().next().unwrap_or("");
    matches!(
        head,
        "what"
            | "what's"
            | "whats"
            | "who"
            | "who's"
            | "when"
            | "where's"
            | "where"
            | "why"
            | "how"
            | "is"
            | "are"
            | "will"
            | "does"
            | "do"
            | "can"
            | "could"
            | "should"
            | "would"
    )
}

/// Remainder of a GIF intent query with filler words stripped (`gif cats` → `cats`).
pub fn gif_terms(query: &str) -> String {
    let skip = [
        "gif", "gifs", "giphy", "tenor", "show", "me", "a", "an", "of", "some",
    ];
    tokenize(query)
        .into_iter()
        .filter(|tok| !skip.contains(&tok.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn match_intent(
    query: &str,
    tokens: &[String],
    word: &str,
    min_len: usize,
    single: bool,
) -> Option<u32> {
    if single {
        return match_word(query, word, min_len);
    }
    if WHOLE_ONLY.contains(&word) {
        return None;
    }
    let mut best = None;
    for token in tokens {
        if token == word {
            best = Some(best.map_or(18_000, |s: u32| s.max(18_000)));
            continue;
        }
        // Prefix of an intent word only for reasonably long tokens (`fore` → forecast).
        // Short prefixes like `we` must stay single-token so "we should meet" is quiet.
        if token.len() >= min_len.max(3) && word.starts_with(token.as_str()) {
            let remain = word.len() - token.len();
            let score = 12_000u32.saturating_sub((remain as u32) * 400);
            best = Some(best.map_or(score, |s: u32| s.max(score)));
        }
    }
    best
}

fn tokenize(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch.to_ascii_lowercase());
        } else if !buf.is_empty() {
            out.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

fn match_word(query: &str, word: &str, min_len: usize) -> Option<u32> {
    if query.len() < min_len {
        return None;
    }
    if word == query {
        return Some(20_000);
    }
    if word.starts_with(query) {
        let remain = word.len() - query.len();
        return Some(14_000u32.saturating_sub((remain as u32) * 400));
    }
    if query.starts_with(word) && word.len() >= min_len {
        return Some(9_000);
    }
    let distance = damerau(query, word);
    if distance == 1 && query.len() >= 4 {
        return Some(8_000);
    }
    if distance == 2 && query.len() >= 5 {
        return Some(4_500);
    }
    None
}

/// One adjacent transposition of `query` that is already a known word.
/// `weahter` → `weather` without walking Damerau against the whole catalog.
pub fn swap_in_lexicon(query: &str, extra_lexicon: &[&str]) -> Option<String> {
    let q = query.trim().to_ascii_lowercase();
    adjacent_swaps(&q).into_iter().find(|swapped| {
        LEXICON.iter().any(|word| *word == swapped)
            || extra_lexicon
                .iter()
                .any(|word| word.eq_ignore_ascii_case(swapped))
    })
}

pub fn adjacent_swaps(query: &str) -> Vec<String> {
    let mut chars: Vec<char> = query.chars().collect();
    if chars.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(chars.len() - 1);
    for i in 0..chars.len() - 1 {
        chars.swap(i, i + 1);
        out.push(chars.iter().collect());
        chars.swap(i, i + 1);
    }
    out
}

pub fn is_adjacent_swap(a: &str, b: &str) -> bool {
    if a.len() != b.len() || a == b {
        return false;
    }
    adjacent_swaps(a).iter().any(|swapped| swapped == b)
}

pub fn allowed_distance(query_len: usize) -> u32 {
    if query_len >= 6 {
        2
    } else if query_len >= 4 {
        1
    } else {
        0
    }
}

pub fn suggest(query: &str, extra_lexicon: &[&str]) -> Option<String> {
    closest(
        query,
        LEXICON.iter().copied().chain(extra_lexicon.iter().copied()),
    )
    .map(|(word, _)| word)
}

pub fn closest<'a, I>(query: &str, words: I) -> Option<(String, u32)>
where
    I: IntoIterator<Item = &'a str>,
{
    let q = query.trim().to_ascii_lowercase();
    let allowed = allowed_distance(q.chars().count());
    if allowed == 0 {
        return None;
    }
    let mut best: Option<(u32, String)> = None;
    for word in words {
        let word = word.trim().to_ascii_lowercase();
        if word.len() < 4 {
            continue;
        }
        let d = damerau(&q, &word);
        if d == 0 || d > allowed {
            continue;
        }
        let rank = d.saturating_mul(10) + word.len().abs_diff(q.len()) as u32;
        if best.as_ref().is_none_or(|(best_rank, _)| rank < *best_rank) {
            best = Some((rank, word));
        }
    }
    best.map(|(rank, word)| (word, rank))
}

pub fn title_typo_score(query: &str, title: &str) -> Option<u32> {
    let q = query.trim().to_ascii_lowercase();
    let title = title.trim().to_ascii_lowercase();
    if q.len() < 4 || title.is_empty() {
        return None;
    }
    let first = title.split_whitespace().next().unwrap_or(&title);
    let distance = damerau(&q, first).min(damerau(&q, &title));
    let allowed = allowed_distance(q.len());
    if distance == 0 || distance > allowed {
        return None;
    }
    if distance == 1 {
        Some(1_800)
    } else {
        Some(700)
    }
}

/// Damerau–Levenshtein (insert, delete, substitute, transpose). ASCII-oriented.
pub fn damerau(a: &str, b: &str) -> u32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m as u32;
    }
    if m == 0 {
        return n as u32;
    }
    if n.abs_diff(m) > 4 {
        return 5;
    }
    let mut prev_prev = vec![0u32; m + 1];
    let mut prev = (0..=m as u32).collect::<Vec<_>>();
    let mut curr = vec![0u32; m + 1];
    for i in 1..=n {
        curr[0] = i as u32;
        for j in 1..=m {
            let cost = u32::from(a[i - 1] != b[j - 1]);
            let del = prev[j] + 1;
            let ins = curr[j - 1] + 1;
            let sub = prev[j - 1] + cost;
            let mut best = del.min(ins).min(sub);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(prev_prev[j - 2] + 1);
            }
            curr[j] = best;
        }
        prev_prev.clone_from(&prev);
        prev.clone_from(&curr);
    }
    prev[m]
}

#[cfg(test)]
mod tests {
    use super::{
        IntentKind, LEXICON, damerau, looks_like_question, resolve, suggest, title_typo_score,
    };

    #[test]
    fn we_means_weather() {
        let meaning = resolve("we");
        assert!(
            meaning
                .intents
                .iter()
                .any(|hit| hit.kind == IntentKind::Weather)
        );
        assert!(meaning.expansions.iter().any(|word| word == "weather"));
    }

    #[test]
    fn weather_typos_still_resolve() {
        let meaning = resolve("weatr");
        assert!(
            meaning
                .intents
                .iter()
                .any(|hit| hit.kind == IntentKind::Weather)
        );
        let swapped = resolve("weahter");
        assert!(
            swapped
                .intents
                .iter()
                .any(|hit| hit.kind == IntentKind::Weather),
            "swapped letters must still mean weather, got {swapped:?}"
        );
        assert_eq!(
            super::swap_in_lexicon("weahter", &[]).as_deref(),
            Some("weather")
        );
        assert_eq!(suggest("wether", &[]).as_deref(), Some("weather"));
        assert_eq!(suggest("firefx", &["firefox"]).as_deref(), Some("firefox"));
        assert!(
            suggest("firefx", &[]).is_none(),
            "firefox is not a special case — correction needs the live catalog"
        );
        assert!(
            !LEXICON.contains(&"firefox"),
            "app names must come from the live catalog, not a hardcoded list"
        );
    }

    #[test]
    fn unrelated_short_queries_stay_quiet() {
        assert!(resolve("fi").intents.is_empty());
        assert!(resolve("firefox").intents.is_empty());
        assert!(suggest("xyzzy", &[]).is_none());
    }

    #[test]
    fn damerau_counts_transpositions() {
        assert_eq!(damerau("we", "we"), 0);
        assert_eq!(damerau("waether", "weather"), 1);
        assert_eq!(damerau("weahter", "weather"), 1);
        assert_eq!(damerau("abc", "abc"), 0);
        assert!(damerau("firefox", "chrome") > 2);
        assert!(super::is_adjacent_swap("weahter", "weather"));
        assert!(!super::is_adjacent_swap("wthr", "weather"));
    }

    #[test]
    fn title_typo_recovers_app_names() {
        assert!(title_typo_score("firfox", "Firefox").is_some());
        assert!(title_typo_score("we", "Weather").is_none());
    }

    #[test]
    fn phrases_hit_weather_calendar_and_inbox() {
        let weather = resolve("what's the weather like today");
        assert!(
            weather.has(IntentKind::Weather),
            "expected weather, got {weather:?}"
        );
        let rain = resolve("is it going to rain?");
        assert!(
            rain.has(IntentKind::Weather),
            "expected rain → weather, got {rain:?}"
        );
        let cal = resolve("what's on my calendar today?");
        assert!(
            cal.has(IntentKind::Calendar),
            "expected calendar, got {cal:?}"
        );
        let inbox = resolve("what's in my inbox");
        assert!(
            inbox.has(IntentKind::Email),
            "expected email, got {inbox:?}"
        );
    }

    #[test]
    fn now_inside_a_sentence_is_not_time() {
        let meaning = resolve("what's on my calendar now");
        assert!(meaning.has(IntentKind::Calendar));
        assert!(!meaning.has(IntentKind::Time));
        assert!(resolve("now").has(IntentKind::Time));
    }

    #[test]
    fn questions_boost_ask_without_drowning_tools() {
        let q = resolve("what's on my calendar today?");
        assert!(q.has(IntentKind::Calendar));
        assert!(q.has(IntentKind::Ask));
        assert!(
            !q.has(IntentKind::Web),
            "tool intents skip the web fallback"
        );
        assert!(looks_like_question("how do I cook pasta"));
        let pasta = resolve("how do I cook pasta");
        assert!(pasta.has(IntentKind::Ask));
        assert!(pasta.has(IntentKind::Web));
    }

    #[test]
    fn we_in_a_sentence_is_not_weather() {
        let meaning = resolve("we should meet");
        assert!(!meaning.has(IntentKind::Weather));
    }
}
