use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk4::gdk::prelude::DisplayExt;
use gtk4::gio;
use gtk4::glib;
use serde::{Deserialize, Serialize};

use crate::item::{Action, Icon, Item, Kind};
use crate::paths;

const MAX_ENTRIES: usize = 80;
const MAX_CHARS: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub text: String,
    pub copied_at: u64,
}

impl Entry {
    pub fn to_item(&self) -> Item {
        let preview: String = self.text.chars().take(88).collect();
        let title = self
            .text
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(64)
            .collect::<String>();
        Item {
            id: format!("clip:{}", self.id),
            title: if title.is_empty() {
                preview.clone()
            } else {
                title
            },
            subtitle: format!("{} · {}", ago(self.copied_at), preview),
            keywords: self.text.clone(),
            kind: Kind::Clipboard,
            icon: Icon::Name("edit-paste".into()),
            action: Action::Paste(self.text.clone()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    pub entries: Vec<Entry>,
}

impl Store {
    pub fn load() -> Self {
        let Ok(text) = fs::read_to_string(file()) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn ingest(&mut self, text: String) -> bool {
        let text = text.replace('\0', "");
        if text.trim().is_empty() || text.chars().count() > MAX_CHARS {
            return false;
        }
        if looks_secret(&text) {
            return false;
        }
        if self.entries.first().map(|e| e.text.as_str()) == Some(text.as_str()) {
            return false;
        }
        self.entries.retain(|e| e.text != text);
        self.entries.insert(
            0,
            Entry {
                id: hash_text(&text),
                text,
                copied_at: now_secs(),
            },
        );
        self.entries.truncate(MAX_ENTRIES);
        true
    }

    pub fn persist(&self) {
        paths::ensure();
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = paths::write_private(&file(), text);
        }
    }
}

pub fn watch(store: Rc<RefCell<Store>>) {
    if let Some(display) = gtk4::gdk::Display::default() {
        let clipboard = display.clipboard();
        clipboard.connect_changed({
            let store = store.clone();
            move |clipboard| capture(clipboard, store.clone())
        });
        capture(&clipboard, store.clone());
    }
    glib::timeout_add_seconds_local(1, move || {
        if let Some(text) = current_text() {
            let changed = store.borrow_mut().ingest(text);
            if changed {
                store.borrow().persist();
            }
        }
        glib::ControlFlow::Continue
    });
}

fn capture(clipboard: &gtk4::gdk::Clipboard, store: Rc<RefCell<Store>>) {
    clipboard.read_text_async(gio::Cancellable::NONE, move |res| {
        if let Ok(Some(text)) = res {
            let changed = store.borrow_mut().ingest(text.to_string());
            if changed {
                store.borrow().persist();
            }
        }
    });
}

pub fn looks_secret(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 8 {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "-----begin ",
        "sk_live_",
        "sk_test_",
        "ghp_",
        "github_pat_",
        "xoxp-",
        "xoxb-",
        "xoxa-",
        "xoxs-",
        "akia",
        "asana_pat",
        "password=",
        "passwd=",
        "secret=",
        "api_key=",
        "apikey=",
        "authorization: bearer ",
        "aws_secret_access_key",
        "private_key",
        "otpauth://",
    ];
    if MARKERS.iter().any(|m| lower.contains(m)) {
        return true;
    }
    if looks_openai_key(&lower) {
        return true;
    }
    if trimmed.chars().any(char::is_whitespace) {
        return false;
    }
    // High-entropy single tokens (API keys, JWTs, hex blobs).
    if trimmed.len() >= 32
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '/' | '='))
    {
        let classes = [
            trimmed.chars().any(|c| c.is_ascii_lowercase()),
            trimmed.chars().any(|c| c.is_ascii_uppercase()),
            trimmed.chars().any(|c| c.is_ascii_digit()),
        ]
        .into_iter()
        .filter(|b| *b)
        .count();
        if classes >= 2 {
            return true;
        }
    }
    false
}

fn looks_openai_key(lower: &str) -> bool {
    lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .any(|tok| tok.starts_with("sk-") && tok.len() >= 20)
}

fn file() -> std::path::PathBuf {
    paths::data_dir().join("clipboard.json")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hash_text(text: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn ago(ts: u64) -> String {
    let delta = now_secs().saturating_sub(ts);
    if delta < 45 {
        "just now".into()
    } else if delta < 90 {
        "1m ago".into()
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 5400 {
        "1h ago".into()
    } else if delta < 86400 {
        format!("{}h ago", delta / 3600)
    } else {
        format!("{}d ago", delta / 86400)
    }
}

pub fn current_text() -> Option<String> {
    let output = std::process::Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::Store;

    #[test]
    fn ingest_dedups_and_promotes() {
        let mut store = Store::default();
        assert!(store.ingest("alpha".into()));
        assert!(!store.ingest("alpha".into()));
        assert!(store.ingest("beta".into()));
        assert!(store.ingest("alpha".into()));
        assert_eq!(store.entries.len(), 2);
        assert_eq!(store.entries[0].text, "alpha");
        assert_eq!(store.entries[1].text, "beta");
    }

    #[test]
    fn ingest_skips_blank() {
        let mut store = Store::default();
        assert!(!store.ingest("   \n".into()));
        assert!(store.entries.is_empty());
    }

    #[test]
    fn ingest_skips_secrets() {
        let mut store = Store::default();
        assert!(!store.ingest("sk-abcdefghijklmnopqrstuvwxyz0123".into()));
        assert!(!store.ingest("ghp_abcdefghijklmnopqrstuvwxyzABCD".into()));
        assert!(!store.ingest("Authorization: Bearer abcdefghijklmnopqrstuvwxyz".into()));
        assert!(store.ingest("please ask-me later".into()));
        assert!(store.ingest("buy milk".into()));
        assert_eq!(store.entries.len(), 2);
    }
}
