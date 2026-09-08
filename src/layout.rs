use serde::{Deserialize, Serialize};

use crate::db;
use crate::hypr::{self, Client, Monitor};
use crate::item::{Action, Icon, Item, Kind};
use crate::paths;

const ALMOST_INSET: i32 = 48;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedLayout {
    pub name: String,
    #[serde(default)]
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    pub class: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Focus(String),
    EnsureFloating(String),
    MovePixel { address: String, x: i32, y: i32 },
    ResizePixel { address: String, w: i32, h: i32 },
    Maximize(String),
    MoveToMonitor { address: String, name: String },
}

const PRESETS: &[(&str, &str, &str, &str)] = &[
    (
        "left-half",
        "Left half",
        "Tile the window to the left half",
        "layout tile left half",
    ),
    (
        "right-half",
        "Right half",
        "Tile the window to the right half",
        "layout tile right half",
    ),
    (
        "top-half",
        "Top half",
        "Tile the window to the top half",
        "layout tile top half",
    ),
    (
        "bottom-half",
        "Bottom half",
        "Tile the window to the bottom half",
        "layout tile bottom half",
    ),
    (
        "top-left",
        "Top left",
        "Tile the window to the top-left quarter",
        "layout tile quarter top left",
    ),
    (
        "top-right",
        "Top right",
        "Tile the window to the top-right quarter",
        "layout tile quarter top right",
    ),
    (
        "bottom-left",
        "Bottom left",
        "Tile the window to the bottom-left quarter",
        "layout tile quarter bottom left",
    ),
    (
        "bottom-right",
        "Bottom right",
        "Tile the window to the bottom-right quarter",
        "layout tile quarter bottom right",
    ),
    (
        "maximize",
        "Maximize",
        "Maximize the window on its monitor",
        "layout maximize fullscreen",
    ),
    (
        "center",
        "Center",
        "Center the window on its monitor",
        "layout center",
    ),
    (
        "almost-maximize",
        "Almost maximize",
        "Fill the monitor with a 48px inset",
        "layout almost maximize inset",
    ),
    (
        "next-display",
        "Next display",
        "Move the window to the next monitor",
        "layout next display monitor",
    ),
    (
        "prev-display",
        "Previous display",
        "Move the window to the previous monitor",
        "layout previous display monitor",
    ),
];

pub fn builtin_items() -> Vec<Item> {
    PRESETS
        .iter()
        .map(|(name, title, subtitle, keywords)| layout_item(name, title, subtitle, keywords, None))
        .collect()
}

pub fn custom_items() -> Vec<Item> {
    load()
        .into_iter()
        .map(|layout| {
            layout_item(
                &layout.name,
                &format!("Layout: {}", layout.name),
                "Apply saved window layout",
                "layout custom saved",
                None,
            )
        })
        .collect()
}

/// Builtin presets plus saved custom layouts. Prefer Catalog's cache on the
/// keystroke path; this helper still hits SQLite for custom rows.
#[allow(dead_code)]
pub fn all_items() -> Vec<Item> {
    let mut items = builtin_items();
    items.extend(custom_items());
    items
}

pub fn save_item(name: &str) -> Item {
    Item {
        id: format!("cmd:layout-save-{name}"),
        title: format!("Save layout “{name}”"),
        subtitle: "Remember the current window arrangement".into(),
        keywords: format!("layout save {name}"),
        kind: Kind::Command,
        icon: Icon::Name("document-save".into()),
        action: Action::SaveLayout {
            name: name.to_string(),
        },
    }
}

fn layout_item(
    name: &str,
    title: &str,
    subtitle: &str,
    keywords: &str,
    address: Option<String>,
) -> Item {
    Item {
        id: format!("cmd:layout-{name}"),
        title: title.into(),
        subtitle: subtitle.into(),
        keywords: keywords.into(),
        kind: Kind::Command,
        icon: Icon::Name("preferences-system-windows".into()),
        action: Action::Layout {
            name: name.to_string(),
            address,
        },
    }
}

pub fn parse_save_query(query: &str) -> Option<String> {
    let q = query.trim();
    let rest = strip_prefix_ci(q, "layout")?;
    let name = rest.trim().strip_prefix('+')?.trim();
    valid_name(name)
}

pub fn parse_save_short(query: &str) -> Option<String> {
    let name = query.trim().strip_prefix('+')?.trim();
    valid_name(name)
}

