use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::db;
use crate::item::{Action, Icon, Item, Kind};

pub(crate) const MAX_HISTORY: usize = 50;

/// Local civil date (proleptic Gregorian). No chrono.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

impl Civil {
    pub fn today() -> Self {
        local_civil().unwrap_or(Self {
            y: 1970,
            m: 1,
            d: 1,
        })
    }

    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let mut parts = raw.split('-');
        let y: i32 = parts.next()?.parse().ok()?;
        let m: u32 = parts.next()?.parse().ok()?;
        let d: u32 = parts.next()?.parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        Some(Self { y, m, d })
    }

    pub fn add_days(self, n: i64) -> Self {
        civil_from_days(days_from_civil(self.y, self.m, self.d) + n)
    }

    fn days(self) -> i64 {
        days_from_civil(self.y, self.m, self.d)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub expr: String,
    pub result: String,
    pub at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl History {
    pub fn load() -> Self {
        Self {
            entries: db::calc_load().unwrap_or_default(),
        }
    }

    #[allow(dead_code)]
    pub fn persist(&self) {}

    pub fn push(&mut self, expr: &str, result: &str) {
        let expr = expr.trim();
        let result = result.trim();
        if expr.is_empty() || result.is_empty() {
            return;
        }
        self.entries
            .retain(|e| !(e.expr == expr && e.result == result));
        self.entries.insert(
            0,
            Entry {
                expr: expr.to_string(),
                result: result.to_string(),
                at: now_secs(),
            },
        );
        self.entries.truncate(MAX_HISTORY);
    }

    pub fn items(&self) -> Vec<Item> {
        self.entries.iter().map(history_item).collect()
    }
}

pub fn record(expr: &str, result: &str) {
    let mut history = History::default();
    history.push(expr, result);
    if let Some(entry) = history.entries.first() {
        let _ = db::calc_push(&entry.expr, &entry.result);
    }
}

/// Instant calc answers: percent, date, then evalexpr math.
pub fn answer_item(query: &str) -> Option<Item> {
    percent_item(query)
        .or_else(|| date_item(query))
        .or_else(|| math_item(query))
}

/// Root ranking only: a complete expression with an operator, unit, date, or
/// percent — not a bare number or a lone word like `1` / `firefox`.
pub fn root_item(query: &str) -> Option<Item> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    if is_bare_number(q) {
        return None;
    }
    answer_item(q)
}

fn is_bare_number(query: &str) -> bool {
    let compact: String = query.chars().filter(|c| !c.is_whitespace()).collect();
    !compact.is_empty()
        && compact
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
        && !compact.contains('+')
}

pub fn percent_item(query: &str) -> Option<Item> {
    let result = eval_percent(query)?;
    calc_item(query.trim(), &result)
}

pub fn date_item(query: &str) -> Option<Item> {
    date_item_at(query, Civil::today())
}

pub fn date_item_at(query: &str, today: Civil) -> Option<Item> {
    let result = eval_date(query, today)?;
    calc_item(query.trim(), &result)
}

pub fn eval_percent(query: &str) -> Option<String> {
    let q = collapse_ws(query).to_ascii_lowercase();
    if !q.contains('%') {
        return None;
    }
    if let Some((pct, of)) = split_ci(&q, "% of ") {
        let pct: f64 = pct.trim().parse().ok()?;
        let of: f64 = of.trim().parse().ok()?;
        return Some(format_number(of * pct / 100.0));
    }
    if let Some((pct, of)) = split_ci(&q, "% off ") {
        let pct: f64 = pct.trim().parse().ok()?;
        let of: f64 = of.trim().parse().ok()?;
        return Some(format_number(of * (1.0 - pct / 100.0)));
    }
    let compact = q.replace(' ', "");
    if let Some(idx) = compact.find('+') {
        let base: f64 = compact[..idx].parse().ok()?;
        let pct = compact[idx + 1..].strip_suffix('%')?;
        let pct: f64 = pct.parse().ok()?;
        return Some(format_number(base * (1.0 + pct / 100.0)));
    }
    if let Some(idx) = compact.find('-') {
        if idx == 0 {
            return None;
        }
        let base: f64 = compact[..idx].parse().ok()?;
        let pct = compact[idx + 1..].strip_suffix('%')?;
        let pct: f64 = pct.parse().ok()?;
        return Some(format_number(base * (1.0 - pct / 100.0)));
    }
    None
}

