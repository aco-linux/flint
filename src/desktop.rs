use std::fs;
use std::path::{Path, PathBuf};

use crate::item::{Action, Icon, Item, Kind};

pub fn load_apps() -> Vec<Item> {
    let mut dirs = Vec::new();
    dirs.push(PathBuf::from("/usr/share/applications"));
    dirs.push(PathBuf::from("/usr/local/share/applications"));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }
    if let Ok(data) = std::env::var("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(data).join("applications"));
    }
    if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in data_dirs.split(':') {
            dirs.push(PathBuf::from(dir).join("applications"));
        }
    }

    let mut apps = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(app) = parse_desktop(&path) else {
                continue;
            };
            if seen.insert(app.id.clone()) {
                apps.push(app);
            }
        }
    }
    apps.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    apps
}

fn parse_desktop(path: &Path) -> Option<Item> {
    let text = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut name: Option<String> = None;
    let mut comment = String::new();
    let mut generic = String::new();
    let mut keywords = String::new();
    let mut icon = Icon::None;
    let mut no_display = false;
    let mut hidden = false;
    let mut is_app = true;
    let mut try_exec: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "Type" => is_app = value == "Application",
            "Name" => name = Some(value.to_string()),
            "Comment" => comment = value.to_string(),
            "GenericName" => generic = value.to_string(),
            "Keywords" => keywords = value.replace(';', " "),
            "Icon" => {
                icon = if value.starts_with('/') {
                    Icon::Path(PathBuf::from(value))
                } else {
                    Icon::Name(value.to_string())
                };
            }
            "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
            "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
            "TryExec" => try_exec = Some(value.to_string()),
            _ => {}
        }
    }

    if !is_app || no_display || hidden {
        return None;
    }
    let name = name.filter(|n| !n.is_empty())?;
    if let Some(bin) = try_exec {
        let bin = bin.split_whitespace().next().unwrap_or(&bin);
        if !bin.is_empty() && !command_exists(bin) {
            return None;
        }
    }

    let subtitle = if !comment.is_empty() {
        comment
    } else if !generic.is_empty() {
        generic
    } else {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Application")
            .to_string()
    };

    let id = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string();

    Some(Item {
        id: format!("app:{id}"),
        title: name,
        subtitle,
        keywords,
        kind: Kind::App,
        icon,
        action: Action::LaunchDesktop {
            path: path.to_path_buf(),
        },
    })
}

fn command_exists(bin: &str) -> bool {
    if bin.contains('/') {
        return Path::new(bin).is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .any(|dir| dir.join(bin).is_file())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::command_exists;

    #[test]
    fn sh_exists() {
        assert!(command_exists("sh"));
    }
}
