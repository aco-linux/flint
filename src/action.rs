use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use gtk4::gdk::prelude::DisplayExt;

use crate::item::Action;

/// Space on Audio/Video runs the same play action as Enter and must be consumed.
pub fn spacebar_play(item: &crate::item::Item) -> Option<Action> {
    match &item.action {
        Action::PlayMedia { path } => Some(Action::PlayMedia { path: path.clone() }),
        Action::OpenPath(path) if crate::preview::is_playable(crate::preview::classify(path)) => {
            Some(Action::PlayMedia { path: path.clone() })
        }
        _ => None,
    }
}

pub fn run(action: &Action) {
    match action {
        Action::LaunchDesktop { path } => launch_desktop(path),
        Action::FocusWindow { address } => {
            let _ = detach(
                "hyprctl",
                &["dispatch", "focuswindow", &format!("address:{address}")],
            );
        }
        Action::Copy(text) => copy_text(text),
        Action::Paste(text) => paste_text(text),
        Action::OpenUri(uri) => {
            if !is_safe_uri(uri) {
                return;
            }
            let _ =
                gtk4::gio::AppInfo::launch_default_for_uri(uri, gtk4::gio::AppLaunchContext::NONE);
        }
        Action::OpenPath(path) => open_path(path),
        Action::PlayMedia { path } => play_media(path),
        Action::Spawn { program, args } => {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let _ = detach(program, &args);
        }
        Action::Shell { command, terminal } => {
            if *terminal {
                run_in_terminal(command);
            } else {
                let _ = detach("sh", &["-c", command]);
            }
        }
        Action::RunScript { path } => run_script(path),
        Action::TypeText(text) => {
            let _ = type_text(text);
        }
        Action::EnterMode(_)
        | Action::SaveSnippet { .. }
        | Action::CreateNote { .. }
        | Action::OpenNote { .. }
        | Action::AskAi { .. }
        | Action::AskSelection { .. }
        | Action::ResumeThread { .. }
        | Action::NewChat
        | Action::Remember { .. }
        | Action::ForgetMemory { .. }
        | Action::ShowMemory
        | Action::AttachClipboard
        | Action::AttachSelected
        | Action::AttachPath { .. }
        | Action::ShareRegion
        | Action::ShareScreen
        | Action::ToggleVoice
        | Action::DictateFocused
        | Action::NoteFromSelection
        | Action::StartFocus { .. }
        | Action::StopFocus
        | Action::SaveSettings
        | Action::InstallExt { .. }
        | Action::SyncScriptCommands
        | Action::SyncVicinae
        | Action::UseModel { .. }
        | Action::SignIn { .. }
        | Action::SignOut
        | Action::RefreshModels
        | Action::LaunchExtension { .. }
        | Action::Extension { .. }
        | Action::SaveQuicklink { .. }
        | Action::SaveLayout { .. }
        | Action::QuitAll
        | Action::Confetti => {}
        Action::Layout { name, address } => crate::layout::apply(name, address.as_deref()),
        Action::CloseWindow { address } => crate::quit::close_window(address),
        Action::KillPid { pid } => crate::quit::kill_pid(*pid),
        Action::QuitClass { class } => crate::quit::quit_class(class),
        Action::ConfirmQuitAll => crate::quit::quit_all(),
        Action::Uninstall { manager, package } => crate::pkg::uninstall(manager, package),
        Action::Capture { kind } => crate::capture::run(kind),
        Action::SetResolution { spec } => {
            let _ = crate::hypr::keyword(&format!("monitor {spec}"));
        }
        Action::Ocr { path } => crate::ocr::run_ocr(path.as_deref()),
        Action::Qr { path } => crate::ocr::run_qr(path.as_deref()),
    }
}

fn run_script(path: &Path) {
    let Some(path_str) = path.to_str() else {
        return;
    };
    if !is_under_store(path) {
        return;
    }
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext == "py")
    {
        let _ = detach("python3", &[path_str]);
        return;
    }
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| matches!(ext, "js" | "ts"))
    {
        let _ = detach("node", &[path_str]);
        return;
    }
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| matches!(ext, "sh" | "bash"))
        || path_str.ends_with(".sh")
    {
        let _ = detach("sh", &[path_str]);
    }
}

fn launch_desktop(path: &Path) {
    if let Some(path) = path.to_str()
        && detach("gio", &["launch", path]).is_ok()
    {
        return;
    }
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if !id.is_empty() {
        let _ = detach("gtk-launch", &[id]);
    }
}

fn play_media(path: &Path) {
    let Some(path_str) = path.to_str() else {
        return;
    };
    for (bin, prefix) in [
        ("mpv", &["--force-window=immediate"][..]),
        ("vlc", &[][..]),
        ("ffplay", &["-autoexit"][..]),
    ] {
        if which(bin) {
            let mut args: Vec<&str> = prefix.to_vec();
            args.push(path_str);
            let _ = detach(bin, &args);
            return;
        }
    }
    open_path(path);
}

