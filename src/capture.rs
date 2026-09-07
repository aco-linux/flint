use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::item::{Action, Icon, Item, Kind};
use crate::paths;

pub fn items(which: impl Fn(&str) -> bool) -> Vec<Item> {
    let mut items = Vec::new();
    if which("grim") {
        items.push(capture_item(
            "shot",
            "Screenshot",
            "Capture the current screen",
            "screenshot",
        ));
        if which("slurp") {
            items.push(capture_item(
                "region",
                "Capture region",
                "Select an area to screenshot",
                "region",
            ));
        }
        if which("satty") || which("swappy") {
            items.push(capture_item(
                "annotate",
                "Annotate screenshot",
                "Capture a region and mark it up",
                "annotate",
            ));
        }
    } else {
        if which("omarchy-capture-screenshot") {
            items.push(cmd(
                "shot",
                "Screenshot",
                "Capture the current screen",
                Action::Spawn {
                    program: "omarchy-capture-screenshot".into(),
                    args: Vec::new(),
                },
            ));
        }
        if which("omarchy-capture-region") {
            items.push(cmd(
                "region",
                "Capture region",
                "Select an area to screenshot",
                Action::Spawn {
                    program: "omarchy-capture-region".into(),
                    args: Vec::new(),
                },
            ));
        }
    }
    if which("wf-recorder") {
        items.push(capture_item(
            "record",
            "Screen recording",
            "Start or stop recording with wf-recorder",
            "record",
        ));
    }
    items
}

fn capture_item(id: &str, title: &str, subtitle: &str, kind: &str) -> Item {
    cmd(
        id,
        title,
        subtitle,
        Action::Capture {
            kind: kind.to_string(),
        },
    )
}

fn cmd(id: &str, title: &str, subtitle: &str, action: Action) -> Item {
    Item {
        id: format!("cmd:{id}"),
        title: title.into(),
        subtitle: subtitle.into(),
        keywords: "screenshot capture record annotate grim".into(),
        kind: Kind::Command,
        icon: Icon::Name("applets-screenshooter".into()),
        action,
    }
}

pub fn screenshot_filename(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    min: i32,
    sec: i32,
) -> String {
    format!("flint-{year:04}{month:02}{day:02}-{hour:02}{min:02}{sec:02}.png")
}

pub fn recording_filename(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    min: i32,
    sec: i32,
) -> String {
    format!("flint-{year:04}{month:02}{day:02}-{hour:02}{min:02}{sec:02}.mp4")
}

pub fn run(kind: &str) {
    let kind = kind.to_string();
    thread::spawn(move || run_sync(&kind));
}

/// Capture once after the caller hides Flint. Never a spy or idle timer.
pub fn shot_to_path(region: bool) -> Option<PathBuf> {
    if !which("grim") {
        return None;
    }
    let geom = if region { slurp()? } else { String::new() };
    let path = screenshot_dir().join(screenshot_filename_now());
    let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    let mut cmd = Command::new("grim");
    if region {
        cmd.args(["-g", &geom]);
    }
    let ok = cmd
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .is_some_and(|s| s.success());
    if ok && path.exists() {
        Some(path)
    } else {
        None
    }
}

