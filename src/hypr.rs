use serde::Deserialize;

use crate::item::{Action, Icon, Item, Kind};

#[derive(Debug, Deserialize)]
struct Workspace {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Client {
    address: String,
    #[serde(default = "default_true")]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "focusHistoryID")]
    focus_history_id: u32,
    workspace: Option<Workspace>,
}

fn default_true() -> bool {
    true
}

pub fn load_windows() -> Vec<Item> {
    let output = std::process::Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok();
    let Some(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(mut clients) = serde_json::from_slice::<Vec<Client>>(&output.stdout) else {
        return Vec::new();
    };
    clients.sort_by_key(|c| c.focus_history_id);

    clients
        .into_iter()
        .filter(|c| c.mapped && !c.hidden && !c.address.is_empty())
        .filter(|c| {
                c.class != "flint"
                    && c.class != "dev.flint.Launcher"
                    && c.class != "rayblast"
                    && c.class != "dev.rayblast.Launcher"
            })
        .map(|c| {
            let workspace = c
                .workspace
                .as_ref()
                .map(|w| w.name.as_str())
                .unwrap_or("?");
            let title = if c.title.is_empty() {
                c.class.clone()
            } else {
                c.title.clone()
            };
            Item {
                id: format!("win:{}", c.address),
                title,
                subtitle: format!("{} · workspace {workspace}", display_class(&c.class)),
                keywords: c.class.clone(),
                kind: Kind::Window,
                icon: Icon::Name(guess_icon(&c.class)),
                action: Action::FocusWindow { address: c.address },
            }
        })
        .collect()
}

fn display_class(class: &str) -> String {
    class
        .rsplit(['.', ' '])
        .next()
        .unwrap_or(class)
        .to_string()
}

fn guess_icon(class: &str) -> String {
    let last = class.rsplit(['.', ' ']).next().unwrap_or(class);
    last.to_ascii_lowercase()
}
