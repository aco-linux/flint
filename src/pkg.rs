use std::path::Path;
use std::process::{Command, Stdio};

use crate::item::{Action, Icon, Item, Kind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapped {
    pub manager: &'static str,
    pub package: String,
}

pub fn mapped_package(path: &Path) -> Option<Mapped> {
    let text = std::fs::read_to_string(path).ok()?;
    resolve(&parse_hint(&text)?, probe)
}

pub fn class_hint(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let keys = parse_keys(&text);
    if let Some(wm) = keys.startup_wm_class.filter(|s| !s.is_empty()) {
        return Some(wm);
    }
    if let Some(id) = keys.flatpak.filter(|s| !s.is_empty()) {
        return Some(id);
    }
    if let Some(bin) = keys.exec_basename.filter(|s| !s.is_empty() && s != "env") {
        return Some(bin);
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub flatpak: Option<String>,
    pub exec_basename: Option<String>,
}

struct Keys {
    flatpak: Option<String>,
    exec_basename: Option<String>,
    startup_wm_class: Option<String>,
}

pub fn parse_hint(text: &str) -> Option<Hint> {
    let keys = parse_keys(text);
    if keys.flatpak.is_none() && keys.exec_basename.is_none() {
        return None;
    }
    Some(Hint {
        flatpak: keys.flatpak,
        exec_basename: keys.exec_basename,
    })
}

fn parse_keys(text: &str) -> Keys {
    let mut in_entry = false;
    let mut flatpak = None;
    let mut exec = None;
    let mut startup_wm_class = None;
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
            "X-Flatpak" => {
                let id = value.trim();
                if !id.is_empty() {
                    flatpak = Some(id.to_string());
                }
            }
            "Exec" if exec.is_none() => exec = exec_basename(value),
            "StartupWMClass" => {
                let wm = value.trim();
                if !wm.is_empty() {
                    startup_wm_class = Some(wm.to_string());
                }
            }
            _ => {}
        }
    }
    Keys {
        flatpak,
        exec_basename: exec,
        startup_wm_class,
    }
}

pub fn exec_basename(exec: &str) -> Option<String> {
    let mut parts = exec.split_whitespace();
    let first = parts.next()?.trim();
    if first == "env" {
        for part in parts.by_ref() {
            if part.contains('=') {
                continue;
            }
            return basename(part);
        }
        return None;
    }
    if first.ends_with("flatpak") || first == "flatpak" {
        let mut saw_run = false;
        for part in parts {
            if part.starts_with('-') {
                continue;
            }
            if !saw_run {
                if part == "run" {
                    saw_run = true;
                }
                continue;
            }
            if part.starts_with('-') {
                continue;
            }
            let id = part.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        return None;
    }
    basename(first)
}

fn basename(cmd: &str) -> Option<String> {
    let name = Path::new(cmd).file_name()?.to_str()?.trim();
    if name.is_empty() || name.starts_with('%') {
        return None;
    }
    Some(name.to_string())
}

pub fn resolve(hint: &Hint, probe: impl Fn(&str, &str) -> bool) -> Option<Mapped> {
    if let Some(id) = hint
        .flatpak
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && let Some(package) = safe_package(id)
    {
        if probe("flatpak-user", &package) {
            return Some(Mapped {
                manager: "flatpak-user",
                package,
            });
        }
        if probe("flatpak-system", &package) {
            return Some(Mapped {
                manager: "flatpak-system",
                package,
            });
        }
        if probe("flatpak", &package) {
            return Some(Mapped {
                manager: "flatpak-user",
                package,
            });
        }
        return None;
    }
    let name = hint
        .exec_basename
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let package = safe_package(name)?;
    if probe("pacman", &package) {
        Some(Mapped {
            manager: "pacman",
            package,
        })
    } else {
        None
    }
}

pub fn safe_package(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '@'))
    {
        return None;
    }
    Some(name.to_string())
}

pub fn uninstall_command(manager: &str, package: &str) -> Option<(String, Vec<String>)> {
    let package = safe_package(package)?;
    Some(match manager {
        "flatpak-user" | "flatpak" => (
            "flatpak".into(),
            vec!["uninstall".into(), "-y".into(), "--user".into(), package],
        ),
        "flatpak-system" => (
            "flatpak".into(),
            vec!["uninstall".into(), "-y".into(), "--system".into(), package],
        ),
        "pacman" => (
            "pkexec".into(),
            vec![
                "pacman".into(),
                "-Rns".into(),
                "--noconfirm".into(),
                package,
            ],
        ),
        _ => return None,
    })
}

