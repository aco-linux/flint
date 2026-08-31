use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use gtk4::gdk::prelude::DisplayExt;

use crate::item::Action;

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
            let _ = gtk4::gio::AppInfo::launch_default_for_uri(
                uri,
                gtk4::gio::AppLaunchContext::NONE,
            );
        }
        Action::OpenPath(path) => open_path(path),
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
        Action::EnterMode(_)
        | Action::SaveSnippet { .. }
        | Action::CreateNote { .. }
        | Action::OpenNote { .. }
        | Action::AskAi { .. }
        | Action::ToggleVoice
        | Action::SaveSettings
        | Action::InstallExt { .. }
        | Action::SyncScriptCommands
        | Action::SyncVicinae
        | Action::UseModel { .. }
        | Action::SignIn { .. }
        | Action::SignOut
        | Action::RefreshModels => {}
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
    if let Some(path) = path.to_str() {
        if detach("gio", &["launch", path]).is_ok() {
            return;
        }
    }
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if !id.is_empty() {
        let _ = detach("gtk-launch", &[id]);
    }
}

fn open_path(path: &Path) {
    match glib::filename_to_uri(path, None) {
        Ok(uri) => {
            let _ = gtk4::gio::AppInfo::launch_default_for_uri(
                &uri,
                gtk4::gio::AppLaunchContext::NONE,
            );
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

fn is_safe_uri(uri: &str) -> bool {
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
        Err(_) => path.starts_with(crate::paths::data_dir().join("store"))
            && !path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
    }
}

#[cfg(test)]
mod tests {
    use super::is_safe_uri;

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
