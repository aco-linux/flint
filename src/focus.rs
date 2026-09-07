use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::item::{Action, Icon, Item, Kind};
use crate::paths;

const FOCUS_SECS: u32 = 25 * 60;
const BREAK_SECS: u32 = 5 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Focus,
    Break,
}

impl SessionKind {
    fn parse(label: &str) -> Self {
        if label.eq_ignore_ascii_case("break") {
            Self::Break
        } else {
            Self::Focus
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::Break => "break",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Focus => "Focus",
            Self::Break => "Break",
        }
    }
}

struct Inner {
    kind: SessionKind,
    end: Instant,
}

static STATE: Mutex<Option<Inner>> = Mutex::new(None);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tick {
    Running { text: String },
    Done { text: String },
}

#[derive(Serialize)]
struct StatusFile {
    text: String,
    class: String,
    tooltip: String,
}

pub fn start(seconds: u32, label: &str) {
    let kind = SessionKind::parse(label);
    let seconds = seconds.max(1);
    if let Ok(mut g) = STATE.lock() {
        *g = Some(Inner {
            kind,
            end: Instant::now() + Duration::from_secs(u64::from(seconds)),
        });
    }
    write_status();
}

pub fn stop() {
    if let Ok(mut g) = STATE.lock() {
        *g = None;
    }
    write_idle();
}

/// Remaining `(kind, seconds)`. `None` when idle. Read from the process mutex —
/// never from `status.json` on a keystroke.
pub fn remaining() -> Option<(SessionKind, u64)> {
    let g = STATE.lock().ok()?;
    let inner = g.as_ref()?;
    let left = inner
        .end
        .saturating_duration_since(Instant::now())
        .as_secs();
    Some((inner.kind, left))
}

pub fn live_item() -> Option<Item> {
    let (kind, secs) = remaining().filter(|(_, secs)| *secs > 0)?;
    let mmss = format_mmss(secs);
    Some(Item {
        id: "cmd:focus-now".into(),
        title: format!("{} {mmss}", kind.label()),
        subtitle: "Enter to stop · Waybar reads status.json".into(),
        keywords: "focus unfocus break timer pomodoro waybar".into(),
        kind: Kind::Command,
        icon: Icon::Name("appointment-soon".into()),
        action: Action::StopFocus,
    })
}

pub fn status_line() -> Option<String> {
    let (kind, secs) = remaining()?;
    Some(format!("{} {}", kind.label(), format_mmss(secs)))
}

pub fn tick() -> Tick {
    match remaining() {
        Some((kind, secs)) if secs > 0 => {
            write_status();
            Tick::Running {
                text: format!("{} {}", kind.label(), format_mmss(secs)),
            }
        }
        Some((kind, _)) => {
            if let Ok(mut g) = STATE.lock() {
                *g = None;
            }
            write_idle();
            Tick::Done {
                text: format!("{} complete", kind.label()),
            }
        }
        None => {
            write_idle();
            Tick::Done {
                text: "Focus stopped".into(),
            }
        }
    }
}

