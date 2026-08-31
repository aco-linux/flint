use std::fs;
use std::path::{Path, PathBuf};

use crate::item::{Action, Item, Kind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    Text,
    Document,
    Other,
}

#[derive(Debug, Clone)]
pub enum Preview {
    None,
    Text(String),
    Image(PathBuf),
    Media { hint: String },
}

pub fn classify(path: &Path) -> MediaKind {
    if path.is_dir() {
        return MediaKind::Other;
    }
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "avif" | "bmp" | "heic" => {
            MediaKind::Image
        }
        "mp3" | "flac" | "wav" | "opus" | "ogg" | "m4a" | "aac" => MediaKind::Audio,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => MediaKind::Video,
        "md" | "markdown" | "mdx" | "txt" | "rst" | "log" | "json" | "toml" | "yaml" | "yml"
        | "rs" | "py" | "js" | "ts" | "go" | "c" | "h" | "css" | "html" | "sh" => MediaKind::Text,
        "pdf" | "odt" | "doc" | "docx" | "rtf" | "pages" => MediaKind::Document,
        _ => MediaKind::Other,
    }
}

pub fn is_playable(kind: MediaKind) -> bool {
    matches!(kind, MediaKind::Audio | MediaKind::Video)
}

pub fn for_item(item: &Item) -> Preview {
    match item.kind {
        Kind::Weather => {
            if item.subtitle.contains("Detecting") {
                Preview::Text("Looking up the weather for your location…".into())
            } else {
                Preview::Text(format!(
                    "{}\n{}\n\nEnter copies the summary.",
                    item.title, item.subtitle
                ))
            }
        }
        Kind::Calc | Kind::Ai => {
            if item.subtitle.is_empty() {
                Preview::None
            } else {
                Preview::Text(item.subtitle.clone())
            }
        }
        _ => match &item.action {
            Action::OpenPath(path) | Action::PlayMedia { path } => for_path(path),
            Action::Copy(text) if text.chars().count() > 24 => {
                Preview::Text(text.chars().take(800).collect())
            }
            _ => Preview::None,
        },
    }
}

pub fn for_path(path: &Path) -> Preview {
    if !path.exists() {
        return Preview::None;
    }
    if path.is_dir() {
        return Preview::Text(list_dir_preview(path));
    }
    match classify(path) {
        MediaKind::Image => {
            if file_too_heavy(path) {
                Preview::Text("Image is large — Enter opens it in your viewer.".into())
            } else {
                Preview::Image(path.to_path_buf())
            }
        }
        MediaKind::Audio => Preview::Media {
            hint: format!(
                "▶  {}\n\nEnter starts playback in your default player.",
                path.display()
            ),
        },
        MediaKind::Video => Preview::Media {
            hint: format!("▶  {}\n\nEnter plays this video.", path.display()),
        },
        MediaKind::Text => Preview::Text(read_head(path, 8 * 1024)),
        MediaKind::Document => Preview::Text(format!(
            "{}\n\nEnter opens this document in your editor.",
            path.display()
        )),
        MediaKind::Other => Preview::Text(path.display().to_string()),
    }
}

fn list_dir_preview(path: &Path) -> String {
    let Ok(entries) = fs::read_dir(path) else {
        return path.display().to_string();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    let shown = names.iter().take(16).cloned().collect::<Vec<_>>();
    let more = names.len().saturating_sub(shown.len());
    let mut body = format!("{}\n\n", path.display());
    body.push_str(&shown.join("\n"));
    if more > 0 {
        body.push_str(&format!("\n… {more} more"));
    }
    body
}

pub fn snippet(path: &Path, chars: usize) -> String {
    read_head(path, 8 * 1024).chars().take(chars).collect()
}

fn read_head(path: &Path, bytes: usize) -> String {
    let Ok(data) = fs::read(path) else {
        return path.display().to_string();
    };
    let slice = if data.len() > bytes {
        &data[..bytes]
    } else {
        &data
    };
    let mut text = String::from_utf8_lossy(slice).into_owned();
    if data.len() > bytes {
        text.push_str("\n…");
    }
    if text.trim().is_empty() {
        path.display().to_string()
    } else {
        text
    }
}

fn file_too_heavy(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.len() > 12 * 1024 * 1024)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::{MediaKind, classify};
    use std::path::Path;

    #[test]
    fn classifies_common_media() {
        assert_eq!(classify(Path::new("song.mp3")), MediaKind::Audio);
        assert_eq!(classify(Path::new("clip.mkv")), MediaKind::Video);
        assert_eq!(classify(Path::new("shot.PNG")), MediaKind::Image);
        assert_eq!(classify(Path::new("notes.md")), MediaKind::Text);
        assert_eq!(classify(Path::new("brief.pdf")), MediaKind::Document);
    }
}
