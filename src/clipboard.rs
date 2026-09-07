use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk4::gdk::prelude::DisplayExt;
use gtk4::gio;
use serde::{Deserialize, Serialize};

use crate::db;
use crate::item::{Action, Icon, Item, Kind};

pub(crate) const MAX_ENTRIES: usize = 80;
pub(crate) const MAX_UNPINNED_BYTES: usize = 512 * 1024;
const MAX_CHARS: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub text: String,
    pub copied_at: u64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub label: String,
}

impl Entry {
    pub fn display_title(&self) -> String {
        if !self.label.is_empty() {
            return self.label.clone();
        }
        let preview: String = self.text.chars().take(88).collect();
        let title = self
            .text
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(64)
            .collect::<String>();
        if title.is_empty() { preview } else { title }
    }

    pub fn to_item(&self) -> Item {
        let preview: String = self.text.chars().take(88).collect();
        let mut bits = Vec::new();
        if self.pinned {
            bits.push("Pinned".to_string());
        }
        if !self.label.is_empty() {
            bits.push(self.label.clone());
        }
        bits.push(ago(self.copied_at));
        bits.push(preview);
        Item {
            id: format!("clip:{}", self.id),
            title: self.display_title(),
            subtitle: bits.join(" · "),
            keywords: self.text.clone(),
            kind: Kind::Clipboard,
            icon: Icon::Name(if self.pinned {
                "starred".into()
            } else {
                "edit-paste".into()
            }),
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
        let Some(entries) = db::clips_load() else {
            return Self::default();
        };
        let mut store = Self { entries };
        let dropped = store.sort_and_trim();
        store.flush_dropped(dropped);
        store
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
        let id;
        if let Some(idx) = self.entries.iter().position(|e| e.text == text) {
            if idx == 0 {
                return false;
            }
            let mut entry = self.entries.remove(idx);
            entry.copied_at = now_secs();
            id = entry.id.clone();
            self.entries.insert(0, entry);
        } else {
            id = hash_text(&text);
            self.entries.insert(
                0,
                Entry {
                    id: id.clone(),
                    text,
                    copied_at: now_secs(),
                    pinned: false,
                    label: String::new(),
                },
            );
        }
        let dropped = self.sort_and_trim();
        self.flush_entry(&id);
        self.flush_dropped(dropped);
        true
    }

    pub fn pin(&mut self, id: &str) -> bool {
        {
            let Some(entry) = self.find_mut(id) else {
                return false;
            };
            entry.pinned = true;
        }
        let dropped = self.sort_and_trim();
        self.flush_entry(id);
        self.flush_dropped(dropped);
        true
    }

    pub fn unpin(&mut self, id: &str) -> bool {
        {
            let Some(entry) = self.find_mut(id) else {
                return false;
            };
            entry.pinned = false;
        }
        let dropped = self.sort_and_trim();
        self.flush_entry(id);
        self.flush_dropped(dropped);
        true
    }

    pub fn rename(&mut self, id: &str, label: &str) -> bool {
        let flushed;
        {
            let Some(entry) = self.find_mut(id) else {
                return false;
            };
            entry.label = label.trim().to_string();
            flushed = entry.id.clone();
        }
        self.flush_entry(&flushed);
        true
    }

    pub fn edit(&mut self, id: &str, text: &str) -> bool {
        let text = text.replace('\0', "");
        if text.trim().is_empty() || text.chars().count() > MAX_CHARS {
            return false;
        }
        if looks_secret(&text) {
            return false;
        }
        let Some(pos) = self.find_index(id) else {
            return false;
        };
        let old_id = self.entries[pos].id.clone();
        let new_id = hash_text(&text);
        if new_id != old_id && self.entries.iter().any(|e| e.id == new_id) {
            return false;
        }
        let entry = &mut self.entries[pos];
        entry.text = text;
        entry.id = new_id.clone();
        if new_id != old_id {
            let _ = db::clip_delete(&old_id);
        }
        let dropped = self.sort_and_trim();
        self.flush_entry(&new_id);
        self.flush_dropped(dropped);
        true
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        let id = strip_prefix(id);
        self.entries.iter().find(|e| e.id == id)
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut Entry> {
        let id = strip_prefix(id);
        self.entries.iter_mut().find(|e| e.id == id)
    }

    fn find_index(&self, id: &str) -> Option<usize> {
        let id = strip_prefix(id);
        self.entries.iter().position(|e| e.id == id)
    }

    fn sort_and_trim(&mut self) -> Vec<String> {
        sort_and_trim_entries(&mut self.entries)
    }

    fn flush_entry(&self, id: &str) {
        let id = strip_prefix(id);
        if let Some(entry) = self.entries.iter().find(|e| e.id == id) {
            let _ = db::clip_upsert(entry);
        }
    }

    fn flush_dropped(&self, dropped: Vec<String>) {
        for id in dropped {
            let _ = db::clip_delete(&id);
        }
    }

    /// Mutations write one row; kept so call sites still compile.
    pub fn persist(&self) {}
}

/// Keep every pinned clip. Unpinned clips are kept newest-first until they
/// exceed [`MAX_UNPINNED_BYTES`] of UTF-8 or [`MAX_ENTRIES`] rows.
pub(crate) fn sort_and_trim_entries(entries: &mut Vec<Entry>) -> Vec<String> {
    let before: HashSet<String> = entries.iter().map(|e| e.id.clone()).collect();
    entries.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| b.copied_at.cmp(&a.copied_at))
    });
    let mut kept = Vec::with_capacity(entries.len());
    let mut unpinned_bytes = 0usize;
    let mut unpinned_count = 0usize;
    for entry in entries.drain(..) {
        if entry.pinned {
            kept.push(entry);
            continue;
        }
        let bytes = entry.text.len();
        if unpinned_count >= MAX_ENTRIES
            || unpinned_bytes.saturating_add(bytes) > MAX_UNPINNED_BYTES
        {
            continue;
        }
        unpinned_bytes += bytes;
        unpinned_count += 1;
        kept.push(entry);
    }
    *entries = kept;
    let after: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    before
        .into_iter()
        .filter(|id| !after.contains(id.as_str()))
        .collect()
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
        "password:",
        "passwd=",
        "secret=",
        "secret:",
        "api_key=",
        "apikey=",
        "authorization: bearer ",
        "bearer ",
        "token:",
        "\"token\":",
        "'token':",
        "aws_secret_access_key",
        "private_key",
        "otpauth://",
    ];
    if MARKERS.iter().any(|m| lower.contains(m)) {
        return true;
    }
    lower
        .split(|c: char| {
            c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | '{' | '}' | '[' | ']' | ':')
        })
        .any(|tok| {
            looks_openai_key(tok)
                || is_high_entropy_token(tok)
                || (tok.starts_with("eyj") && tok.len() >= 16)
        })
}

