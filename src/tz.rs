use std::time::{SystemTime, UNIX_EPOCH};

use crate::item::{Action, Icon, Item, Kind};

struct City {
    name: &'static str,
    aliases: &'static [&'static str],
    offset_min: i32,
}

const CITIES: &[City] = &[
    City {
        name: "UTC",
        aliases: &["utc", "gmt", "zulu"],
        offset_min: 0,
    },
    City {
        name: "London",
        aliases: &["london", "uk", "britain"],
        offset_min: 0,
    },
    City {
        name: "Paris",
        aliases: &["paris"],
        offset_min: 60,
    },
    City {
        name: "Berlin",
        aliases: &["berlin"],
        offset_min: 60,
    },
    City {
        name: "New York",
        aliases: &["nyc", "ny", "new york", "newyork"],
        offset_min: -300,
    },
    City {
        name: "Toronto",
        aliases: &["toronto"],
        offset_min: -300,
    },
    City {
        name: "Chicago",
        aliases: &["chicago"],
        offset_min: -360,
    },
    City {
        name: "Denver",
        aliases: &["denver"],
        offset_min: -420,
    },
    City {
        name: "Los Angeles",
        aliases: &["la", "los angeles", "losangeles", "pst"],
        offset_min: -480,
    },
    City {
        name: "San Francisco",
        aliases: &["sf", "san francisco", "sanfrancisco"],
        offset_min: -480,
    },
    City {
        name: "Mexico City",
        aliases: &["mexico", "mexico city", "cdmx"],
        offset_min: -360,
    },
    City {
        name: "São Paulo",
        aliases: &["sao paulo", "saopaulo", "brazil"],
        offset_min: -180,
    },
    City {
        name: "Johannesburg",
        aliases: &["johannesburg", "joburg", "south africa"],
        offset_min: 120,
    },
    City {
        name: "Moscow",
        aliases: &["moscow"],
        offset_min: 180,
    },
    City {
        name: "Dubai",
        aliases: &["dubai"],
        offset_min: 240,
    },
    City {
        name: "Mumbai",
        aliases: &["mumbai", "delhi", "india", "ist"],
        offset_min: 330,
    },
    City {
        name: "Singapore",
        aliases: &["singapore"],
        offset_min: 480,
    },
    City {
        name: "Hong Kong",
        aliases: &["hong kong", "hongkong", "hk"],
        offset_min: 480,
    },
    City {
        name: "Shanghai",
        aliases: &["shanghai", "beijing", "china"],
        offset_min: 480,
    },
    City {
        name: "Tokyo",
        aliases: &["tokyo", "japan", "jst"],
        offset_min: 540,
    },
    City {
        name: "Seoul",
        aliases: &["seoul", "korea"],
        offset_min: 540,
    },
    City {
        name: "Sydney",
        aliases: &["sydney", "australia"],
        offset_min: 600,
    },
    City {
        name: "Auckland",
        aliases: &["auckland", "nz", "new zealand"],
        offset_min: 720,
    },
];

const DST_NOTE: &str = "standard offset, not DST";

pub fn items(query: &str) -> Vec<Item> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    if let Some((left, right)) = split_vs(q) {
        let Some(a) = lookup(left) else {
            return Vec::new();
        };
        let Some(b) = lookup(right) else {
            return Vec::new();
        };
        return vec![diff_item(a, b)];
    }
    if let Some(city) = parse_single(q) {
        return vec![city_item(city)];
    }
    Vec::new()
}

fn parse_single(query: &str) -> Option<&'static City> {
    let lower = query.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("time in ")
        .or_else(|| lower.strip_prefix("time at "))
        .or_else(|| lower.strip_prefix("time "))
    {
        return lookup(rest.trim());
    }
    if let Some(rest) = lower.strip_suffix(" time") {
        return lookup(rest.trim());
    }
    None
}

fn split_vs(query: &str) -> Option<(&str, &str)> {
    let lower = query.to_ascii_lowercase();
    for sep in [" vs ", " versus ", " difference "] {
        if let Some(idx) = lower.find(sep) {
            let left = query[..idx].trim();
            let right = query[idx + sep.len()..].trim();
            let left = left
                .trim_start_matches("time in ")
                .trim_start_matches("time at ")
                .trim_start_matches("time ");
            if !left.is_empty() && !right.is_empty() {
                return Some((left, right));
            }
        }
    }
    None
}