fn valid_name(name: &str) -> Option<String> {
    if name.is_empty()
        || name.contains(['/', '\\', '\0'])
        || name == "."
        || name == ".."
        || name.chars().any(|c| c.is_control())
    {
        return None;
    }
    Some(name.to_string())
}

fn strip_prefix_ci<'a>(query: &'a str, prefix: &str) -> Option<&'a str> {
    let q = query.as_bytes();
    let p = prefix.as_bytes();
    if q.len() < p.len() {
        return None;
    }
    if query.get(..p.len())?.eq_ignore_ascii_case(prefix) {
        Some(&query[p.len()..])
    } else {
        None
    }
}

#[allow(dead_code)]
pub fn file() -> std::path::PathBuf {
    paths::config_dir().join("layouts.json")
}

pub fn load() -> Vec<NamedLayout> {
    db::layouts_load().unwrap_or_default()
}

pub fn save(layouts: &[NamedLayout]) {
    for layout in layouts {
        let _ = db::layout_upsert(layout);
    }
}

pub fn upsert(layout: NamedLayout) {
    save(&[layout]);
}

pub fn save_current(name: &str) -> Option<NamedLayout> {
    let name = valid_name(name)?;
    let clients = hypr::clients();
    let monitors = hypr::monitors();
    let slots = capture_slots(&clients, &monitors);
    let layout = NamedLayout { name, slots };
    upsert(layout.clone());
    Some(layout)
}

pub fn capture_slots(clients: &[Client], monitors: &[Monitor]) -> Vec<Slot> {
    clients
        .iter()
        .filter(|c| c.mapped && !c.hidden && !c.address.is_empty() && !c.class.is_empty())
        .filter(|c| !hypr::is_launcher_class(&c.class))
        .filter_map(|c| {
            let mon = hypr::monitor_for(c, monitors)?;
            Some(slot_from_client(c, mon))
        })
        .collect()
}

pub fn slot_from_client(client: &Client, mon: &Monitor) -> Slot {
    let mw = mon.width.max(1) as f64;
    let mh = mon.height.max(1) as f64;
    Slot {
        class: client.class.clone(),
        x: ((client.at[0] - mon.x) as f64 / mw).clamp(0.0, 1.0),
        y: ((client.at[1] - mon.y) as f64 / mh).clamp(0.0, 1.0),
        w: (client.size[0].max(1) as f64 / mw).clamp(0.0, 1.0),
        h: (client.size[1].max(1) as f64 / mh).clamp(0.0, 1.0),
    }
}

pub fn apply(name: &str, address: Option<&str>) {
    let clients = hypr::clients();
    let monitors = hypr::monitors();
    let layouts = load();
    for step in plan(name, address, &clients, &monitors, &layouts) {
        exec_step(&step);
    }
}

pub fn plan(
    name: &str,
    address: Option<&str>,
    clients: &[Client],
    monitors: &[Monitor],
    layouts: &[NamedLayout],
) -> Vec<Step> {
    if is_preset(name) {
        return plan_preset(name, address, clients, monitors);
    }
    plan_named(name, clients, monitors, layouts)
}

fn is_preset(name: &str) -> bool {
    PRESETS.iter().any(|(id, ..)| *id == name)
}

fn plan_preset(
    name: &str,
    address: Option<&str>,
    clients: &[Client],
    monitors: &[Monitor],
) -> Vec<Step> {
    let Some(client) = hypr::target_client(clients, address) else {
        return Vec::new();
    };
    if hypr::is_launcher_class(&client.class) {
        return Vec::new();
    }
    let Some(mon) = hypr::monitor_for(client, monitors).or_else(|| hypr::focused_monitor(monitors))
    else {
        return Vec::new();
    };
    let addr = client.address.clone();
    match name {
        "maximize" => vec![Step::Focus(addr.clone()), Step::Maximize(addr)],
        "next-display" => plan_move_monitor(&addr, monitors, mon, 1),
        "prev-display" => plan_move_monitor(&addr, monitors, mon, -1),
        _ => {
            let Some(rect) = preset_rect(name, mon, client) else {
                return Vec::new();
            };
            place_steps(&addr, client.floating, rect)
        }
    }
}

