use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::item::{Action, Icon, Item, Kind};
use crate::paths;
use crate::preview;

pub fn items(which: impl Fn(&str) -> bool) -> Vec<Item> {
    let mut items = Vec::new();
    if which("tesseract") {
        items.push(Item {
            id: "cmd:ocr-clip".into(),
            title: "OCR clipboard image".into(),
            subtitle: "Read text from a PNG on the clipboard".into(),
            keywords: "ocr tesseract image text screenshot".into(),
            kind: Kind::Command,
            icon: Icon::Name("accessories-character-map".into()),
            action: Action::Ocr { path: None },
        });
    }
    if which("zbarimg") {
        items.push(Item {
            id: "cmd:qr-clip".into(),
            title: "Decode clipboard QR".into(),
            subtitle: "Read a QR code from a PNG on the clipboard".into(),
            keywords: "qr barcode zbar image".into(),
            kind: Kind::Command,
            icon: Icon::Name("view-barcode".into()),
            action: Action::Qr { path: None },
        });
    }
    items
}

pub fn actions_for_path(path: &Path) -> Vec<(String, Action)> {
    if preview::classify(path) != preview::MediaKind::Image {
        return Vec::new();
    }
    let mut out = Vec::new();
    if command_exists("tesseract") {
        out.push((
            "Read text (OCR)".into(),
            Action::Ocr {
                path: Some(path.to_path_buf()),
            },
        ));
    }
    if command_exists("zbarimg") {
        out.push((
            "Decode QR".into(),
            Action::Qr {
                path: Some(path.to_path_buf()),
            },
        ));
    }
    out
}

pub fn run_ocr(path: Option<&Path>) {
    run_tool(path, ocr_output);
}

pub fn run_qr(path: Option<&Path>) {
    run_tool(path, qr_output);
}

fn run_tool(path: Option<&Path>, read: impl Fn(&Path) -> Option<String>) {
    match path {
        Some(path) => {
            if let Some(text) = read(path).filter(|t| !t.is_empty()) {
                crate::action::copy_text(&text);
            }
        }
        None => {
            let Some(temp) = dump_clipboard_png() else {
                return;
            };
            if let Some(text) = read(&temp).filter(|t| !t.is_empty()) {
                crate::action::copy_text(&text);
            }
            let _ = std::fs::remove_file(&temp);
        }
    }
}

pub fn tesseract_args(path: &Path) -> [&std::ffi::OsStr; 2] {
    [path.as_os_str(), std::ffi::OsStr::new("stdout")]
}

pub fn zbarimg_args(path: &Path) -> [&std::ffi::OsStr; 3] {
    [
        std::ffi::OsStr::new("--quiet"),
        std::ffi::OsStr::new("--raw"),
        path.as_os_str(),
    ]
}

fn ocr_output(path: &Path) -> Option<String> {
    if !command_exists("tesseract") {
        return None;
    }
    let args = tesseract_args(path);
    let output = Command::new("tesseract")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn qr_output(path: &Path) -> Option<String> {
    if !command_exists("zbarimg") {
        return None;
    }
    let args = zbarimg_args(path);
    let output = Command::new("zbarimg")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn dump_clipboard_png() -> Option<PathBuf> {
    paths::ensure();
    let output = Command::new("wl-paste")
        .args(["--type", "image/png"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    let path = paths::runtime_dir().join("ocr-clipboard.png");
    paths::write_private(&path, &output.stdout).ok()?;
    Some(path)
}

fn command_exists(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{actions_for_path, items, tesseract_args, zbarimg_args};
    use crate::item::Action;
    use std::path::Path;

    #[test]
    fn missing_binaries_omit_commands() {
        assert!(items(|_| false).is_empty());
        let with_tess = items(|bin| bin == "tesseract");
        assert_eq!(with_tess.len(), 1);
        assert!(matches!(with_tess[0].action, Action::Ocr { path: None }));
        let with_zbar = items(|bin| bin == "zbarimg");
        assert_eq!(with_zbar.len(), 1);
        assert!(matches!(with_zbar[0].action, Action::Qr { path: None }));
    }

    #[test]
    fn argv_is_not_a_shell_string() {
        let path = Path::new("/tmp/foo bar.png");
        let tess = tesseract_args(path);
        assert_eq!(tess[0], path.as_os_str());
        assert_eq!(tess[1], "stdout");
        assert!(
            !tess
                .iter()
                .any(|arg| arg.to_string_lossy().contains("tesseract "))
        );
        let zbar = zbarimg_args(path);
        assert_eq!(zbar[2], path.as_os_str());
        assert_eq!(zbar[0], "--quiet");
        assert_eq!(zbar[1], "--raw");
    }

    #[test]
    fn image_actions_follow_path_binaries() {
        let text = Path::new("/tmp/readme.md");
        assert!(actions_for_path(text).is_empty());
    }
}
