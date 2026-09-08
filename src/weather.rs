use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::item::{Action, Icon, Item, Kind};

const CACHE_FOR: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub location: String,
    pub extra: String,
    pub summary: String,
}

struct Cached {
    at: Instant,
    snapshot: Snapshot,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

pub fn cached() -> Option<Snapshot> {
    let guard = CACHE.lock().ok()?;
    let cached = guard.as_ref()?;
    if cached.at.elapsed() > CACHE_FOR {
        return None;
    }
    Some(cached.snapshot.clone())
}

pub fn fetch() -> Result<Snapshot, String> {
    if let Some(hit) = cached() {
        return Ok(hit);
    }
    let output = Command::new("curl")
        .args([
            "-fsS",
            "--max-time",
            "2",
            "-A",
            "flint/0.2",
            "https://wttr.in/?format=%l|%c|%t|%C|%h|%w|%p|%o",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "curl is required for live weather".to_string())?;
    if !output.status.success() {
        return Err("Weather lookup failed".into());
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let snapshot = parse(&raw).ok_or_else(|| "Weather response was empty".to_string())?;
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cached {
            at: Instant::now(),
            snapshot: snapshot.clone(),
        });
    }
    Ok(snapshot)
}

pub fn parse(raw: &str) -> Option<Snapshot> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains("<html") {
        return None;
    }
    let parts: Vec<&str> = raw.split('|').map(str::trim).collect();
    if parts.len() < 4 {
        return None;
    }
    let location = parts[0].trim_end_matches(',').to_string();
    if location.is_empty() {
        return None;
    }
    let glyph = parts[1];
    let temp = parts[2].to_string();
    let condition = parts[3].to_string();
    let humidity = parts.get(4).copied().unwrap_or("");
    let wind = parts.get(5).copied().unwrap_or("");
    let precip = parts.get(6).copied().unwrap_or("");
    let rain = parts.get(7).copied().unwrap_or("");
    let extra = [humidity, wind, precip, rain]
        .into_iter()
        .filter(|part| !part.is_empty() && *part != "0.0mm" && *part != "0mm")
        .collect::<Vec<_>>()
        .join(" · ");
    let summary = format!("{temp} {glyph} {condition}").trim().to_string();
    Some(Snapshot {
        location,
        extra,
        summary,
    })
}

pub fn item(snapshot: Option<&Snapshot>) -> Item {
    match snapshot {
        Some(snap) => Item {
            id: "live:weather".into(),
            title: snap.summary.clone(),
            subtitle: if snap.extra.is_empty() {
                snap.location.clone()
            } else {
                format!("{} · {}", snap.location, snap.extra)
            },
            keywords: "weather forecast temperature wx climate".into(),
            kind: Kind::Weather,
            icon: Icon::Name("weather-few-clouds".into()),
            action: Action::Copy(format!("{} — {}", snap.location, snap.summary)),
        },
        None => Item {
            id: "live:weather".into(),
            title: "Weather".into(),
            subtitle: "Detecting your location…".into(),
            keywords: "weather forecast temperature wx climate".into(),
            kind: Kind::Weather,
            icon: Icon::Name("weather-few-clouds".into()),
            action: Action::Copy("Detecting weather…".into()),
        },
    }
}

pub fn time_item() -> Item {
    let now = chrono_lite();
    Item {
        id: "live:time".into(),
        title: now.clone(),
        subtitle: "Local time · Enter copies".into(),
        keywords: "time clock now date".into(),
        kind: Kind::Calc,
        icon: Icon::Name("preferences-system-time".into()),
        action: Action::Copy(now),
    }
}

fn chrono_lite() -> String {
    let output = Command::new("date")
        .args(["+%a %b %d  %H:%M"])
        .output()
        .ok();
    if let Some(output) = output
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !text.is_empty() {
            return text;
        }
    }
    "Local time".into()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_wttr_pipe_format() {
        let snap =
            parse("San Francisco, United States|☀️|+18°C|Clear|61%|↙11km/h|0.0mm|10%").unwrap();
        assert_eq!(snap.location, "San Francisco, United States");
        assert!(snap.summary.contains("18"));
        assert!(snap.summary.contains("Clear"));
        assert!(snap.extra.contains("61%"));
        assert!(snap.extra.contains("10%"));
    }

    #[test]
    fn rejects_html_and_empty() {
        assert!(parse("").is_none());
        assert!(parse("<html>nope</html>").is_none());
    }
}