fn run_sync(kind: &str) {
    match kind {
        "screenshot" => {
            let path = screenshot_dir().join(screenshot_filename_now());
            let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
            let _ = Command::new("grim")
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        "region" => {
            let Some(geom) = slurp() else {
                return;
            };
            let path = screenshot_dir().join(screenshot_filename_now());
            let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
            let _ = Command::new("grim")
                .args(["-g", &geom])
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        "annotate" => {
            let Some(geom) = slurp() else {
                return;
            };
            let path = screenshot_dir().join(screenshot_filename_now());
            let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
            let ok = Command::new("grim")
                .args(["-g", &geom])
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .ok()
                .is_some_and(|s| s.success());
            if !ok {
                return;
            }
            if which("satty") {
                let _ = Command::new("satty")
                    .args(["--filename"])
                    .arg(&path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            } else if which("swappy") {
                let _ = Command::new("swappy")
                    .arg("-f")
                    .arg(&path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
        }
        "record" => toggle_record(),
        _ => {}
    }
}

fn slurp() -> Option<String> {
    let output = Command::new("slurp")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let geom = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if geom.is_empty() { None } else { Some(geom) }
}

fn toggle_record() {
    if stop_record() {
        return;
    }
    let path = video_dir().join(recording_filename_now());
    let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    let Ok(child) = Command::new("wf-recorder")
        .arg("-f")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    write_lock(child.id());
}

fn stop_record() -> bool {
    let Some(lock) = read_lock() else {
        return false;
    };
    if !recorder_is_ours(&lock) {
        let _ = fs::remove_file(pid_file());
        return false;
    }
    // SAFETY: lock.pid is > 1 and currently matches this machine's wf-recorder
    // (comm + starttime, or a legacy pid-only file whose comm is still
    // wf-recorder). kill(2) is a syscall; ESRCH/EPERM are ignored.
    unsafe {
        let _ = libc::kill(lock.pid, libc::SIGINT);
    }
    let _ = fs::remove_file(pid_file());
    true
}

fn pid_file() -> PathBuf {
    paths::runtime_dir().join("wf-recorder.pid")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecorderLock {
    pid: i32,
    starttime: Option<u64>,
}

fn write_lock(pid: u32) {
    paths::ensure();
    let pid = pid as i32;
    if let Ok(mut file) = fs::File::create(pid_file()) {
        match proc_starttime(pid).filter(|t| *t > 0) {
            Some(starttime) => {
                let _ = writeln!(file, "{pid} {starttime}");
            }
            None => {
                let _ = writeln!(file, "{pid}");
            }
        }
        let _ = fs::set_permissions(pid_file(), fs::Permissions::from_mode(0o600));
    }
}

fn read_lock() -> Option<RecorderLock> {
    parse_recorder_lock(&fs::read_to_string(pid_file()).ok()?)
}

fn parse_recorder_lock(text: &str) -> Option<RecorderLock> {
    let mut parts = text.split_whitespace();
    let pid: i32 = parts.next()?.parse().ok()?;
    if pid <= 1 {
        return None;
    }
    let starttime = parts.next().and_then(|s| s.parse().ok());
    Some(RecorderLock { pid, starttime })
}

fn recorder_is_ours(lock: &RecorderLock) -> bool {
    recorder_matches(lock, &proc_comm(lock.pid), proc_starttime(lock.pid))
}

fn recorder_matches(lock: &RecorderLock, comm: &str, starttime: Option<u64>) -> bool {
    if comm.trim() != "wf-recorder" {
        return false;
    }
    match lock.starttime {
        Some(want) if want > 0 => starttime == Some(want),
        Some(_) => false,
        None => true,
    }
}

fn proc_comm(pid: i32) -> String {
    fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default()
}

fn proc_starttime(pid: i32) -> Option<u64> {
    parse_proc_starttime(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

fn parse_proc_starttime(stat: &str) -> Option<u64> {
    let after_comm = stat.rsplit_once(')')?.1;
    after_comm.split_whitespace().nth(19)?.parse().ok()
}

fn screenshot_dir() -> PathBuf {
    let pictures = dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .unwrap_or_else(|| PathBuf::from("Pictures"));
    pictures.join("Screenshots")
}

fn video_dir() -> PathBuf {
    dirs::video_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Videos")))
        .unwrap_or_else(|| PathBuf::from("Videos"))
}

fn screenshot_filename_now() -> String {
    let t = local_parts().unwrap_or([1970, 1, 1, 0, 0, 0]);
    screenshot_filename(t[0], t[1], t[2], t[3], t[4], t[5])
}

fn recording_filename_now() -> String {
    let t = local_parts().unwrap_or([1970, 1, 1, 0, 0, 0]);
    recording_filename(t[0], t[1], t[2], t[3], t[4], t[5])
}

fn local_parts() -> Option<[i32; 6]> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as libc::time_t;
    // SAFETY: `tm` is written by localtime_r before we read it.
    unsafe {
        let mut tm = std::mem::zeroed::<libc::tm>();
        if libc::localtime_r(&ts, &mut tm).is_null() {
            return None;
        }
        Some([
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
        ])
    }
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        RecorderLock, items, parse_proc_starttime, parse_recorder_lock, recorder_matches,
        recording_filename, screenshot_filename,
    };

    #[test]
    fn filename_pattern() {
        assert_eq!(
            screenshot_filename(2026, 9, 6, 14, 5, 7),
            "flint-20260906-140507.png"
        );
        assert_eq!(
            recording_filename(2026, 9, 6, 14, 5, 7),
            "flint-20260906-140507.mp4"
        );
    }

    #[test]
    fn commands_omitted_when_binary_missing() {
        let none = items(|_| false);
        assert!(none.is_empty(), "no capture tools on PATH");

        let grim_only = items(|bin| bin == "grim");
        let ids: Vec<&str> = grim_only.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["cmd:shot"]);
        assert!(
            !ids.iter().any(|id| id.contains("share")),
            "screen share with AI is an explicit Ask command, not capture idle"
        );
        assert!(
            grim_only
                .iter()
                .all(|i| matches!(i.action, crate::item::Action::Capture { .. }))
        );

        let grim_slurp = items(|bin| matches!(bin, "grim" | "slurp" | "satty" | "wf-recorder"));
        let ids: Vec<&str> = grim_slurp.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            ["cmd:shot", "cmd:region", "cmd:annotate", "cmd:record"]
        );

        let omarchy = items(|bin| bin.starts_with("omarchy-capture-"));
        let ids: Vec<&str> = omarchy.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["cmd:shot", "cmd:region"]);
        assert!(
            omarchy
                .iter()
                .all(|i| matches!(i.action, crate::item::Action::Spawn { .. }))
        );
    }

    #[test]
    fn parse_recorder_lock_reads_pid_and_starttime() {
        let lock = parse_recorder_lock("4321 98765\n").unwrap();
        assert_eq!(lock.pid, 4321);
        assert_eq!(lock.starttime, Some(98765));
        let legacy = parse_recorder_lock("99\n").unwrap();
        assert_eq!(legacy.pid, 99);
        assert_eq!(legacy.starttime, None);
        assert!(parse_recorder_lock("1 1\n").is_none());
        assert!(parse_recorder_lock("not-a-pid\n").is_none());
    }

    #[test]
    fn recorder_matches_requires_wf_recorder_identity() {
        let lock = RecorderLock {
            pid: 42,
            starttime: Some(100),
        };
        assert!(recorder_matches(&lock, "wf-recorder\n", Some(100)));
        assert!(
            !recorder_matches(&lock, "bash\n", Some(100)),
            "reused PID of an unrelated process must not be signalled"
        );
        assert!(
            !recorder_matches(&lock, "wf-recorder\n", Some(101)),
            "same name after PID reuse must not match a different start time"
        );
        assert!(
            !recorder_matches(
                &RecorderLock {
                    pid: 42,
                    starttime: Some(0),
                },
                "wf-recorder\n",
                Some(100)
            ),
            "starttime 0 is not a wildcard"
        );
        let legacy = RecorderLock {
            pid: 42,
            starttime: None,
        };
        assert!(recorder_matches(&legacy, "wf-recorder\n", Some(9)));
        assert!(!recorder_matches(&legacy, "firefox\n", Some(9)));
    }

    #[test]
    fn parse_proc_starttime_uses_field_22() {
        let stat = "4242 (wf-recorder) S 1 4242 4242 0 -1 4194304 10 0 0 0 1 2 0 0 20 0 1 0 999888 123456 80 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        assert_eq!(parse_proc_starttime(stat), Some(999888));
        assert!(parse_proc_starttime("no-paren-stat").is_none());
    }
}
