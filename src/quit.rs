use crate::db;
use crate::hypr::{self, Client};
use crate::item::{Action, Icon, Item, Kind};
use crate::paths;

const ALWAYS_KEEP: &[&str] = &[
    "flint",
    "dev.flint.launcher",
    "rayblast",
    "dev.rayblast.Launcher",
];

pub fn items() -> Vec<Item> {
    vec![quit_all_item()]
}

pub fn quit_all_item() -> Item {
    Item {
        id: "cmd:quit-all".into(),
        title: "Quit all applications".into(),
        subtitle: "Close every window except Flint and the denylist".into(),
        keywords: "quit all applications close kill".into(),
        kind: Kind::Command,
        icon: Icon::Name("application-exit".into()),
        action: Action::QuitAll,
    }
}

pub fn confirm_item() -> Item {
    Item {
        id: "cmd:quit-all-confirm".into(),
        title: "Confirm quit all applications".into(),
        subtitle: "Enter closes every window except Flint and the denylist".into(),
        keywords: "quit all confirm".into(),
        kind: Kind::Command,
        icon: Icon::Name("dialog-warning".into()),
        action: Action::ConfirmQuitAll,
    }
}

#[allow(dead_code)]
pub fn file() -> std::path::PathBuf {
    paths::config_dir().join("quit-keep.json")
}

pub fn default_keep() -> Vec<String> {
    ALWAYS_KEEP.iter().map(|s| (*s).to_string()).collect()
}

pub fn load_keep() -> Vec<String> {
    let mut keep = default_keep();
    let extra = db::quit_keep_load().unwrap_or_default();
    for class in extra {
        let class = class.trim();
        if class.is_empty() {
            continue;
        }
        if !keep.iter().any(|k| k.eq_ignore_ascii_case(class)) {
            keep.push(class.to_string());
        }
    }
    keep
}

pub fn can_kill(pid: i32, self_pid: i32) -> bool {
    pid > 1 && pid != self_pid
}

pub fn should_keep(class: &str, keep: &[String]) -> bool {
    hypr::is_launcher_class(class) || keep.iter().any(|k| k.eq_ignore_ascii_case(class))
}

pub fn targets<'a>(clients: &'a [Client], keep: &[String], self_pid: i32) -> Vec<&'a Client> {
    clients
        .iter()
        .filter(|c| {
            c.mapped
                && !c.hidden
                && !c.address.is_empty()
                && !should_keep(&c.class, keep)
                && c.pid != 1
                && c.pid != self_pid
        })
        .collect()
}

pub fn close_window(address: &str) {
    if address.trim().is_empty() {
        return;
    }
    if let Some(client) = hypr::client_by_address(&hypr::clients(), address)
        && hypr::is_launcher_class(&client.class)
    {
        return;
    }
    let _ = hypr::dispatch(&format!("closewindow address:{address}"));
}

pub fn kill_pid(pid: i32) {
    let self_pid = std::process::id() as i32;
    if !can_kill(pid, self_pid) {
        return;
    }
    if let Some(client) = hypr::clients().iter().find(|c| c.pid == pid)
        && hypr::is_launcher_class(&client.class)
    {
        return;
    }
    // SAFETY: pid is a foreign mapped client, never 1 or our own pid.
    unsafe {
        let _ = libc::kill(pid, libc::SIGKILL);
    }
}

pub fn quit_class(class: &str) {
    let class = class.trim();
    if class.is_empty() || hypr::is_launcher_class(class) {
        return;
    }
    let self_pid = std::process::id() as i32;
    let keep = load_keep();
    for client in hypr::clients() {
        if !class_eq(&client.class, class) {
            continue;
        }
        if should_keep(&client.class, &keep) || client.pid == 1 || client.pid == self_pid {
            continue;
        }
        if !client.address.is_empty() {
            let _ = hypr::dispatch(&format!("closewindow address:{}", client.address));
        }
    }
}

pub fn quit_all() {
    let self_pid = std::process::id() as i32;
    let keep = load_keep();
    let clients = hypr::clients();
    for client in targets(&clients, &keep, self_pid) {
        if !client.address.is_empty() {
            let _ = hypr::dispatch(&format!("closewindow address:{}", client.address));
        }
    }
}

fn class_eq(a: &str, b: &str) -> bool {
    crate::layout::class_matches(a, b)
}

#[cfg(test)]
mod tests {
    use super::{can_kill, default_keep, should_keep, targets};
    use crate::hypr;

    const CLIENTS: &str = r#"[
      {
        "address": "0xff",
        "mapped": true,
        "class": "firefox",
        "pid": 4242,
        "focusHistoryID": 1
      },
      {
        "address": "0xinit",
        "mapped": true,
        "class": "init",
        "pid": 1,
        "focusHistoryID": 2
      },
      {
        "address": "0xflint",
        "mapped": true,
        "class": "dev.flint.launcher",
        "pid": 99,
        "focusHistoryID": 0
      },
      {
        "address": "0xterm",
        "mapped": true,
        "class": "kitty",
        "pid": 50,
        "focusHistoryID": 3
      },
      {
        "address": "0xkeep",
        "mapped": true,
        "class": "keepme",
        "pid": 77,
        "focusHistoryID": 4
      }
    ]"#;

    #[test]
    fn refuses_pid_1_and_self() {
        let cases = [
            (1, 100, false),
            (0, 100, false),
            (-5, 100, false),
            (100, 100, false),
            (4242, 100, true),
        ];
        for (pid, self_pid, want) in cases {
            assert_eq!(can_kill(pid, self_pid), want, "pid {pid} self {self_pid}");
        }
    }

    #[test]
    fn denylist_keeps_flint_and_extra_classes() {
        let clients = hypr::parse_clients(CLIENTS.as_bytes());
        let mut keep = default_keep();
        keep.push("keepme".into());
        assert!(should_keep("dev.flint.launcher", &keep));
        assert!(should_keep("keepme", &keep));
        assert!(!should_keep("firefox", &keep));
        let self_pid = 50;
        let got: Vec<&str> = targets(&clients, &keep, self_pid)
            .iter()
            .map(|c| c.class.as_str())
            .collect();
        assert_eq!(got, ["firefox"]);
        assert!(!got.contains(&"init"));
        assert!(!got.contains(&"dev.flint.launcher"));
        assert!(!got.contains(&"kitty"), "own pid skipped");
        assert!(!got.contains(&"keepme"));
    }
}
