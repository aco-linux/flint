use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::config::{McpServer, Settings};
use crate::item::{Action, Icon, Item, Kind};
use crate::mode::Mode;
use crate::scripts;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub keywords: String,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

pub fn catalog() -> Vec<Listing> {
    serde_json::from_str(include_str!("../share/store.json")).unwrap_or_default()
}

pub fn items(settings: &Settings) -> Vec<Item> {
    let mut items = Vec::new();

    items.push(Item {
        id: "store:sync-vicinae".into(),
        title: "Refresh Vicinae catalog".into(),
        subtitle: "Linux-native Raycast-compatible extensions from vicinaehq/extensions".into(),
        keywords: "vicinae store raycast extensions".into(),
        kind: Kind::Store,
        icon: Icon::Name("view-refresh".into()),
        action: Action::SyncVicinae,
    });

    items.push(Item {
        id: "store:sync-scripts".into(),
        title: "Sync Raycast Script Commands".into(),
        subtitle: "Open-source raycast/script-commands (not the proprietary App Store)".into(),
        keywords: "raycast store scripts import github".into(),
        kind: Kind::Store,
        icon: Icon::Name("folder-download".into()),
        action: Action::SyncScriptCommands,
    });

    let dir = PathBuf::from(&settings.store.script_commands_dir);
    if dir.exists() && is_safe_git_dest(&dir) {
        items.push(Item {
            id: "store:scripts-folder".into(),
            title: "Open script-commands folder".into(),
            subtitle: dir.to_string_lossy().into_owned(),
            keywords: "store folder".into(),
            kind: Kind::Store,
            icon: Icon::Name("folder".into()),
            action: Action::OpenPath(dir.clone()),
        });
        for cmd in scripts::load_dir(&dir).into_iter().take(24) {
            let mut item = cmd.to_item();
            if !settings.general.allow_script_commands {
                item.subtitle = format!("{} · off until Settings", item.subtitle);
                item.action = Action::EnterMode(Mode::Settings);
            }
            items.push(item);
        }
    }

    let installed_dir = vicinae_dir();
    if installed_dir.exists() {
        items.push(Item {
            id: "store:vicinae-folder".into(),
            title: "Open installed Vicinae extensions".into(),
            subtitle: installed_dir.to_string_lossy().into_owned(),
            keywords: "vicinae folder".into(),
            kind: Kind::Store,
            icon: Icon::Name("folder".into()),
            action: Action::OpenPath(installed_dir.clone()),
        });
    }

    for listing in catalog() {
        let installed = match listing.kind.as_str() {
            "mcp" => settings.mcp.iter().any(|m| m.name == listing.id),
            _ => false,
        };
        let subtitle = if installed {
            format!("Installed · {}", listing.subtitle)
        } else {
            listing.subtitle.clone()
        };
        items.push(Item {
            id: format!("store:{}", listing.id),
            title: listing.title.clone(),
            subtitle,
            keywords: format!("store extension {} {}", listing.kind, listing.keywords),
            kind: Kind::Store,
            icon: Icon::Name(
                match listing.kind.as_str() {
                    "mcp" => "network-workgroup",
                    "script" => "utilities-terminal",
                    _ => "application-x-addon",
                }
                .into(),
            ),
            action: Action::InstallExt {
                id: listing.id.clone(),
            },
        });
    }

    for ext in vicinae_listings() {
        let dest = vicinae_dir().join(&ext.id);
        let subtitle = if dest.exists() {
            format!("Installed · {}", ext.subtitle)
        } else {
            ext.subtitle.clone()
        };
        items.push(Item {
            id: format!("store:vicinae:{}", ext.id),
            title: ext.title.clone(),
            subtitle,
            keywords: format!("vicinae raycast {}", ext.keywords),
            kind: Kind::Store,
            icon: Icon::Name("application-x-addon".into()),
            action: Action::InstallExt {
                id: format!("vicinae:{}", ext.id),
            },
        });
    }

    items
}

