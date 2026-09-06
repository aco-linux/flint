use std::fs;

use serde::{Deserialize, Serialize};

use crate::item::{Action, Icon, Item, Kind};
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub keyword: String,
    #[serde(default)]
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub increment: u32,
}

/// Clock fields used by `{date}` / `{time}` / `{day}` placeholders.
#[derive(Debug, Clone)]
pub struct Stamp {
    pub date: String,
    pub time: String,
    pub day: String,
}

impl Stamp {
    pub fn local() -> Self {
        local_stamp().unwrap_or(Self {
            date: "1970-01-01".into(),
            time: "00:00".into(),
            day: "Thursday".into(),
        })
    }
}

impl Snippet {
    pub fn display_title(&self) -> String {
        if self.title.is_empty() {
            self.keyword.clone()
        } else {
            self.title.clone()
        }
    }

    pub fn to_item(&self) -> Item {
        let preview: String = self.text.chars().take(80).collect();
        Item {
            id: format!("snip:{}", self.keyword),
            title: self.display_title(),
            subtitle: if preview == self.text {
                preview
            } else {
                format!("{preview}…")
            },
            keywords: self.keyword.clone(),
            kind: Kind::Snippet,
            icon: Icon::Name("insert-text".into()),
            action: Action::Paste(self.text.clone()),
        }
    }
}

/// Replace known placeholders. Unknown `{foo}` stays intact.
/// `{increment}` uses the current counter and returns current+1 when present.
pub fn expand(text: &str, clipboard: &str, now: &Stamp, increment: u32) -> (String, u32) {
    let mut out = String::with_capacity(text.len());
    let mut next = increment;
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with('{')
            && let Some(rel) = text[i + 1..].find('}')
        {
            let key = &text[i + 1..i + 1 + rel];
            let repl = match key {
                "clipboard" => Some(clipboard),
                "date" => Some(now.date.as_str()),
                "time" => Some(now.time.as_str()),
                "datetime" => None,
                "day" => Some(now.day.as_str()),
                "increment" => None,
                "cursor" => Some(""),
                _ => None,
            };
            let owned;
            let piece = match key {
                "datetime" => {
                    owned = format!("{} {}", now.date, now.time);
                    Some(owned.as_str())
                }
                "increment" => {
                    owned = increment.to_string();
                    next = increment.saturating_add(1);
                    Some(owned.as_str())
                }
                _ => repl,
            };
            if let Some(piece) = piece {
                out.push_str(piece);
                i += key.len() + 2;
                continue;
            }
        }
        let ch = text[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, next)
}

pub fn expand_keyword(keyword: &str, clipboard: &str) -> Option<String> {
    let mut snippets = load();
    let snip = snippets.iter_mut().find(|s| s.keyword == keyword)?;
    let now = Stamp::local();
    let (expanded, next) = expand(&snip.text, clipboard, &now, snip.increment);
    if next != snip.increment {
        snip.increment = next;
        save(&snippets);
    }
    Some(expanded)
}

pub fn load() -> Vec<Snippet> {
    let path = file();
    if !path.exists() {
        let defaults = default_snippets();
        save(&defaults);
        return defaults;
    }
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(snippets: &[Snippet]) {
    paths::ensure();
    if let Ok(text) = serde_json::to_string_pretty(snippets) {
        let _ = paths::write_private(&file(), text);
    }
}

pub fn upsert(keyword: &str, text: &str) {
    let keyword = keyword.trim();
    if keyword.is_empty() || text.is_empty() {
        return;
    }
    let mut snippets = load();
    if let Some(existing) = snippets.iter_mut().find(|s| s.keyword == keyword) {
        existing.text = text.to_string();
    } else {
        snippets.push(Snippet {
            keyword: keyword.to_string(),
            title: keyword.to_string(),
            text: text.to_string(),
            increment: 0,
        });
    }
    snippets.sort_by(|a, b| a.keyword.cmp(&b.keyword));
    save(&snippets);
}

pub fn file() -> std::path::PathBuf {
    paths::config_dir().join("snippets.json")
}

fn local_stamp() -> Option<Stamp> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as libc::time_t;
    const DAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    // SAFETY: `tm` is written by localtime_r before we read it.
    unsafe {
        let mut tm = std::mem::zeroed::<libc::tm>();
        if libc::localtime_r(&ts, &mut tm).is_null() {
            return None;
        }
        let wday = tm.tm_wday.clamp(0, 6) as usize;
        Some(Stamp {
            date: format!(
                "{:04}-{:02}-{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday
            ),
            time: format!("{:02}:{:02}", tm.tm_hour, tm.tm_min),
            day: DAYS[wday].to_string(),
        })
    }
}

fn default_snippets() -> Vec<Snippet> {
    vec![
        Snippet {
            keyword: "shrug".into(),
            title: "Shrug".into(),
            text: r"¯\_(ツ)_/¯".into(),
            increment: 0,
        },
        Snippet {
            keyword: "tableflip".into(),
            title: "Table flip".into(),
            text: "(╯°□°）╯︵ ┻━┻".into(),
            increment: 0,
        },
        Snippet {
            keyword: "date".into(),
            title: "ISO date".into(),
            text: "{date}".into(),
            increment: 0,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{Snippet, Stamp, expand};

    #[test]
    fn item_uses_keyword_when_title_blank() {
        let snip = Snippet {
            keyword: "em".into(),
            title: String::new(),
            text: "hi".into(),
            increment: 0,
        };
        assert_eq!(snip.display_title(), "em");
        let item = snip.to_item();
        assert_eq!(item.title, "em");
    }

    #[test]
    fn expand_table() {
        let now = Stamp {
            date: "2026-09-06".into(),
            time: "14:05".into(),
            day: "Sunday".into(),
        };
        let cases = [
            ("see {clipboard}", "copied", 0u32, "see copied", 0u32),
            ("{date}", "", 0, "2026-09-06", 0),
            ("{time}", "", 0, "14:05", 0),
            ("{datetime}", "", 0, "2026-09-06 14:05", 0),
            ("{day}", "", 0, "Sunday", 0),
            ("ticket-{increment}", "", 7, "ticket-7", 8),
            ("keep {foo} intact", "", 0, "keep {foo} intact", 0),
            ("x{cursor}y", "", 0, "xy", 0),
        ];
        for (text, clip, inc, want, next) in cases {
            let (got, got_next) = expand(text, clip, &now, inc);
            assert_eq!((got.as_str(), got_next), (want, next), "expand {text}");
        }
        let (once, next) = expand("{increment}/{increment}", "", &now, 3);
        assert_eq!(once, "3/3");
        assert_eq!(next, 4);
    }
}