fn open_path(path: &Path) {
    match glib::filename_to_uri(path, None) {
        Ok(uri) => {
            let _ =
                gtk4::gio::AppInfo::launch_default_for_uri(&uri, gtk4::gio::AppLaunchContext::NONE);
        }
        Err(_) => {
            let _ = detach("xdg-open", &[path.to_string_lossy().as_ref()]);
        }
    }
}

pub fn copy_text(text: &str) {
    if let Some(display) = gtk4::gdk::Display::default() {
        display.clipboard().set_text(text);
    }
    let _ = Command::new("wl-copy")
        .arg("--")
        .arg(text)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn paste_text(text: &str) {
    copy_text(text);
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(140));
        if which("wtype") {
            let _ = Command::new("wtype")
                .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    });
}

pub fn wtype_available() -> bool {
    which("wtype")
}

/// Type `text` with `wtype --` as argv (never a shell, never ydotool).
/// Returns false when wtype is missing; the text is copied instead.
pub fn type_text(text: &str) -> bool {
    if !which("wtype") {
        copy_text(text);
        return false;
    }
    let args = wtype_args(text);
    let _ = Command::new("wtype")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    true
}

/// `wtype -- <text>` so a leading dash in the transcript cannot be an option.
pub(crate) fn wtype_args(text: &str) -> [&str; 2] {
    ["--", text]
}

fn run_in_terminal(command: &str) {
    let wrapped = format!("{command}; echo; read -n 1 -s -r -p 'Press any key to close'");
    let candidates: &[(&str, &[&str])] = &[
        ("xdg-terminal-exec", &["sh", "-lc"]),
        ("ghostty", &["-e", "sh", "-lc"]),
        ("kitty", &["sh", "-lc"]),
        ("foot", &["sh", "-lc"]),
        ("alacritty", &["-e", "sh", "-lc"]),
        ("wezterm", &["start", "--", "sh", "-lc"]),
    ];
    for (bin, prefix) in candidates {
        if which(bin) {
            let mut args: Vec<&str> = prefix.to_vec();
            args.push(&wrapped);
            let _ = detach(bin, &args);
            return;
        }
    }
    let _ = detach("sh", &["-c", command]);
}

fn detach(program: &str, args: &[&str]) -> std::io::Result<std::process::Child> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

pub(crate) fn is_safe_uri(uri: &str) -> bool {
    let uri = uri.trim();
    if uri.is_empty() || uri.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let lower = uri.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("file://")
}

fn is_under_store(path: &Path) -> bool {
    let store = crate::paths::data_dir().join("store");
    let Ok(store) = store.canonicalize() else {
        return false;
    };
    match path.canonicalize() {
        Ok(real) => real.starts_with(&store),
        Err(_) => {
            path.starts_with(crate::paths::data_dir().join("store"))
                && !path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_safe_uri, spacebar_play};
    use crate::item::{Action, Icon, Item, Kind};
    use std::path::PathBuf;

    #[test]
    fn spacebar_plays_audio_and_video_and_ignores_text() {
        let audio = Item {
            id: "file:/tmp/song.mp3".into(),
            title: "song.mp3".into(),
            subtitle: String::new(),
            keywords: String::new(),
            kind: Kind::Media,
            icon: Icon::None,
            action: Action::PlayMedia {
                path: PathBuf::from("/tmp/song.mp3"),
            },
        };
        let video = Item {
            id: "file:/tmp/clip.mp4".into(),
            title: "clip.mp4".into(),
            subtitle: String::new(),
            keywords: String::new(),
            kind: Kind::Media,
            icon: Icon::None,
            action: Action::PlayMedia {
                path: PathBuf::from("/tmp/clip.mp4"),
            },
        };
        let note = Item {
            id: "file:/tmp/readme.md".into(),
            title: "readme.md".into(),
            subtitle: String::new(),
            keywords: String::new(),
            kind: Kind::File,
            icon: Icon::None,
            action: Action::OpenPath(PathBuf::from("/tmp/readme.md")),
        };
        assert!(matches!(
            spacebar_play(&audio),
            Some(Action::PlayMedia { path }) if path.ends_with("song.mp3")
        ));
        assert!(matches!(
            spacebar_play(&video),
            Some(Action::PlayMedia { path }) if path.ends_with("clip.mp4")
        ));
        assert!(
            spacebar_play(&note).is_none(),
            "space in a text query must still insert a space"
        );
    }

    #[test]
    fn wtype_uses_argv_not_a_shell() {
        let args = super::wtype_args("hello --world");
        assert_eq!(args, ["--", "hello --world"]);
        assert_ne!(args[0], "-c");
        assert!(!args.iter().any(|a| a.contains('|')));
    }

    #[test]
    fn rejects_dangerous_uri_schemes() {
        assert!(is_safe_uri("https://example.com"));
        assert!(is_safe_uri("http://127.0.0.1:11434"));
        assert!(is_safe_uri("file:///home/aco/notes.md"));
        assert!(!is_safe_uri("javascript:alert(1)"));
        assert!(!is_safe_uri("file:///etc/passwd\njavascript:x"));
        assert!(!is_safe_uri("smb://evil"));
        assert!(!is_safe_uri(""));
    }
}