pub fn install(id: &str, settings: &mut Settings) -> Result<String, String> {
    let id = id.trim_start_matches("store:");
    if id == "sync-scripts" {
        return sync_script_commands(settings);
    }
    if id == "sync-vicinae" {
        return sync_vicinae();
    }
    if let Some(name) = id.strip_prefix("vicinae:") {
        return install_vicinae(name);
    }
    let listing = catalog()
        .into_iter()
        .find(|l| l.id == id)
        .ok_or_else(|| "Unknown store item".to_string())?;
    match listing.kind.as_str() {
        "mcp" => {
            if settings.mcp.iter().any(|m| m.name == listing.id) {
                return Ok(format!("{} is already installed", listing.title));
            }
            let command = listing.command.clone().unwrap_or_default();
            if !is_safe_mcp_command(&command) {
                return Err("MCP command is not in the allow-list (npx)".into());
            }
            let mut args = listing.args.clone().unwrap_or_default();
            if listing.id == "mcp-filesystem"
                && let Some(home) = dirs::home_dir()
            {
                args = vec![
                    "-y".into(),
                    "@modelcontextprotocol/server-filesystem".into(),
                    home.to_string_lossy().into_owned(),
                ];
            }
            settings.mcp.push(McpServer {
                name: listing.id.clone(),
                command,
                args,
                enabled: false,
            });
            settings.save();
            Ok(format!(
                "Added MCP server {}. Enable “Allow MCP tool listing” in Settings to spawn it.",
                listing.title
            ))
        }
        _ => Err("This listing cannot be installed yet".into()),
    }
}

pub fn sync_script_commands(settings: &Settings) -> Result<String, String> {
    let dest = PathBuf::from(&settings.store.script_commands_dir);
    if !is_safe_git_dest(&dest) {
        return Err("Script-commands folder must stay under Flint’s data directory".into());
    }
    if let Some(parent) = dest.parent() {
        crate::paths::ensure_dir(parent);
    }
    git_clone_or_pull(&dest, "https://github.com/raycast/script-commands.git")?;
    Ok("Raycast Script Commands synced".into())
}

pub fn sync_vicinae() -> Result<String, String> {
    let listings = fetch_vicinae()?;
    crate::paths::ensure();
    let path = vicinae_cache();
    let raw = serde_json::to_string_pretty(&listings).map_err(|e| e.to_string())?;
    crate::paths::write_private(&path, raw).map_err(|e| e.to_string())?;
    Ok(format!(
        "{} Vicinae extensions in the catalog",
        listings.len()
    ))
}

fn vicinae_listings() -> Vec<Listing> {
    if let Ok(raw) = fs::read_to_string(vicinae_cache())
        && let Ok(list) = serde_json::from_str::<Vec<Listing>>(&raw)
    {
        return list;
    }
    bundled_vicinae()
}

fn bundled_vicinae() -> Vec<Listing> {
    [
        ("bluetooth", "Bluetooth", "Manage adapters and devices"),
        ("github", "GitHub", "Issues, PRs, and repositories"),
        ("wifi-commander", "Wi-Fi", "Scan and connect"),
        (
            "process-manager",
            "Process Manager",
            "Inspect and kill processes",
        ),
        ("systemd", "systemd", "Units and services"),
        ("flathub-search", "Flathub", "Search and install Flatpaks"),
        ("fuzzy-files", "Fuzzy Files", "Fast file search"),
        (
            "vscode-recents",
            "VS Code Recents",
            "Jump back into recent workspaces",
        ),
        ("zed-recents", "Zed Recents", "Recent Zed projects"),
        ("hypr", "Hyprland", "Workspaces, clients, and binds"),
        ("clipboard", "Clipboard extras", "Vicinae clipboard helpers"),
        (
            "ollama-wordsmith",
            "Ollama Wordsmith",
            "Talk to local Ollama from an extension",
        ),
    ]
    .into_iter()
    .map(|(id, title, subtitle)| Listing {
        id: id.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        kind: "vicinae".into(),
        keywords: format!("{id} vicinae"),
        command: None,
        args: None,
    })
    .collect()
}

