use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// One launch of something you used yesterday outranks 200 launches from years ago.
const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;
const WEEK: u64 = 7 * DAY;
const MONTH: u64 = 30 * DAY;

pub type Map = HashMap<String, Record>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Record {
    pub count: u32,
    /// Unix seconds. `0` means we only know the old lifetime count.
    #[serde(default)]
    pub last: u64,
}

/// Prefix bonus used to be 8_000 vs 40 per use (200×). Keep it in the same
/// ballpark as a single recent use, not two hundred of them.
pub const PREFIX_BONUS: u32 = 1_200;

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn load() -> Map {
    let Ok(text) = fs::read_to_string(path()) else {
        return HashMap::new();
    };
    if let Ok(map) = serde_json::from_str::<Map>(&text) {
        return map;
    }
    let Ok(old) = serde_json::from_str::<HashMap<String, u32>>(&text) else {
        return HashMap::new();
    };
    old.into_iter()
        .map(|(id, count)| (id, Record { count, last: 0 }))
        .collect()
}

pub fn bump(id: &str) {
    let mut map = load();
    let now = now_secs();
    let record = map.entry(id.to_string()).or_insert(Record {
        count: 0,
        last: now,
    });
    record.count = record.count.saturating_add(1);
    record.last = now;
    if let Some(parent) = path().parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&map) {
        let _ = crate::paths::write_private(&path(), text);
    }
}

pub fn score(record: Option<&Record>, now: u64) -> u32 {
    let Some(record) = record else {
        return 0;
    };
    let freq = record.count.min(80).saturating_mul(25);
    let recency = if record.last == 0 || now < record.last {
        0
    } else {
        match now - record.last {
            0..HOUR => 3_500,
            HOUR..DAY => 2_200,
            DAY..WEEK => 1_200,
            WEEK..MONTH => 500,
            _ => 0,
        }
    };
    freq.saturating_add(recency)
}

fn path() -> PathBuf {
    crate::paths::data_dir().join("usage.json")
}

#[cfg(test)]
mod tests {
    use super::{PREFIX_BONUS, Record, score};

    #[test]
    fn yesterday_beats_a_stale_high_count() {
        let now = 1_800_000_000;
        let recent = Record {
            count: 2,
            last: now - 3_600,
        };
        let ancient = Record {
            count: 200,
            last: now - 2 * 365 * 24 * 3600,
        };
        assert!(
            score(Some(&recent), now) > score(Some(&ancient), now),
            "recent {} vs ancient {}",
            score(Some(&recent), now),
            score(Some(&ancient), now)
        );
    }

    #[test]
    fn prefix_bonus_is_not_two_hundred_uses() {
        let now = 1_800_000_000;
        let one_use_today = score(
            Some(&Record {
                count: 1,
                last: now - 60,
            }),
            now,
        );
        assert!(
            PREFIX_BONUS < one_use_today.saturating_mul(2),
            "prefix {PREFIX_BONUS} should not drown a recent use ({one_use_today})"
        );
    }

    #[test]
    fn migrates_count_only_records() {
        let raw = r#"{"app:firefox.desktop":12}"#;
        let old: std::collections::HashMap<String, u32> = serde_json::from_str(raw).unwrap();
        let count = *old.get("app:firefox.desktop").unwrap();
        let migrated = Record { count, last: 0 };
        assert_eq!(migrated.count, 12);
        assert_eq!(score(Some(&migrated), 1_800_000_000), count.min(80) * 25);
    }
}