fn plan_move_monitor(
    address: &str,
    monitors: &[Monitor],
    current: &Monitor,
    dir: i32,
) -> Vec<Step> {
    let Some(next) = hypr::neighbor_monitor(monitors, current, dir) else {
        return Vec::new();
    };
    if next.id == current.id {
        return Vec::new();
    }
    vec![
        Step::Focus(address.to_string()),
        Step::MoveToMonitor {
            address: address.to_string(),
            name: next.name.clone(),
        },
    ]
}

fn plan_named(
    name: &str,
    clients: &[Client],
    monitors: &[Monitor],
    layouts: &[NamedLayout],
) -> Vec<Step> {
    let Some(layout) = layouts.iter().find(|row| row.name == name) else {
        return Vec::new();
    };
    let Some(mon) = hypr::focused_monitor(monitors).or_else(|| {
        clients
            .iter()
            .find(|c| !hypr::is_launcher_class(&c.class))
            .and_then(|c| hypr::monitor_for(c, monitors))
    }) else {
        return Vec::new();
    };
    let mut used = Vec::new();
    let mut steps = Vec::new();
    for slot in &layout.slots {
        let Some(idx) = clients.iter().enumerate().find_map(|(i, c)| {
            if used.contains(&i) {
                return None;
            }
            if c.mapped
                && !c.hidden
                && !hypr::is_launcher_class(&c.class)
                && class_matches(&c.class, &slot.class)
            {
                Some(i)
            } else {
                None
            }
        }) else {
            continue;
        };
        used.push(idx);
        let client = &clients[idx];
        let rect = frac_rect(mon, slot.x, slot.y, slot.w, slot.h);
        steps.extend(place_steps(&client.address, client.floating, rect));
    }
    steps
}

fn place_steps(address: &str, floating: bool, rect: Rect) -> Vec<Step> {
    let mut steps = vec![Step::Focus(address.to_string())];
    if !floating {
        steps.push(Step::EnsureFloating(address.to_string()));
    }
    steps.push(Step::MovePixel {
        address: address.to_string(),
        x: rect.x,
        y: rect.y,
    });
    steps.push(Step::ResizePixel {
        address: address.to_string(),
        w: rect.w.max(1),
        h: rect.h.max(1),
    });
    steps
}

fn exec_step(step: &Step) {
    match step {
        Step::Focus(address) => {
            let _ = hypr::dispatch(&format!("focuswindow address:{address}"));
        }
        Step::EnsureFloating(address) => {
            if !hypr::dispatch(&format!("setfloating address:{address}")) {
                let _ = hypr::dispatch(&format!("togglefloating address:{address}"));
            }
        }
        Step::MovePixel { address, x, y } => {
            let _ = hypr::dispatch(&format!("movewindowpixel exact {x} {y},address:{address}"));
        }
        Step::ResizePixel { address, w, h } => {
            let _ = hypr::dispatch(&format!(
                "resizewindowpixel exact {w} {h},address:{address}"
            ));
        }
        Step::Maximize(address) => {
            let _ = hypr::dispatch(&format!("focuswindow address:{address}"));
            if !hypr::dispatch("fullscreenstate 1 0") {
                let _ = hypr::dispatch("fullscreen 1");
            }
        }
        Step::MoveToMonitor { address, name } => {
            if !hypr::dispatch(&format!("movewindow mon:{name},address:{address}"))
                && !hypr::dispatch(&format!("movewindow mon:{name}"))
            {
                let _ = hypr::dispatch(&format!("movewindow mon:+1,address:{address}"));
            }
        }
    }
}

pub fn preset_rect(name: &str, mon: &Monitor, client: &Client) -> Option<Rect> {
    let (left, right) = split(mon.width);
    let (top, bottom) = split(mon.height);
    Some(match name {
        "left-half" => Rect {
            x: mon.x,
            y: mon.y,
            w: left,
            h: mon.height,
        },
        "right-half" => Rect {
            x: mon.x + left,
            y: mon.y,
            w: right,
            h: mon.height,
        },
        "top-half" => Rect {
            x: mon.x,
            y: mon.y,
            w: mon.width,
            h: top,
        },
        "bottom-half" => Rect {
            x: mon.x,
            y: mon.y + top,
            w: mon.width,
            h: bottom,
        },
        "top-left" => Rect {
            x: mon.x,
            y: mon.y,
            w: left,
            h: top,
        },
        "top-right" => Rect {
            x: mon.x + left,
            y: mon.y,
            w: right,
            h: top,
        },
        "bottom-left" => Rect {
            x: mon.x,
            y: mon.y + top,
            w: left,
            h: bottom,
        },
        "bottom-right" => Rect {
            x: mon.x + left,
            y: mon.y + top,
            w: right,
            h: bottom,
        },
        "almost-maximize" => {
            let inset = ALMOST_INSET;
            Rect {
                x: mon.x + inset,
                y: mon.y + inset,
                w: (mon.width - inset * 2).max(1),
                h: (mon.height - inset * 2).max(1),
            }
        }
        "center" => {
            let w = client.size[0].max(1);
            let h = client.size[1].max(1);
            Rect {
                x: mon.x + (mon.width - w) / 2,
                y: mon.y + (mon.height - h) / 2,
                w,
                h,
            }
        }
        _ => return None,
    })
}