fn fetch_vicinae() -> Result<Vec<Listing>, String> {
    let output = Command::new("curl")
        .args([
            "-sS",
            "--fail",
            "--max-time",
            "20",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: FlintLauncher/0.1",
            "https://api.github.com/repos/vicinaehq/extensions/contents/extensions?ref=main",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    if !output.status.success() {
        return Err("GitHub catalog request failed".into());
    }
    let entries: Vec<GithubEntry> = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Unexpected GitHub catalog".to_string())?;
    let mut listings: Vec<Listing> = entries
        .into_iter()
        .filter(|e| e.kind == "dir")
        .map(|e| {
            let title = title_case(&e.name);
            Listing {
                id: e.name.clone(),
                title: title.clone(),
                subtitle: format!("Vicinae · {title}"),
                kind: "vicinae".into(),
                keywords: format!("{} vicinae raycast", e.name.replace('-', " ")),
                command: None,
                args: None,
            }
        })
        .collect();
    listings.sort_by(|a, b| a.title.cmp(&b.title));
    if listings.is_empty() {
        return Err("Vicinae catalog was empty".into());
    }
    Ok(listings)
}

fn install_vicinae(name: &str) -> Result<String, String> {
    if !is_safe_ext_name(name) {
        return Err("Invalid extension name".into());
    }
    let dest = vicinae_dir().join(name);
    if dest.exists() {
        return Ok(format!("{name} is already installed"));
    }
    crate::paths::ensure_dir(&vicinae_dir());
    let tmp = crate::paths::runtime_dir().join(format!("vicinae-{name}"));
    let _ = fs::remove_dir_all(&tmp);
    let status = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=never",
            "clone",
            "--depth",
            "1",
            "--filter=blob:none",
            "--sparse",
            "https://github.com/vicinaehq/extensions.git",
            tmp.to_str().unwrap_or("."),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "git is not available".to_string())?;
    if !status.success() {
        return Err("Could not clone vicinaehq/extensions".into());
    }
    let _ = Command::new("git")
        .args([
            "-C",
            tmp.to_str().unwrap_or("."),
            "sparse-checkout",
            "set",
            &format!("extensions/{name}"),
        ])
        .status();
    let src = tmp.join("extensions").join(name);
    if !src.exists() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("{name} was not in the Vicinae repo"));
    }
    fs::rename(&src, &dest)
        .or_else(|_| copy_dir(&src, &dest))
        .map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&tmp);
    if dest.join("package.json").exists() && which("npm") {
        let _ = Command::new("npm")
            .args(["install", "--ignore-scripts"])
            .current_dir(&dest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    Ok(format!(
        "Installed {name}. Open it from Vicinae, or run its scripts from the folder."
    ))
}

fn git_clone_or_pull(dest: &Path, url: &str) -> Result<(), String> {
    let status = if dest.join(".git").exists() {
        Command::new("git")
            .args([
                "-c",
                "protocol.file.allow=never",
                "-C",
                dest.to_str().unwrap_or("."),
                "pull",
                "--ff-only",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    } else {
        Command::new("git")
            .args([
                "-c",
                "protocol.file.allow=never",
                "clone",
                "--depth",
                "1",
                url,
                dest.to_str().unwrap_or("."),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err("git failed — install git and try again".into()),
        Err(_) => Err("git is not available".into()),
    }
}

fn copy_dir(src: &PathBuf, dest: &PathBuf) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

fn vicinae_dir() -> PathBuf {
    crate::paths::data_dir().join("store/vicinae")
}

fn vicinae_cache() -> PathBuf {
    crate::paths::data_dir().join("store/vicinae-catalog.json")
}

fn title_case(name: &str) -> String {
    name.split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), c.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_safe_ext_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_safe_mcp_command(command: &str) -> bool {
    matches!(command, "npx")
}

fn is_safe_git_dest(dest: &Path) -> bool {
    if dest
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return false;
    }
    dest.starts_with(crate::paths::data_dir().join("store"))
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_safe_ext_name;

    #[test]
    fn rejects_path_injection_in_extension_names() {
        assert!(is_safe_ext_name("bluetooth"));
        assert!(is_safe_ext_name("wifi-commander"));
        assert!(!is_safe_ext_name("../etc"));
        assert!(!is_safe_ext_name("foo/bar"));
        assert!(!is_safe_ext_name("foo;rm"));
        assert!(!is_safe_ext_name(""));
        assert!(!is_safe_ext_name("foo bar"));
    }

    #[test]
    fn mcp_command_is_npx_only() {
        assert!(super::is_safe_mcp_command("npx"));
        assert!(!super::is_safe_mcp_command("bash"));
        assert!(!super::is_safe_mcp_command("/usr/bin/npx"));
    }

    #[test]
    fn git_dest_stays_under_store() {
        let dest = crate::paths::data_dir().join("store/script-commands");
        assert!(super::is_safe_git_dest(&dest));
        assert!(!super::is_safe_git_dest(&std::path::PathBuf::from(
            "/tmp/evil"
        )));
        assert!(!super::is_safe_git_dest(
            &crate::paths::data_dir().join("store/../evil")
        ));
    }
}