pub fn confirm_item(title: &str, mapped: &Mapped) -> Item {
    Item {
        id: format!("cmd:uninstall-{}-{}", mapped.manager, mapped.package),
        title: format!("Uninstall {title}"),
        subtitle: format!("{} · {} — Enter to confirm", mapped.package, mapped.manager),
        keywords: format!("uninstall remove {}", mapped.package),
        kind: Kind::Command,
        icon: Icon::Name("edit-delete".into()),
        action: Action::Uninstall {
            manager: mapped.manager.to_string(),
            package: mapped.package.clone(),
        },
    }
}

pub fn uninstall(manager: &str, package: &str) {
    let Some((program, args)) = uninstall_command(manager, package) else {
        return;
    };
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let _ = Command::new(program)
        .args(args_ref)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn probe(manager: &str, package: &str) -> bool {
    match manager {
        "flatpak-user" => flatpak_info(&["info", "--user", package]),
        "flatpak-system" => flatpak_info(&["info", "--system", package]),
        "flatpak" => flatpak_info(&["info", package]),
        "pacman" => Command::new("pacman")
            .args(["-Q", package])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .is_some_and(|s| s.success()),
        _ => false,
    }
}

fn flatpak_info(args: &[&str]) -> bool {
    Command::new("flatpak")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .is_some_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::{Hint, exec_basename, parse_hint, resolve, safe_package, uninstall_command};

    #[test]
    fn parses_x_flatpak_from_snippet() {
        let snippet = "[Desktop Entry]\nName=Firefox\nExec=/usr/bin/flatpak run org.mozilla.firefox\nX-Flatpak=org.mozilla.firefox\n";
        let hint = parse_hint(snippet).expect("hint");
        assert_eq!(hint.flatpak.as_deref(), Some("org.mozilla.firefox"));
    }

    #[test]
    fn exec_basename_table() {
        let cases = [
            ("/usr/bin/firefox %u", Some("firefox")),
            ("env FOO=1 /usr/bin/kitty", Some("kitty")),
            (
                "flatpak run org.mozilla.firefox",
                Some("org.mozilla.firefox"),
            ),
            ("", None),
        ];
        for (exec, want) in cases {
            assert_eq!(exec_basename(exec).as_deref(), want, "{exec}");
        }
    }

    #[test]
    fn refuses_empty_or_unsafe_package() {
        assert!(safe_package("").is_none());
        assert!(safe_package("  ").is_none());
        assert!(safe_package("foo;rm").is_none());
        assert!(uninstall_command("pacman", "").is_none());
        assert!(uninstall_command("flatpak-user", " ").is_none());
        assert!(uninstall_command("mystery", "firefox").is_none());
        let (bin, args) = uninstall_command("pacman", "firefox").expect("pacman");
        assert_eq!(bin, "pkexec");
        assert!(args.contains(&"firefox".into()));
        assert!(args.contains(&"-Rns".into()));
        let (bin, args) = uninstall_command("flatpak-user", "org.mozilla.firefox").expect("fp");
        assert_eq!(bin, "flatpak");
        assert!(args.contains(&"--user".into()));
    }

    #[test]
    fn resolve_does_not_guess_when_probe_fails() {
        let hint = Hint {
            flatpak: Some("org.mozilla.firefox".into()),
            exec_basename: Some("firefox".into()),
        };
        assert!(resolve(&hint, |_, _| false).is_none());
        let mapped = resolve(&hint, |mgr, pkg| {
            mgr == "flatpak-user" && pkg == "org.mozilla.firefox"
        })
        .expect("mapped");
        assert_eq!(mapped.manager, "flatpak-user");
        let pacman = Hint {
            flatpak: None,
            exec_basename: Some("kitty".into()),
        };
        let mapped = resolve(&pacman, |mgr, pkg| mgr == "pacman" && pkg == "kitty").expect("pm");
        assert_eq!(mapped.manager, "pacman");
        assert_eq!(mapped.package, "kitty");
    }
}
