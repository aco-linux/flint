/// Query meaning: prefixes, aliases, and close misspellings.
/// "we" → weather, "weatr" → weather, "firfox" → firefox when that word is in the lexicon.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentKind {
    Weather,
    Time,
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

const INTENTS: &[(&str, IntentKind, usize)] = &[
    ("weather", IntentKind::Weather, 2),
    ("forecast", IntentKind::Weather, 4),
    ("temperature", IntentKind::Weather, 4),
    ("wx", IntentKind::Weather, 2),
    ("time", IntentKind::Time, 3),
    ("clock", IntentKind::Time, 3),
    ("now", IntentKind::Time, 3),
];

const LEXICON: &[&str] = &[
    "weather",
    "forecast",
    "temperature",
    "clipboard",
    "settings",
    "snippets",
    "windows",
    "firefox",
    "chrome",
    "terminal",
    "markdown",
    "document",
    "documents",
    "downloads",
    "pictures",
    "screenshot",
    "calendar",
    "calculator",
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

    for (word, kind, min_len) in INTENTS {
        if let Some(score) = match_word(&q, word, *min_len) {
            meaning.intents.push(Hit {
                kind: *kind,
                score,
                via: (*word).to_string(),
            });
        }
    }
    meaning
        .intents
        .sort_by_key(|hit| std::cmp::Reverse(hit.score));
    meaning.intents.dedup_by(|a, b| a.kind == b.kind);

    if meaning.corrected.is_none() && q.chars().count() >= 4 {
        meaning.corrected = suggest(&q, extra_lexicon);
    }
    if let Some(corrected) = meaning.corrected.clone()
        && corrected != q
    {
        meaning.expansions.push(corrected);
    }
    for hit in &meaning.intents {
        if hit.via != q && !meaning.expansions.contains(&hit.via) {
            meaning.expansions.push(hit.via.clone());
        }
    }
    meaning
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

pub fn suggest(query: &str, extra_lexicon: &[&str]) -> Option<String> {
    let q = query.trim().to_ascii_lowercase();
    if q.chars().count() < 4 {
        return None;
    }
    let mut best: Option<(u32, String)> = None;
    for word in LEXICON.iter().copied().chain(extra_lexicon.iter().copied()) {
        let word = word.trim().to_ascii_lowercase();
        if word.len() < 4 {
            continue;
        }
        let d = damerau(&q, &word);
        let allowed = if q.len() >= 6 { 2 } else { 1 };
        if d == 0 || d > allowed {
            continue;
        }
        let rank = d.saturating_mul(10) + word.len().abs_diff(q.len()) as u32;
        if best.as_ref().is_none_or(|(best_rank, _)| rank < *best_rank) {
            best = Some((rank, word));
        }
    }
    best.map(|(_, word)| word)
}

pub fn title_typo_score(query: &str, title: &str) -> Option<u32> {
    let q = query.trim().to_ascii_lowercase();
    let title = title.trim().to_ascii_lowercase();
    if q.len() < 4 || title.is_empty() {
        return None;
    }
    let first = title.split_whitespace().next().unwrap_or(&title);
    let distance = damerau(&q, first).min(damerau(&q, &title));
    if distance == 1 {
        Some(1_800)
    } else if distance == 2 && q.len() >= 6 {
        Some(700)
    } else {
        None
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
    use super::{IntentKind, damerau, resolve, suggest, title_typo_score};

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
        assert_eq!(suggest("wether", &[]).as_deref(), Some("weather"));
        assert_eq!(suggest("firefx", &["firefox"]).as_deref(), Some("firefox"));
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
        assert_eq!(damerau("abc", "abc"), 0);
        assert!(damerau("firefox", "chrome") > 2);
    }

    #[test]
    fn title_typo_recovers_app_names() {
        assert!(title_typo_score("firfox", "Firefox").is_some());
        assert!(title_typo_score("we", "Weather").is_none());
    }
}