fn is_high_entropy_token(tok: &str) -> bool {
    if tok.len() < 32 {
        return false;
    }
    if !tok
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '/' | '='))
    {
        return false;
    }
    let classes = [
        tok.chars().any(|c| c.is_ascii_lowercase()),
        tok.chars().any(|c| c.is_ascii_uppercase()),
        tok.chars().any(|c| c.is_ascii_digit()),
    ]
    .into_iter()
    .filter(|b| *b)
    .count();
    classes >= 2
}

fn looks_openai_key(lower: &str) -> bool {
    lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .any(|tok| tok.starts_with("sk-") && tok.len() >= 20)
}

pub fn strip_prefix(id: &str) -> &str {
    id.strip_prefix("clip:").unwrap_or(id)
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
    paste_text(&["--no-newline"])
}

/// Primary selection. Job 29 is this, not AT-SPI. Tries `wl-paste --primary` then `-p`.
pub fn primary_text() -> Option<String> {
    paste_text(primary_paste_args()).or_else(|| paste_text(primary_paste_args_short()))
}

/// Primary selection, then the regular clipboard. Empty both → `None`.
pub fn selection_or_clipboard() -> Option<String> {
    primary_text().or_else(current_text)
}

pub(crate) fn primary_paste_args() -> &'static [&'static str] {
    &["--primary", "--no-newline"]
}