fn lookup(raw: &str) -> Option<&'static City> {
    let key = normalize(raw);
    if key.is_empty() {
        return None;
    }
    CITIES.iter().find(|city| {
        city.aliases.iter().any(|alias| normalize(alias) == key) || normalize(city.name) == key
    })
}

fn normalize(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn city_item(city: &City) -> Item {
    let clock = clock_at(city.offset_min);
    Item {
        id: format!("tz:{}", city.name),
        title: format!("{}  {clock}", city.name),
        subtitle: format!(
            "{} · {DST_NOTE} · copy time",
            format_offset(city.offset_min)
        ),
        keywords: format!("time timezone {}", city.aliases.join(" ")),
        kind: Kind::Calc,
        icon: Icon::Name("preferences-system-time".into()),
        action: Action::Copy(clock),
    }
}

fn diff_item(a: &City, b: &City) -> Item {
    let clock_a = clock_at(a.offset_min);
    let clock_b = clock_at(b.offset_min);
    let delta = a.offset_min - b.offset_min;
    let title = format!("{}  {clock_a}  ·  {}  {clock_b}", a.name, b.name);
    Item {
        id: format!("tz:{}:{}", a.name, b.name),
        title,
        subtitle: format!("{} · {DST_NOTE}", format_delta(delta)),
        keywords: format!("time timezone vs {} {}", a.name, b.name),
        kind: Kind::Calc,
        icon: Icon::Name("preferences-system-time".into()),
        action: Action::Copy(format!("{clock_a} / {clock_b}")),
    }
}

fn clock_at(offset_min: i32) -> String {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let shifted = unix + (offset_min as i64 * 60);
    let secs = ((shifted % 86_400) + 86_400) % 86_400;
    let hour = (secs / 3600) as u32;
    let min = ((secs % 3600) / 60) as u32;
    format!("{hour:02}:{min:02}")
}

fn format_offset(offset_min: i32) -> String {
    let sign = if offset_min >= 0 { '+' } else { '-' };
    let abs = offset_min.unsigned_abs();
    let h = abs / 60;
    let m = abs % 60;
    if m == 0 {
        format!("UTC{sign}{h}")
    } else {
        format!("UTC{sign}{h}:{m:02}")
    }
}

fn format_delta(delta_min: i32) -> String {
    let sign = if delta_min > 0 {
        "ahead"
    } else if delta_min < 0 {
        "behind"
    } else {
        return "same standard time".into();
    };
    let abs = delta_min.unsigned_abs();
    let h = abs / 60;
    let m = abs % 60;
    if m == 0 {
        format!("{h}h {sign}")
    } else {
        format!("{h}h {m:02}m {sign}")
    }
}

#[cfg(test)]
mod tests {
    use super::{format_offset, items, lookup};

    #[test]
    fn time_in_tokyo() {
        let rows = items("time in tokyo");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].title.starts_with("Tokyo"));
        assert!(rows[0].subtitle.contains("UTC+9"));
        assert!(rows[0].subtitle.contains("standard offset, not DST"));
        assert_eq!(items("tokyo time").len(), 1);
    }

    #[test]
    fn nyc_vs_london() {
        let rows = items("nyc vs london");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].title.contains("New York"));
        assert!(rows[0].title.contains("London"));
        assert!(rows[0].subtitle.contains("standard offset, not DST"));
        assert_eq!(items("time nyc vs london").len(), 1);
    }

    #[test]
    fn ignores_unrelated_vs() {
        assert!(items("firefox vs chrome").is_empty());
        assert!(items("time").is_empty());
        assert!(items("la").is_empty());
        assert!(lookup("notacity").is_none());
    }

    #[test]
    fn offset_formatting() {
        assert_eq!(format_offset(540), "UTC+9");
        assert_eq!(format_offset(-300), "UTC-5");
        assert_eq!(format_offset(330), "UTC+5:30");
    }
}
