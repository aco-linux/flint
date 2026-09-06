use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::db;

/// User nicknames for any result id (`app:…`, `cmd:…`, …).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(flatten)]
    map: HashMap<String, String>,
}

impl Store {
    pub fn load() -> Self {
        Self {
            map: db::aliases_load().unwrap_or_default(),
        }
    }

    pub fn persist(&self) {}

    /// Empty alias removes the nickname.
    pub fn set(&mut self, id: &str, alias: &str) {
        let id = id.trim();
        let alias = alias.trim();
        if id.is_empty() {
            return;
        }
        if alias.is_empty() {
            self.map.remove(id);
            let _ = db::alias_set(id, None);
        } else {
            self.map.insert(id.to_string(), alias.to_string());
            let _ = db::alias_set(id, Some(alias));
        }
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.map.get(id).map(String::as_str)
    }

    pub fn lookup(&self, alias: &str) -> Option<&str> {
        let needle = alias.trim();
        if needle.is_empty() {
            return None;
        }
        self.map.iter().find_map(|(id, value)| {
            if value.eq_ignore_ascii_case(needle) {
                Some(id.as_str())
            } else {
                None
            }
        })
    }

    #[cfg(test)]
    pub fn all(&self) -> &HashMap<String, String> {
        &self.map
    }
}

#[cfg(test)]
mod tests {
    use super::Store;

    #[test]
    fn set_get_lookup() {
        let mut store = Store::default();
        store.set("app:firefox.desktop", "ff");
        assert_eq!(store.get("app:firefox.desktop"), Some("ff"));
        assert_eq!(store.lookup("ff"), Some("app:firefox.desktop"));
        assert_eq!(
            store.all().get("app:firefox.desktop").map(String::as_str),
            Some("ff")
        );
        assert_eq!(store.lookup("FF"), Some("app:firefox.desktop"));
        assert_eq!(store.lookup("nope"), None);
    }

    #[test]
    fn empty_alias_removes() {
        let mut store = Store::default();
        store.set("cmd:lock", "l");
        store.set("cmd:lock", "  ");
        assert_eq!(store.get("cmd:lock"), None);
        assert_eq!(store.lookup("l"), None);
    }
}
