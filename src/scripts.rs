use std::fs;
use std::path::{Path, PathBuf};

use crate::item::{Action, Icon, Item, Kind};

#[derive(Debug, Clone)]
pub struct ScriptCommand {
    pub title: String,
    pub subtitle: String,
    pub path: PathBuf,
    pub mode: String,
}

impl ScriptCommand {
    pub fn to_item(&self) -> Item {
        Item {
            id: format!("script:{}", self.path.display()),
            title: self.title.clone(),
            subtitle: self.subtitle.clone(),
            keywords: format!("raycast script command {}", self.mode),
            kind: Kind::Script,
            icon: Icon::Name("utilities-terminal".into()),
            action: Action::RunScript {
                path: self.path.clone(),
            },
        }
    }
}

pub fn load_dir(root: &Path) -> Vec<ScriptCommand> {
    let mut out = Vec::new();
    if !root.exists() {
        return out;
    }
    walk(root, &mut out, 0);
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

fn walk(dir: &Path, out: &mut Vec<ScriptCommand>, depth: usize) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            walk(&path, out, depth + 1);
            continue;
        }
        if let Some(cmd) = parse_file(&path) {
            out.push(cmd);
        }
    }
}

pub fn parse_file(path: &Path) -> Option<ScriptCommand> {
    let name = path.file_name()?.to_str()?;
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let ok_ext = matches!(ext, "sh" | "py" | "js" | "ts") || name.ends_with(".sh");
    if !ok_ext {
        return None;
    }
    let raw = fs::read_to_string(path).ok()?;
    let meta = parse_meta(&raw);
    let title = meta
        .title
        .or_else(|| {
            name.rsplit_once('.')
                .map(|(stem, _)| stem.replace('-', " "))
        })
        .filter(|t| !t.is_empty())?;
    let package = meta.package.unwrap_or_default();
    let mode = meta.mode.unwrap_or_else(|| "silent".into());
    let subtitle = if package.is_empty() {
        format!("Script Command · {mode}")
    } else {
        format!("{package} · {mode}")
    };
    Some(ScriptCommand {
        title,
        subtitle,
        path: path.to_path_buf(),
        mode,
    })
}

struct Meta {
    title: Option<String>,
    package: Option<String>,
    mode: Option<String>,
}

fn parse_meta(raw: &str) -> Meta {
    let mut meta = Meta {
        title: None,
        package: None,
        mode: None,
    };
    for line in raw.lines().take(80) {
        let line = line.trim();
        if let Some(value) = meta_value(line, "@raycast.title") {
            meta.title = Some(value);
        } else if let Some(value) = meta_value(line, "@raycast.packageName") {
            meta.package = Some(value);
        } else if let Some(value) = meta_value(line, "@raycast.mode") {
            meta.mode = Some(value);
        }
    }
    meta
}

fn meta_value(line: &str, key: &str) -> Option<String> {
    let idx = line.find(key)?;
    let rest = line[idx + key.len()..].trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_meta;

    #[test]
    fn reads_raycast_script_headers() {
        let raw = r#"#!/bin/bash
# @raycast.schemaVersion 1
# @raycast.title Open Downloads
# @raycast.mode silent
# @raycast.packageName System
"#;
        let meta = parse_meta(raw);
        assert_eq!(meta.title.as_deref(), Some("Open Downloads"));
        assert_eq!(meta.mode.as_deref(), Some("silent"));
        assert_eq!(meta.package.as_deref(), Some("System"));
    }
}
