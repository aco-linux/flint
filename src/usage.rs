use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub fn load() -> HashMap<String, u32> {
    let Ok(text) = fs::read_to_string(path()) else {
        return HashMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn bump(id: &str) {
    let mut map = load();
    *map.entry(id.to_string()).or_insert(0) += 1;
    if let Some(parent) = path().parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&map) {
        let _ = crate::paths::write_private(&path(), text);
    }
}

fn path() -> PathBuf {
    crate::paths::data_dir().join("usage.json")
}
