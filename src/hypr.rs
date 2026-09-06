use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::item::{Action, Icon, Item, Kind};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Workspace {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Client {
    #[serde(default)]
    pub address: String,
    #[serde(default = "default_true")]
    pub mapped: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, rename = "focusHistoryID")]
    pub focus_history_id: u32,
    #[serde(default)]
    pub workspace: Option<Workspace>,
    #[serde(default)]
    pub at: [i32; 2],
    #[serde(default)]
    pub size: [i32; 2],
    #[serde(default)]
    pub pid: i32,
    #[serde(default)]
    pub monitor: i64,
    #[serde(default)]
    pub floating: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Monitor {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub width: i32,
    #[serde(default)]
    pub height: i32,
    #[serde(default, rename = "refreshRate")]
    pub refresh_rate: f64,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub scale: f64,
    #[serde(default)]
    pub focused: bool,
    #[serde(default, rename = "availableModes")]
    pub available_modes: Vec<String>,
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
    let _ = Command::new("hyprctl").args(["eval", code]).status();
}

fn command_ok(command: &str) -> bool {
    command_socket_json(command).is_some_and(|bytes| {
        String::from_utf8_lossy(&bytes)
            .trim()
            .eq_ignore_ascii_case("ok")
    })
}

pub fn load_windows() -> Vec<Item> {
    let mut clients = clients();
    clients.sort_by_key(|c| c.focus_history_id);

    clients
        .into_iter()
        .filter(|c| c.mapped && !c.hidden && !c.address.is_empty())
        .filter(|c| !is_launcher_class(&c.class))
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

pub(crate) fn is_launcher_class(class: &str) -> bool {
    let class = class.trim();
    class.eq_ignore_ascii_case("flint")
        || class.eq_ignore_ascii_case("dev.flint.launcher")
        || class.eq_ignore_ascii_case("rayblast")
        || class.eq_ignore_ascii_case("dev.rayblast.Launcher")
}

pub(crate) fn clients() -> Vec<Client> {
    parse_clients(&json_cmd("j/clients", &["clients", "-j"]))
}

pub(crate) fn monitors() -> Vec<Monitor> {
    parse_monitors(&json_cmd("j/monitors", &["monitors", "-j"]))
}

pub(crate) fn parse_clients(raw: &[u8]) -> Vec<Client> {
    serde_json::from_slice(raw).unwrap_or_default()
}

pub(crate) fn parse_monitors(raw: &[u8]) -> Vec<Monitor> {
    serde_json::from_slice(raw).unwrap_or_default()
}

pub(crate) fn client_by_address<'a>(clients: &'a [Client], address: &str) -> Option<&'a Client> {
    clients.iter().find(|c| c.address == address)
}

pub(crate) fn focused_monitor(monitors: &[Monitor]) -> Option<&Monitor> {
    monitors
        .iter()
        .find(|m| m.focused)
        .or_else(|| monitors.first())
}

pub(crate) fn monitor_for<'a>(client: &Client, monitors: &'a [Monitor]) -> Option<&'a Monitor> {
    monitors
        .iter()
        .find(|m| m.id == client.monitor)
        .or_else(|| {
            monitors.iter().find(|m| {
                let x = client.at[0];
                let y = client.at[1];
                x >= m.x && y >= m.y && x < m.x + m.width && y < m.y + m.height
            })
        })
}

pub(crate) fn neighbor_monitor<'a>(
    monitors: &'a [Monitor],
    current: &Monitor,
    dir: i32,
) -> Option<&'a Monitor> {
    if monitors.is_empty() {
        return None;
    }
    let mut order: Vec<&Monitor> = monitors.iter().collect();
    order.sort_by_key(|m| (m.x, m.y, m.id));
    let idx = order.iter().position(|m| m.id == current.id)?;
    let len = order.len() as i32;
    let next = (idx as i32 + dir).rem_euclid(len) as usize;
    Some(order[next])
}

pub(crate) fn target_client<'a>(
    clients: &'a [Client],
    address: Option<&str>,
) -> Option<&'a Client> {
    if let Some(address) = address
        && let Some(client) = client_by_address(clients, address)
        && client.mapped
        && !client.hidden
        && !is_launcher_class(&client.class)
    {
        return Some(client);
    }
    let mut ranked: Vec<&Client> = clients
        .iter()
        .filter(|c| c.mapped && !c.hidden && !c.address.is_empty() && !is_launcher_class(&c.class))
        .collect();
    ranked.sort_by_key(|c| c.focus_history_id);
    ranked.into_iter().next()
}