pub fn eval_date(query: &str, today: Civil) -> Option<String> {
    let q = collapse_ws(query).to_ascii_lowercase();
    if q == "today" {
        return Some(today.iso());
    }
    if q == "tomorrow" {
        return Some(today.add_days(1).iso());
    }
    if q == "yesterday" {
        return Some(today.add_days(-1).iso());
    }
    if let Some(rest) = q.strip_prefix("days until ") {
        let date = Civil::parse(rest)?;
        return Some((date.days() - today.days()).to_string());
    }
    if let Some(rest) = q.strip_prefix("days since ") {
        let date = Civil::parse(rest)?;
        return Some((today.days() - date.days()).to_string());
    }
    if let Some(n) = parse_days_from_now(&q) {
        return Some(today.add_days(n).iso());
    }
    if let Some(n) = parse_today_offset(&q) {
        return Some(today.add_days(n).iso());
    }
    None
}

fn math_item(query: &str) -> Option<Item> {
    let trimmed = query.trim();
    let expr = trimmed.strip_prefix('=').unwrap_or(trimmed).trim();
    if expr.is_empty() || expr.len() > 200 {
        return None;
    }
    let forced = trimmed.starts_with('=');
    if !forced && !looks_like_math(expr) {
        return None;
    }
    let value = evalexpr::eval(expr).ok()?;
    calc_item(expr, &value.to_string())
}

pub fn looks_like_math(expr: &str) -> bool {
    let has_digit = expr.chars().any(|c| c.is_ascii_digit());
    let has_op = expr.chars().any(|c| "+-*/%^()".contains(c))
        || expr.contains("sqrt")
        || expr.contains("sin")
        || expr.contains("cos")
        || expr.contains("pi");
    has_digit && has_op
}

fn calc_item(expr: &str, result: &str) -> Option<Item> {
    if expr.is_empty() || result.is_empty() {
        return None;
    }
    Some(Item {
        id: format!("calc:{expr}"),
        title: result.to_string(),
        subtitle: format!("{expr}  →  copy result"),
        keywords: format!("{expr} calculator math"),
        kind: Kind::Calc,
        icon: Icon::Name("accessories-calculator".into()),
        action: Action::Copy(result.to_string()),
    })
}

fn history_item(entry: &Entry) -> Item {
    Item {
        id: format!("calc-hist:{}", entry.at),
        title: entry.result.clone(),
        subtitle: format!("{}  ·  history", entry.expr),
        keywords: format!("{} calculator history", entry.expr),
        kind: Kind::Calc,
        icon: Icon::Name("accessories-calculator".into()),
        action: Action::Copy(entry.result.clone()),
    }
}

fn parse_days_from_now(q: &str) -> Option<i64> {
    let (n, rest) = split_leading_int(q.trim())?;
    let rest = rest.trim();
    if matches!(
        rest,
        "days from now" | "day from now" | "d from now" | "days from today" | "day from today"
    ) {
        Some(n)
    } else {
        None
    }
}

fn parse_today_offset(q: &str) -> Option<i64> {
    let compact = q.replace(' ', "");
    let rest = compact
        .strip_prefix("today")
        .or_else(|| compact.strip_prefix("now"))?;
    if rest.is_empty() {
        return None;
    }
    let sign: i64 = if rest.starts_with('+') {
        1
    } else if rest.starts_with('-') {
        -1
    } else {
        return None;
    };
    let (n, unit) = split_leading_int(&rest[1..])?;
    let unit = unit.trim();
    if unit.is_empty() || matches!(unit, "d" | "day" | "days") {
        Some(sign * n)
    } else {
        None
    }
}

fn split_leading_int(input: &str) -> Option<(i64, &str)> {
    let input = input.trim();
    let end = input
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, c)| i + c.len_utf8())?;
    if end == 0 {
        return None;
    }
    let n: i64 = input[..end].parse().ok()?;
    Some((n, &input[end..]))
}

