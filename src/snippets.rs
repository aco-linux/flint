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
        });
    }
    snippets.sort_by(|a, b| a.keyword.cmp(&b.keyword));
    save(&snippets);
}

pub fn file() -> std::path::PathBuf {
    paths::config_dir().join("snippets.json")
}

fn default_snippets() -> Vec<Snippet> {
    vec![
        Snippet {
            keyword: "shrug".into(),
            title: "Shrug".into(),
            text: r"¯\_(ツ)_/¯".into(),
        },
        Snippet {
            keyword: "tableflip".into(),
            title: "Table flip".into(),
            text: "(╯°□°）╯︵ ┻━┻".into(),
        },
        Snippet {
            keyword: "date".into(),
            title: "ISO date template".into(),
            text: "2026-01-01".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::Snippet;

    #[test]
    fn item_uses_keyword_when_title_blank() {
        let snip = Snippet {
            keyword: "em".into(),
            title: String::new(),
            text: "hi".into(),
        };
        assert_eq!(snip.display_title(), "em");
        let item = snip.to_item();
        assert_eq!(item.title, "em");
    }
}
