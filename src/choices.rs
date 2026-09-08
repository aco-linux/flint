use std::collections::HashMap;

use crate::db;
use crate::usage::{self, Record};

/// Full-query launch: sits under alias (120k) and above weather (110k).
pub const CHOICE_BONUS: u32 = 115_000;
/// Prefix of a learned query (`sl` → Slack also teaches `s`).
pub const PREFIX_CHOICE_BONUS: u32 = 20_000;

const PREFIX_CAP: usize = 12;

pub type Map = HashMap<String, HashMap<String, Record>>;
/// class → query → item_id → record
pub type ContextMap = HashMap<String, Map>;

pub fn normalize(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn load() -> Map {
    db::choices_load().unwrap_or_default()
}

pub fn load_context() -> ContextMap {
    db::choice_context_load().unwrap_or_default()
}

pub fn bump(map: &mut Map, query: &str, item_id: &str) {
    if item_id.is_empty() {
        return;
    }
    let q = normalize(query);
    if q.is_empty() {
        return;
    }
    let now = usage::now_secs();
    remember(map, &q, item_id, now, true);
    for prefix in prefixes(&q) {
        remember(map, &prefix, item_id, now, false);
    }
    let _ = db::choices_record(&q, item_id, now, &prefixes(&q));
}

pub fn clear(map: &mut Map) {
    map.clear();
    let _ = db::choices_clear();
}

pub fn bump_class(map: &mut ContextMap, class: &str, query: &str, item_id: &str) {
    let class = class.trim();
    if class.is_empty() || item_id.is_empty() {
        return;
    }
    let q = normalize(query);
    if q.is_empty() {
        return;
    }
    let now = usage::now_secs();
    let row = map.entry(class.to_string()).or_default();
    remember(row, &q, item_id, now, true);
    let prefs = prefixes(&q);
    for prefix in &prefs {
        remember(row, prefix, item_id, now, false);
    }
    let _ = db::choice_context_record(class, &q, item_id, now, &prefs);
}

pub fn clear_context(map: &mut ContextMap) {
    map.clear();
}

pub fn best<'a>(map: &'a Map, query: &str) -> Option<(&'a str, Record)> {
    let q = normalize(query);
    let row = map.get(&q)?;
    row.iter()
        .max_by(|a, b| {
            a.1.last
                .cmp(&b.1.last)
                .then_with(|| a.1.count.cmp(&b.1.count))
                .then_with(|| a.0.cmp(b.0))
        })
        .map(|(id, rec)| (id.as_str(), *rec))
}

pub fn best_class<'a>(map: &'a ContextMap, class: &str, query: &str) -> Option<(&'a str, Record)> {
    let class = class.trim();
    if class.is_empty() {
        return None;
    }
    best(map.get(class)?, query)
}

/// `count == 0` means this query was only learned as a prefix of a longer one.
pub fn bonus(record: &Record, now: u64) -> u32 {
    let prefix = record.count == 0;
    decay(
        if prefix {
            PREFIX_CHOICE_BONUS
        } else {
            CHOICE_BONUS
        },
        record,
        now,
    )
}

fn decay(base: u32, record: &Record, now: u64) -> u32 {
    let recency = usage::recency_bonus(record.last, now);
    let scaled = if recency == 0 {
        base / 10
    } else {
        // Same-day (HOUR bucket 3500, rest-of-day 2200) keeps the full ladder
        // step: 2200/3500 would drop a yesterday `we` → WezTerm below weather.
        match recency {
            3_500 | 2_200 => base,
            1_200 => base.saturating_mul(8) / 10,
            500 => base.saturating_mul(6) / 10,
            _ => base / 10,
        }
    };
    scaled.saturating_add(record.count.min(10).saturating_mul(100))
}

fn remember(map: &mut Map, query: &str, item_id: &str, now: u64, full: bool) {
    let entry = map
        .entry(query.to_string())
        .or_default()
        .entry(item_id.to_string())
        .or_insert(Record { count: 0, last: 0 });
    if full {
        entry.count = entry.count.saturating_add(1);
    }
    entry.last = now;
}

fn prefixes(query: &str) -> Vec<String> {
    let chars: Vec<char> = query.chars().take(PREFIX_CAP).collect();
    (1..=chars.len())
        .map(|i| chars[..i].iter().collect::<String>())
        .filter(|prefix| prefix != query)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{PREFIX_CHOICE_BONUS, bonus, normalize, prefixes};
    use crate::usage::Record;

    #[test]
    fn normalize_collapses_space_and_case() {
        assert_eq!(normalize("  Sl  ACK "), "sl ack");
        assert_eq!(normalize("SL"), "sl");
    }

    #[test]
    fn prefixes_stop_before_the_full_query() {
        assert_eq!(prefixes("sl"), vec!["s".to_string()]);
        assert_eq!(prefixes("s"), Vec::<String>::new());
        let long = "abcdefghijklmnop";
        let got = prefixes(long);
        assert_eq!(got.first().map(String::as_str), Some("a"));
        assert_eq!(got.last().map(String::as_str), Some("abcdefghijkl"));
        assert!(!got.iter().any(|p| p == long));
    }

    #[test]
    fn context_class_lookup_then_global() {
        use super::{ContextMap, Map, best, best_class, remember};

        let mut ctx = ContextMap::new();
        let mut global = Map::new();
        let now = 1_800_000_000;
        remember(&mut global, "fox", "app:firefox", now, true);
        let row = ctx.entry("kitty".into()).or_default();
        remember(row, "fox", "app:code", now, true);
        assert_eq!(
            best_class(&ctx, "kitty", "fox").map(|(id, _)| id),
            Some("app:code")
        );
        assert!(best_class(&ctx, "other", "fox").is_none());
        assert_eq!(best(&global, "fox").map(|(id, _)| id), Some("app:firefox"));
    }

    #[test]
    fn prefix_row_is_the_small_bonus() {
        let now = 1_800_000_000;
        let prefix = Record {
            count: 0,
            last: now - 60,
        };
        let full = Record {
            count: 2,
            last: now - 60,
        };
        assert_eq!(bonus(&prefix, now), PREFIX_CHOICE_BONUS);
        assert!(bonus(&full, now) > PREFIX_CHOICE_BONUS);
    }
}