pub(crate) fn primary_paste_args_short() -> &'static [&'static str] {
    &["-p", "--no-newline"]
}

fn paste_text(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("wl-paste")
        .args(args)
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
    use super::{Store, primary_paste_args, primary_paste_args_short};

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
        assert!(!store.ingest("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.aaaa.bbbb".into()));
        assert!(!store.ingest("password: hunter2hunter2hunter2".into()));
        assert!(!store.ingest(r#"{"token":"eyJhbGciOiJIUzI1NiJ9"}"#.into()));
        assert_eq!(store.entries.len(), 2);
    }

    #[test]
    fn pin_survives_truncate() {
        let mut store = Store::default();
        for i in 0..(super::MAX_ENTRIES + 4) {
            assert!(store.ingest(format!("clip-{i}")));
        }
        let oldest = store.entries.last().expect("entries").id.clone();
        assert!(store.pin(&oldest));
        assert!(store.get(&oldest).is_some_and(|e| e.pinned));
        for i in 0..10 {
            assert!(store.ingest(format!("fresh-{i}")));
        }
        let pinned = store.get(&oldest).expect("pinned kept");
        assert!(pinned.pinned);
        assert!(store.entries.iter().filter(|e| e.pinned).count() >= 1);
        assert!(store.entries[0].pinned);
    }

    #[test]
    fn rename_changes_title() {
        let mut store = Store::default();
        assert!(store.ingest("plain clipboard body".into()));
        let id = store.entries[0].id.clone();
        assert_eq!(store.entries[0].to_item().title, "plain clipboard body");
        assert!(store.rename(&id, "Work email"));
        assert_eq!(store.entries[0].to_item().title, "Work email");
        assert!(store.entries[0].to_item().subtitle.contains("Work email"));
    }

    #[test]
    fn edit_rejects_secret_pattern() {
        let mut store = Store::default();
        assert!(store.ingest("safe text".into()));
        let id = store.entries[0].id.clone();
        assert!(!store.edit(&id, "sk-abcdefghijklmnopqrstuvwxyz0123"));
        assert_eq!(store.entries[0].text, "safe text");
        assert!(store.edit(&id, "updated body"));
        assert_eq!(store.entries[0].text, "updated body");
        assert_ne!(store.entries[0].id, id);
    }

    #[test]
    fn old_json_without_new_fields_deserializes() {
        let raw = r#"{"entries":[{"id":"abc","text":"hello","copied_at":1}]}"#;
        let store: Store = serde_json::from_str(raw).expect("legacy json");
        assert_eq!(store.entries.len(), 1);
        assert!(!store.entries[0].pinned);
        assert!(store.entries[0].label.is_empty());
        assert_eq!(store.entries[0].text, "hello");
    }

    #[test]
    fn size_cap_drops_unpinned_keeps_pinned() {
        let mut store = Store::default();
        let big = "word ".repeat(2_000);
        assert!(store.ingest(format!("pin-me {big}")));
        let pinned_id = store.entries[0].id.clone();
        assert!(store.pin(&pinned_id));
        for i in 0..80 {
            assert!(store.ingest(format!("big {i} {big}")));
        }
        let pinned = store.get(&pinned_id).expect("pinned kept");
        assert!(pinned.pinned);
        let unpinned: Vec<_> = store.entries.iter().filter(|e| !e.pinned).collect();
        let unpinned_bytes: usize = unpinned.iter().map(|e| e.text.len()).sum();
        assert!(unpinned_bytes <= super::MAX_UNPINNED_BYTES);
        assert!(unpinned.len() <= super::MAX_ENTRIES);
        assert!(
            unpinned.len() < 80,
            "oversize unpinned clips must be dropped, kept {}",
            unpinned.len()
        );
    }

    #[test]
    fn job29_is_primary_selection_not_atspi() {
        assert_eq!(primary_paste_args(), &["--primary", "--no-newline"]);
        assert_eq!(primary_paste_args_short(), &["-p", "--no-newline"]);
        assert!(
            primary_paste_args().contains(&"--primary")
                || primary_paste_args_short().contains(&"-p")
        );
    }
}
