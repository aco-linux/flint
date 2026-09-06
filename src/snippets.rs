use serde::{Deserialize, Serialize};

use crate::db;
use crate::item::{Action, Icon, Item, Kind};
use crate::placeholder::{self, Stamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub keyword: String,
    #[serde(default)]
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub increment: u32,
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
    placeholder::expand_snippet(text, clipboard, now, increment)
}

pub fn expand_keyword(keyword: &str, clipboard: &str) -> Option<String> {
    let mut snip = db::snippet_get(keyword)?;
    let now = Stamp::local();
    let (expanded, next) = expand(&snip.text, clipboard, &now, snip.increment);
    if next != snip.increment {
        snip.increment = next;
        let _ = db::snippet_set_increment(keyword, next);
    }
    Some(expanded)
}

pub fn load() -> Vec<Snippet> {
    db::snippets_load().unwrap_or_default()
}

pub fn save(snippets: &[Snippet]) {
    for snippet in snippets {
        let _ = db::snippet_upsert(snippet);
    }
}

pub fn upsert(keyword: &str, text: &str) {
    let keyword = keyword.trim();
    if keyword.is_empty() || text.is_empty() {
        return;
    }
    if let Some(mut existing) = db::snippet_get(keyword) {
        existing.text = text.to_string();
        save(&[existing]);
        return;
    }
    save(&[Snippet {
        keyword: keyword.to_string(),
        title: keyword.to_string(),
        text: text.to_string(),
        increment: 0,
    }]);
}

#[allow(dead_code)]
pub fn file() -> std::path::PathBuf {
    crate::paths::config_dir().join("snippets.json")
}

pub(crate) fn default_snippets() -> Vec<Snippet> {
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
    use super::{Snippet, expand};
    use crate::placeholder::Stamp;

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
