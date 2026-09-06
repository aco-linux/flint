use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

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

/// Hyprland pushes open / close / focus / title on `.socket2.sock`.
/// We keep `Catalog` current from those events and never poll on a keystroke.
pub fn watch(on_change: impl Fn(Vec<Item>) + Send + 'static) {
    thread::spawn(move || listen(on_change));
}

fn listen(on_change: impl Fn(Vec<Item>)) {
    on_change(load_windows());
    let Some(path) = socket2_path() else {
        return;
    };
    let Ok(stream) = UnixStream::connect(&path) else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut last = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    while reader.read_line(&mut line).ok().is_some_and(|n| n > 0) {
        if is_window_event(line.trim()) {
            let wait = Duration::from_millis(40).saturating_sub(last.elapsed());
            if !wait.is_zero() {
                thread::sleep(wait);
            }
            on_change(load_windows());
            last = Instant::now();
        }
        line.clear();
    }
}

/// Ask Hyprland to treat the launcher as a floating window *before* it maps.
/// Post-map float/resize dispatches are what make Flint appear tiled-large and
/// then contract; the static rule has to win on the first frame instead.
pub fn float_launcher() {
    install_float_rule();
}

pub fn install_float_rule() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let class = crate::APP_ID;
        let width = crate::WINDOW_WIDTH;
        let height = crate::WINDOW_HEIGHT;
        // Match both class and initial_class: GTK can set app_id after the
        // first commit, and static float rules only apply at map time.
        for (name, prop) in [
            ("flint-float", "class"),
            ("flint-float-initial", "initial_class"),
        ] {
            lua_eval(&format!(
                r#"hl.window_rule({{ name = "{name}", match = {{ {prop} = "{class}" }}, float = true, center = true, size = {{{width}, {height}}}, no_anim = true }})"#
            ));
        }
    });
}

fn lua_eval(code: &str) {
    if command_ok(&format!("eval {code}")) {
        return;
    }
    let _ = std::process::Command::new("hyprctl")
        .args(["eval", code])
        .status();
}

fn command_ok(command: &str) -> bool {
    command_socket_json(command).is_some_and(|bytes| {
        String::from_utf8_lossy(&bytes)
            .trim()
            .eq_ignore_ascii_case("ok")
    })
}

pub fn load_windows() -> Vec<Item> {
    let raw = clients_json();
    let Ok(mut clients) = serde_json::from_slice::<Vec<Client>>(&raw) else {
        return Vec::new();
    };
    clients.sort_by_key(|c| c.focus_history_id);

    clients
        .into_iter()
        .filter(|c| c.mapped && !c.hidden && !c.address.is_empty())
        .filter(|c| {
            c.class != "flint"
                && c.class != "dev.flint.launcher"
                && c.class != "rayblast"
                && c.class != "dev.rayblast.Launcher"
        })
        .map(|c| {
            let workspace = c.workspace.as_ref().map(|w| w.name.as_str()).unwrap_or("?");
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

fn clients_json() -> Vec<u8> {
    if let Some(bytes) = command_socket_json("j/clients") {
        return bytes;
    }
    let output = std::process::Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok();
    match output {
        Some(output) if output.status.success() => output.stdout,
        _ => Vec::new(),
    }
}

fn command_socket_json(command: &str) -> Option<Vec<u8>> {
    let path = command_socket_path()?;
    let mut stream = UnixStream::connect(path).ok()?;
    stream.write_all(command.as_bytes()).ok()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    if buf.is_empty() {
        return None;
    }
    Some(buf)
}

pub fn is_window_event(line: &str) -> bool {
    let name = line.split(">>").next().unwrap_or("").trim();
    matches!(
        name,
        "openwindow"
            | "closewindow"
            | "activewindow"
            | "activewindowv2"
            | "movewindow"
            | "movewindowv2"
            | "windowtitle"
            | "windowtitlev2"
            | "changefloatingmode"
            | "fullscreen"
            | "minimize"
            | "urgent"
            | "moveintogroup"
            | "moveoutofgroup"
    )
}

fn instance_dir() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_RUNTIME_DIR")?;
    let hypr = Path::new(&xdg).join("hypr");
    if let Ok(his) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
        let dir = hypr.join(his);
        if dir.exists() {
            return Some(dir);
        }
    }
    let mut found = None;
    for entry in std::fs::read_dir(hypr).ok()? {
        let path = entry.ok()?.path();
        if path.join(".socket2.sock").exists() {
            if found.is_some() {
                return None;
            }
            found = Some(path);
        }
    }
    found
}

fn socket2_path() -> Option<PathBuf> {
    instance_dir().map(|dir| dir.join(".socket2.sock"))
}

fn command_socket_path() -> Option<PathBuf> {
    instance_dir().map(|dir| dir.join(".socket.sock"))
}

fn display_class(class: &str) -> String {
    class.rsplit(['.', ' ']).next().unwrap_or(class).to_string()
}

fn guess_icon(class: &str) -> String {
    let last = class.rsplit(['.', ' ']).next().unwrap_or(class);
    last.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::is_window_event;

    #[test]
    fn hyprland_snippet_floats_a_resizable_launcher() {
        let conf = include_str!("../share/hyprland.conf");
        assert!(
            conf.contains("dev.flint.launcher"),
            "window rules must match APP_ID"
        );
        assert!(
            conf.contains("float = true") || conf.contains("float"),
            "launcher must float, not tile"
        );
        assert!(conf.contains("center"));
        assert!(
            conf.contains(&crate::WINDOW_WIDTH.to_string())
                && conf.contains(&crate::WINDOW_HEIGHT.to_string()),
            "finite size, not a maximized tile"
        );
        assert!(
            conf.contains("no_anim") || conf.contains("noanim"),
            "float must apply on the first frame, not animate tile→float"
        );
        assert!(conf.contains("initial_class") || conf.contains("initialClass"));
        assert!(conf.contains(crate::APP_ID));
    }

    #[test]
    fn window_events_are_the_ones_that_change_the_list() {
        assert!(is_window_event("openwindow>>0x1,1,kitty,term"));
        assert!(is_window_event("closewindow>>0x1"));
        assert!(is_window_event("activewindow>>firefox,GitHub"));
        assert!(is_window_event("windowtitlev2>>0x1,new title"));
        assert!(!is_window_event("workspace>>2"));
        assert!(!is_window_event("ready>>"));
        assert!(!is_window_event(""));
    }
}