pub(crate) fn dispatch(spec: &str) -> bool {
    if command_ok(&format!("dispatch {spec}")) {
        return true;
    }
    Command::new("hyprctl")
        .arg("dispatch")
        .arg("--")
        .args(spec.split_whitespace())
        .status()
        .ok()
        .is_some_and(|s| s.success())
}

pub(crate) fn keyword(spec: &str) -> bool {
    if command_ok(&format!("keyword {spec}")) {
        return true;
    }
    Command::new("hyprctl")
        .args(["keyword", spec])
        .status()
        .ok()
        .is_some_and(|s| s.success())
}

pub(crate) fn parse_mode(raw: &str) -> Option<(u32, u32, f64)> {
    let raw = raw.trim().trim_end_matches("Hz").trim();
    if raw.is_empty() {
        return None;
    }
    let (wh, rate) = match raw.split_once('@') {
        Some((wh, rate)) => (wh.trim(), rate.trim().parse().ok()?),
        None => (raw, 60.0),
    };
    let (w, h) = wh.split_once('x')?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h, rate))
}

pub(crate) fn format_monitor_keyword(mon: &Monitor, width: u32, height: u32, rate: f64) -> String {
    let rate = if (rate.fract() - 0.0).abs() < 0.05 {
        format!("{rate:.0}")
    } else {
        format!("{rate:.2}")
    };
    let scale = if (mon.scale - 0.0).abs() < f64::EPSILON || (mon.scale - 1.0).abs() < 0.001 {
        "1".into()
    } else {
        format!("{}", mon.scale)
    };
    format!(
        "{},{width}x{height}@{rate},{}x{},{scale}",
        mon.name, mon.x, mon.y
    )
}

pub(crate) fn resolution_items_from(monitors: &[Monitor]) -> Vec<Item> {
    let mut items = Vec::new();
    for mon in monitors {
        if mon.name.is_empty() || mon.width <= 0 || mon.height <= 0 {
            continue;
        }
        let mut seen = HashSet::new();
        let mut modes = Vec::new();
        for raw in &mon.available_modes {
            if let Some((w, h, hz)) = parse_mode(raw)
                && seen.insert((w, h))
            {
                modes.push((w, h, hz));
            }
        }
        let fallback_rate = if mon.refresh_rate > 1.0 {
            mon.refresh_rate
        } else {
            60.0
        };
        for (w, h) in [(1920u32, 1080u32), (2560, 1440), (3840, 2160), (1280, 720)] {
            if seen.insert((w, h)) {
                modes.push((w, h, fallback_rate));
            }
        }
        for (w, h, hz) in modes {
            let spec = format_monitor_keyword(mon, w, h, hz);
            items.push(Item {
                id: format!("cmd:res-{}-{w}x{h}", mon.name),
                title: format!("{} · {w}×{h}", mon.name),
                subtitle: format!("Set display resolution @ {:.0} Hz", hz),
                keywords: format!("resolution display monitor {w}x{h} {}", mon.name),
                kind: Kind::Command,
                icon: Icon::Name("video-display".into()),
                action: Action::SetResolution { spec },
            });
        }
    }
    items
}

pub fn resolution_items() -> Vec<Item> {
    resolution_items_from(&monitors())
}