fn split(len: i32) -> (i32, i32) {
    let a = len / 2;
    (a, len - a)
}

pub fn frac_rect(mon: &Monitor, x: f64, y: f64, w: f64, h: f64) -> Rect {
    let mw = mon.width.max(1) as f64;
    let mh = mon.height.max(1) as f64;
    Rect {
        x: mon.x + (mw * x.clamp(0.0, 1.0)).round() as i32,
        y: mon.y + (mh * y.clamp(0.0, 1.0)).round() as i32,
        w: (mw * w.clamp(0.0, 1.0)).round().max(1.0) as i32,
        h: (mh * h.clamp(0.0, 1.0)).round().max(1.0) as i32,
    }
}

pub fn class_matches(client: &str, slot: &str) -> bool {
    let client = client.trim();
    let slot = slot.trim();
    if client.eq_ignore_ascii_case(slot) {
        return true;
    }
    let c = client.to_ascii_lowercase();
    let s = slot.to_ascii_lowercase();
    c.ends_with(&format!(".{s}"))
        || c.rsplit(['.', ' ']).next().is_some_and(|last| last == s)
        || s.rsplit(['.', ' ']).next().is_some_and(|last| last == c)
}

#[cfg(test)]
mod tests {
    use super::{
        NamedLayout, Slot, Step, capture_slots, class_matches, frac_rect, parse_save_query,
        parse_save_short, plan, preset_rect, valid_name,
    };
    use crate::hypr::{self, Client, Monitor};

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
        "availableModes": ["1920x1080@60.00Hz"]
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
        "address": "0xff",
        "mapped": true,
        "hidden": false,
        "at": [0, 0],
        "size": [800, 600],
        "workspace": {"id": 1, "name": "1"},
        "floating": false,
        "monitor": 0,
        "class": "firefox",
        "title": "Firefox",
        "pid": 4242,
        "focusHistoryID": 1
      },
      {
        "address": "0xcode",
        "mapped": true,
        "at": [960, 0],
        "size": [960, 1080],
        "floating": true,
        "monitor": 0,
        "class": "code",
        "title": "Code",
        "pid": 7,
        "focusHistoryID": 2
      },
      {
        "address": "0xflint",
        "mapped": true,
        "at": [100, 100],
        "size": [980, 720],
        "monitor": 0,
        "class": "dev.flint.launcher",
        "pid": 99,
        "focusHistoryID": 0
      }
    ]"#;

    fn step_address(step: &Step) -> Option<&str> {
        match step {
            Step::Focus(a)
            | Step::EnsureFloating(a)
            | Step::Maximize(a)
            | Step::MovePixel { address: a, .. }
            | Step::ResizePixel { address: a, .. }
            | Step::MoveToMonitor { address: a, .. } => Some(a.as_str()),
        }
    }

    fn fixtures() -> (Vec<Client>, Vec<Monitor>) {
        (
            hypr::parse_clients(CLIENTS.as_bytes()),
            hypr::parse_monitors(MONITORS.as_bytes()),
        )
    }

    #[test]
    fn preset_rects_table() {
        let (clients, monitors) = fixtures();
        let mon = &monitors[0];
        let fox = &clients[0];
        let cases = [
            ("left-half", 0, 0, 960, 1080),
            ("right-half", 960, 0, 960, 1080),
            ("top-half", 0, 0, 1920, 540),
            ("bottom-half", 0, 540, 1920, 540),
            ("top-left", 0, 0, 960, 540),
            ("top-right", 960, 0, 960, 540),
            ("bottom-left", 0, 540, 960, 540),
            ("bottom-right", 960, 540, 960, 540),
            ("almost-maximize", 48, 48, 1824, 984),
            ("center", 560, 240, 800, 600),
        ];
        for (name, x, y, w, h) in cases {
            let rect = preset_rect(name, mon, fox).expect(name);
            assert_eq!((rect.x, rect.y, rect.w, rect.h), (x, y, w, h), "{name}");
        }
        assert!(preset_rect("maximize", mon, fox).is_none());
    }

    #[test]
    fn left_half_plan_floats_and_never_moves_flint() {
        let (clients, monitors) = fixtures();
        let steps = plan("left-half", None, &clients, &monitors, &[]);
        assert!(steps.iter().all(|s| step_address(s) != Some("0xflint")));
        assert!(matches!(&steps[0], Step::Focus(a) if a == "0xff"));
        assert!(matches!(&steps[1], Step::EnsureFloating(a) if a == "0xff"));
        assert!(matches!(&steps[2], Step::MovePixel { address, x: 0, y: 0 } if address == "0xff"));
        assert!(
            matches!(&steps[3], Step::ResizePixel { address, w: 960, h: 1080 } if address == "0xff")
        );
    }

    #[test]
    fn selected_address_wins_over_focus_history() {
        let (clients, monitors) = fixtures();
        let steps = plan("right-half", Some("0xcode"), &clients, &monitors, &[]);
        assert!(matches!(&steps[0], Step::Focus(a) if a == "0xcode"));
        assert!(
            !steps.iter().any(|s| matches!(s, Step::EnsureFloating(_))),
            "already floating"
        );
    }

    #[test]
    fn next_display_targets_hdmi() {
        let (clients, monitors) = fixtures();
        let steps = plan("next-display", Some("0xff"), &clients, &monitors, &[]);
        assert!(matches!(
            &steps[1],
            Step::MoveToMonitor { address, name } if address == "0xff" && name == "HDMI-A-1"
        ));
    }

    #[test]
    fn custom_layout_matches_class_and_skips_flint() {
        let (clients, monitors) = fixtures();
        let layouts = [NamedLayout {
            name: "code".into(),
            slots: vec![
                Slot {
                    class: "firefox".into(),
                    x: 0.0,
                    y: 0.0,
                    w: 0.5,
                    h: 1.0,
                },
                Slot {
                    class: "code".into(),
                    x: 0.5,
                    y: 0.0,
                    w: 0.5,
                    h: 1.0,
                },
            ],
        }];
        let steps = plan("code", None, &clients, &monitors, &layouts);
        let addrs: Vec<&str> = steps.iter().filter_map(step_address).collect();
        assert_eq!(
            addrs
                .iter()
                .copied()
                .filter(|a| *a != "0xff" && *a != "0xcode")
                .count(),
            0
        );
        assert!(addrs.contains(&"0xff"));
        assert!(addrs.contains(&"0xcode"));
        assert!(!addrs.contains(&"0xflint"));
        let slots = capture_slots(&clients, &monitors);
        assert!(slots.iter().all(|s| s.class != "dev.flint.launcher"));
        assert_eq!(slots.len(), 2);
    }

    #[test]
    fn fractions_of_monitor() {
        let (_, monitors) = fixtures();
        let rect = frac_rect(&monitors[0], 0.5, 0.0, 0.5, 1.0);
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (960, 0, 960, 1080));
    }

    #[test]
    fn class_match_table() {
        let cases = [
            ("firefox", "firefox", true),
            ("org.mozilla.firefox", "firefox", true),
            ("Firefox", "firefox", true),
            ("code", "firefox", false),
        ];
        for (client, slot, want) in cases {
            assert_eq!(class_matches(client, slot), want, "{client} vs {slot}");
        }
    }

    #[test]
    fn save_query_table() {
        let cases = [
            ("layout +code", Some("code")),
            ("LAYOUT +dev", Some("dev")),
            ("layout +", None),
            ("layout code", None),
            ("+code", None),
        ];
        for (q, want) in cases {
            assert_eq!(parse_save_query(q).as_deref(), want, "{q}");
        }
        assert_eq!(parse_save_short("+code").as_deref(), Some("code"));
        assert!(valid_name("../x").is_none());
        assert!(valid_name("").is_none());
    }
}