pub fn format_mmss(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

pub fn status_payload(kind: SessionKind, secs: u64) -> String {
    let text = format_mmss(secs);
    let payload = StatusFile {
        tooltip: format!("{} session · {text} remaining", kind.label()),
        class: kind.class().to_string(),
        text,
    };
    serde_json::to_string(&payload).unwrap_or_else(|_| {
        format!(
            r#"{{"text":"{}","class":"{}","tooltip":"{}"}}"#,
            format_mmss(secs),
            kind.class(),
            kind.label()
        )
    })
}

pub fn idle_payload() -> String {
    serde_json::to_string(&StatusFile {
        text: String::new(),
        class: "idle".into(),
        tooltip: "No focus session".into(),
    })
    .unwrap_or_else(|_| r#"{"text":"","class":"idle","tooltip":"No focus session"}"#.into())
}

pub fn status_path() -> std::path::PathBuf {
    paths::data_dir().join("status.json")
}

fn write_status() {
    #[cfg(not(test))]
    if let Some((kind, secs)) = remaining() {
        let _ = paths::write_private(&status_path(), status_payload(kind, secs));
    } else {
        write_idle();
    }
}

fn write_idle() {
    #[cfg(not(test))]
    {
        let _ = paths::write_private(&status_path(), idle_payload());
    }
}

pub fn items() -> Vec<Item> {
    vec![
        cmd(
            "focus-start",
            "Start focus 25m",
            "Pomodoro-style focus · Waybar reads status.json",
            Action::StartFocus {
                seconds: FOCUS_SECS,
                label: "focus".into(),
            },
        ),
        cmd(
            "focus-break",
            "Start break 5m",
            "Five-minute break · Waybar reads status.json",
            Action::StartFocus {
                seconds: BREAK_SECS,
                label: "break".into(),
            },
        ),
        cmd(
            "focus-stop",
            "Stop focus",
            "End the current focus or break",
            Action::StopFocus,
        ),
        cmd(
            "unfocus",
            "Unfocus",
            "End the current focus session",
            Action::StopFocus,
        ),
    ]
}

fn cmd(id: &str, title: &str, subtitle: &str, action: Action) -> Item {
    Item {
        id: format!("cmd:{id}"),
        title: title.into(),
        subtitle: subtitle.into(),
        keywords: "focus unfocus break timer pomodoro waybar".into(),
        kind: Kind::Command,
        icon: Icon::Name("appointment-soon".into()),
        action,
    }
}

#[cfg(test)]
pub(crate) fn reset() {
    if let Ok(mut g) = STATE.lock() {
        *g = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BREAK_SECS, FOCUS_SECS, SessionKind, Tick, format_mmss, idle_payload, remaining, reset,
        start, status_payload, stop, tick,
    };
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn mmss_zero_pads() {
        assert_eq!(format_mmss(1500), "25:00");
        assert_eq!(format_mmss(5 * 60), "05:00");
        assert_eq!(format_mmss(65), "01:05");
        assert_eq!(format_mmss(0), "00:00");
    }

    #[test]
    fn status_json_is_tiny() {
        let raw = status_payload(SessionKind::Focus, 1500);
        let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
        assert_eq!(v["text"], "25:00");
        assert_eq!(v["class"], "focus");
        assert!(v["tooltip"].as_str().unwrap_or("").contains("25:00"));
        assert!(raw.len() < 256, "status.json must stay tiny, got {raw}");
        let idle = idle_payload();
        let v: serde_json::Value = serde_json::from_str(&idle).expect("idle json");
        assert_eq!(v["text"], "");
        assert_eq!(v["class"], "idle");
        assert_eq!(
            super::status_path().file_name().and_then(|n| n.to_str()),
            Some("status.json")
        );
    }

    #[test]
    fn mutex_remaining_not_a_file_poll() {
        let _g = LOCK.lock().expect("lock");
        reset();
        assert!(remaining().is_none());
        start(FOCUS_SECS, "focus");
        let (kind, secs) = remaining().expect("running");
        assert_eq!(kind, SessionKind::Focus);
        assert!(
            (24 * 60..25 * 60 + 1).contains(&secs),
            "remaining should be about 25m, got {secs}"
        );
        start(BREAK_SECS, "break");
        let (kind, secs) = remaining().expect("break");
        assert_eq!(kind, SessionKind::Break);
        assert!(
            (4 * 60..5 * 60 + 1).contains(&secs),
            "break remaining should be about 5m, got {secs}"
        );
        stop();
        assert!(remaining().is_none());
        match tick() {
            Tick::Done { .. } => {}
            Tick::Running { text } => panic!("idle tick must stop the timeout, got {text}"),
        }
        reset();
    }

    #[test]
    fn status_file_is_mode_600_json() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "flint-status-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("status.json");
        crate::paths::write_private(&path, status_payload(SessionKind::Break, 5 * 60))
            .expect("write");
        let raw = std::fs::read_to_string(&path).expect("read");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
        assert_eq!(v["text"], "05:00");
        assert_eq!(v["class"], "break");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