fn split_ci<'a>(hay: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let idx = hay.find(needle)?;
    Some((&hay[..idx], &hay[idx + needle.len()..]))
}

fn collapse_ws(input: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for ch in input.trim().chars() {
        if ch.is_whitespace() {
            if !gap {
                out.push(' ');
                gap = true;
            }
        } else {
            gap = false;
            out.push(ch);
        }
    }
    out
}

fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if (value.fract() - 0.0).abs() < 1e-9 {
        format!("{value:.0}")
    } else {
        let text = format!("{value:.4}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn local_civil() -> Option<Civil> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as libc::time_t;
    // SAFETY: `tm` is written by localtime_r before we read it.
    unsafe {
        let mut tm = std::mem::zeroed::<libc::tm>();
        if libc::localtime_r(&ts, &mut tm).is_null() {
            return None;
        }
        Some(Civil {
            y: tm.tm_year + 1900,
            m: (tm.tm_mon + 1) as u32,
            d: tm.tm_mday as u32,
        })
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Howard Hinnant civil <-> days (unix epoch day 0 = 1970-01-01).
fn days_from_civil(mut y: i32, m: u32, d: u32) -> i64 {
    y -= i32::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i64::from(era) * 146097 + i64::from(doe) - 719468
}

fn civil_from_days(z: i64) -> Civil {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Civil { y, m, d }
}

#[cfg(test)]
mod tests {
    use super::{Civil, History, eval_date, eval_percent};

    #[test]
    fn percent_of_off_and_plus() {
        assert_eq!(eval_percent("20% of 80").as_deref(), Some("16"));
        assert_eq!(eval_percent("20% off 80").as_deref(), Some("64"));
        assert_eq!(eval_percent("80 + 20%").as_deref(), Some("96"));
        assert_eq!(eval_percent("80+20%").as_deref(), Some("96"));
        assert_eq!(eval_percent("80 - 20%").as_deref(), Some("64"));
        assert!(eval_percent("2+2").is_none());
    }

    #[test]
    fn date_plus_seven_and_until() {
        let today = Civil {
            y: 2026,
            m: 1,
            d: 1,
        };
        assert_eq!(
            eval_date("today + 7d", today).as_deref(),
            Some("2026-01-08")
        );
        assert_eq!(
            eval_date("today+7 days", today).as_deref(),
            Some("2026-01-08")
        );
        assert_eq!(
            eval_date("100 days from now", today).as_deref(),
            Some("2026-04-11")
        );
        assert_eq!(
            eval_date("days until 2026-12-25", today).as_deref(),
            Some("358")
        );
        assert_eq!(
            eval_date("days since 2026-01-01", today).as_deref(),
            Some("0")
        );
        assert_eq!(
            eval_date("days since 2025-12-31", today).as_deref(),
            Some("1")
        );
    }

    #[test]
    fn history_roundtrip() {
        let mut history = History::default();
        history.push("2+2", "4");
        history.push("20% of 80", "16");
        history.push("2+2", "4");
        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.entries[0].expr, "2+2");
        let json = serde_json::to_string(&history).expect("json");
        let back: History = serde_json::from_str(&json).expect("roundtrip");
        assert_eq!(back.entries[0].result, "4");
        assert_eq!(back.entries[1].expr, "20% of 80");
        history.persist();
    }

    #[test]
    fn civil_epoch_survives() {
        let epoch = Civil {
            y: 1970,
            m: 1,
            d: 1,
        };
        assert_eq!(epoch.add_days(0).iso(), "1970-01-01");
        assert_eq!(epoch.add_days(1).iso(), "1970-01-02");
        assert_eq!(
            Civil {
                y: 2026,
                m: 1,
                d: 31
            }
            .add_days(1)
            .iso(),
            "2026-02-01"
        );
    }

    #[test]
    fn root_item_skips_bare_numbers() {
        assert!(super::root_item("1").is_none());
        assert!(super::root_item("42").is_none());
        let plus = super::root_item("1+1").expect("1+1");
        assert_eq!(plus.title, "2");
        assert!(super::root_item("20% of 80").is_some());
    }
}