fn json_cmd(socket: &str, hyprctl_args: &[&str]) -> Vec<u8> {
    if let Some(bytes) = command_socket_json(socket) {
        return bytes;
    }
    let output = Command::new("hyprctl").args(hyprctl_args).output().ok();
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
    use super::{
        format_monitor_keyword, is_launcher_class, is_window_event, neighbor_monitor,
        parse_clients, parse_mode, parse_monitors, resolution_items_from, target_client,
    };

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

    const MONITORS: &str = r#"[
      {
        "id": 0,
        "name": "DP-1",
        "width": 1920,
        "height": 1080,
        "refreshRate": 60.0,
        "x": 0,
        "y": 0,
        "scale": 1.0,
        "focused": true,
        "availableModes": ["1920x1080@60.00Hz", "1280x720@60.00Hz"]
      },
      {
        "id": 1,
        "name": "HDMI-A-1",
        "width": 2560,
        "height": 1440,
        "refreshRate": 144.0,
        "x": 1920,
        "y": 0,
        "scale": 1.0,
        "focused": false,
        "availableModes": ["2560x1440@144.00Hz"]
      }
    ]"#;

    const CLIENTS: &str = r#"[
      {
        "address": "0xaaa",
        "mapped": true,
        "hidden": false,
        "at": [100, 100],
        "size": [800, 600],
        "workspace": {"id": 1, "name": "1"},
        "floating": true,
        "monitor": 0,
        "class": "firefox",
        "title": "Mozilla Firefox",
        "pid": 4242,
        "focusHistoryID": 1
      },
      {
        "address": "0xflint",
        "mapped": true,
        "at": [200, 200],
        "size": [980, 720],
        "monitor": 0,
        "class": "dev.flint.launcher",
        "title": "Flint",
        "pid": 99,
        "focusHistoryID": 0
      }
    ]"#;

    #[test]
    fn parses_monitors_and_clients_from_hyprctl_json() {
        let monitors = parse_monitors(MONITORS.as_bytes());
        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0].name, "DP-1");
        assert_eq!(monitors[0].available_modes.len(), 2);
        let clients = parse_clients(CLIENTS.as_bytes());
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0].pid, 4242);
        assert_eq!(clients[0].at, [100, 100]);
        assert_eq!(clients[0].size, [800, 600]);
        assert!(is_launcher_class(&clients[1].class));
    }

    #[test]
    fn target_skips_flint_and_uses_focus_history() {
        let clients = parse_clients(CLIENTS.as_bytes());
        let target = target_client(&clients, None).expect("firefox");
        assert_eq!(target.address, "0xaaa");
        assert_eq!(
            target_client(&clients, Some("0xflint")).map(|c| c.address.as_str()),
            Some("0xaaa"),
            "flint is never the layout target — fall back to the focused client"
        );
        assert_eq!(
            target_client(&clients, Some("0xaaa")).map(|c| c.class.as_str()),
            Some("firefox")
        );
    }

    #[test]
    fn next_and_prev_monitor_wrap() {
        let monitors = parse_monitors(MONITORS.as_bytes());
        let next = neighbor_monitor(&monitors, &monitors[0], 1).expect("hdmi");
        assert_eq!(next.name, "HDMI-A-1");
        let prev = neighbor_monitor(&monitors, &monitors[0], -1).expect("wrap");
        assert_eq!(prev.name, "HDMI-A-1");
        let back = neighbor_monitor(&monitors, &monitors[1], 1).expect("wrap dp");
        assert_eq!(back.name, "DP-1");
    }

    #[test]
    fn mode_parser_table() {
        let cases = [
            ("1920x1080@60.00Hz", Some((1920, 1080, 60.0))),
            ("2560x1440@144Hz", Some((2560, 1440, 144.0))),
            ("1280x720", Some((1280, 720, 60.0))),
            ("", None),
            ("not-a-mode", None),
        ];
        for (raw, want) in cases {
            let got = parse_mode(raw);
            match want {
                Some((w, h, hz)) => {
                    let (gw, gh, ghz) = got.expect(raw);
                    assert_eq!((gw, gh), (w, h), "{raw}");
                    assert!((ghz - hz).abs() < 0.01, "{raw}");
                }
                None => assert!(got.is_none(), "{raw}"),
            }
        }
    }

    #[test]
    fn resolution_items_include_available_and_common_modes() {
        let monitors = parse_monitors(MONITORS.as_bytes());
        let items = resolution_items_from(&monitors);
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains(&"cmd:res-DP-1-1920x1080"));
        assert!(ids.contains(&"cmd:res-DP-1-1280x720"));
        assert!(ids.contains(&"cmd:res-DP-1-2560x1440"));
        assert!(ids.contains(&"cmd:res-DP-1-3840x2160"));
        assert!(ids.contains(&"cmd:res-HDMI-A-1-2560x1440"));
        assert!(
            ids.iter()
                .filter(|id| **id == "cmd:res-DP-1-1920x1080")
                .count()
                == 1
        );
        let spec = format_monitor_keyword(&monitors[0], 1920, 1080, 60.0);
        assert_eq!(spec, "DP-1,1920x1080@60,0x0,1");
        assert!(items.iter().any(|i| matches!(
            &i.action,
            crate::item::Action::SetResolution { spec } if spec == "DP-1,1920x1080@60,0x0,1"
        )));
    }
}
