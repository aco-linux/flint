use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::db;

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
    db::usage_load().unwrap_or_default()
}

pub fn bump(id: &str) {
    if id.is_empty() {
        return;
    }
    let _ = db::usage_bump(id, now_secs());
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
