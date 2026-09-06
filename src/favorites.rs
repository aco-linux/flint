use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths;

/// Pinned result ids, newest pin first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    ids: Vec<String>,
}

impl Store {
    pub fn load() -> Self {
        let Ok(text) = fs::read_to_string(file()) else {
            return Self::default();
        };
        if let Ok(store) = serde_json::from_str::<Store>(&text) {
            return store;
        }
        serde_json::from_str::<Vec<String>>(&text)
            .map(|ids| Self {
                ids: unique_front(ids),
            })
            .unwrap_or_default()
    }

    pub fn persist(&self) {
        paths::ensure();
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = paths::write_private(&file(), text);
        }
    }

    /// Pin if missing (front), unpin if present. Returns whether it is pinned after.
    pub fn toggle(&mut self, id: &str) -> bool {
        let id = id.trim();
        if id.is_empty() {
            return false;
        }
        if let Some(idx) = self.ids.iter().position(|existing| existing == id) {
            self.ids.remove(idx);
            false
        } else {
            self.ids.insert(0, id.to_string());
            true
        }
    }

    pub fn is_pinned(&self, id: &str) -> bool {
        self.ids.iter().any(|existing| existing == id)
    }

    pub fn all(&self) -> &[String] {
        &self.ids
    }
}

fn unique_front(ids: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for id in ids {
        if !id.is_empty() && !out.iter().any(|existing| existing == &id) {
            out.push(id);
        }
    }
    out
}

fn file() -> std::path::PathBuf {
    paths::data_dir().join("favorites.json")
}

#[cfg(test)]
mod tests {
    use super::Store;

    #[test]
    fn toggle_is_idempotent_and_unique() {
        let mut store = Store::default();
        assert!(store.toggle("app:a"));
        assert!(store.is_pinned("app:a"));
        assert_eq!(store.all(), &["app:a"]);
        assert!(!store.toggle("app:a"));
        assert!(!store.is_pinned("app:a"));
        assert!(store.all().is_empty());
        assert!(store.toggle("app:a"));
        assert!(store.toggle("app:b"));
        assert_eq!(
            store.all().iter().filter(|id| *id == "app:a").count(),
            1,
            "toggle must not duplicate an id"
        );
        assert!(!store.toggle("app:a"));
        assert_eq!(store.all(), &["app:b"]);
        assert!(store.toggle("app:a"));
        assert_eq!(store.all(), &["app:a", "app:b"]);
    }

    #[test]
    fn newest_pin_is_first() {
        let mut store = Store::default();
        store.toggle("app:a");
        store.toggle("app:b");
        store.toggle("app:c");
        assert_eq!(store.all(), &["app:c", "app:b", "app:a"]);
        store.toggle("app:b");
        assert_eq!(store.all(), &["app:c", "app:a"]);
    }
}
