use std::fs;
use std::path::Path;

use crate::paths;

const SKILLS_CAP: usize = 12 * 1024;

/// Load `~/.config/flint/skills/*.md` into a system-prompt block.
/// Missing directory is fine. Total cap is 12 KiB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub path: std::path::PathBuf,
    pub preview: String,
}

pub fn dir() -> std::path::PathBuf {
    paths::config_dir().join("skills")
}

pub fn list() -> Vec<Skill> {
    list_in(&dir())
}

pub fn list_in(dir: &Path) -> Vec<Skill> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("md")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with('.'))
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("skill")
                .to_string();
            let preview = text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("")
                .chars()
                .take(80)
                .collect();
            Some(Skill {
                name,
                path,
                preview,
            })
        })
        .collect()
}

pub fn prompt_block() -> Option<String> {
    from_dir(&dir())
}

fn truncate_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub fn from_dir(dir: &Path) -> Option<String> {
    if !dir.is_dir() {
        return None;
    }
    let mut files: Vec<_> = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("md")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with('.'))
        })
        .collect();
    files.sort();
    let mut body = String::from("## Skills\n");
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("skill");
        let chunk = format!("### {name}\n{text}\n");
        if body.len() + chunk.len() > SKILLS_CAP {
            let room = SKILLS_CAP.saturating_sub(body.len());
            if room > 32 {
                body.push_str(truncate_bytes(&chunk, room));
            }
            break;
        }
        body.push_str(&chunk);
    }
    if body.len() <= "## Skills\n".len() {
        None
    } else {
        Some(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{SKILLS_CAP, from_dir};
    use std::fs;

    #[test]
    fn missing_dir_is_fine() {
        let dir = std::env::temp_dir().join(format!(
            "flint-skills-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        assert!(from_dir(&dir).is_none());
    }

    #[test]
    fn concatenates_md_and_caps() {
        let dir = std::env::temp_dir().join(format!(
            "flint-skills-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        fs::write(dir.join("alpha.md"), "Prefer rustfmt.").expect("write");
        fs::write(dir.join("beta.md"), "Never commit secrets.").expect("write");
        fs::write(dir.join("ignore.txt"), "not a skill").expect("write");
        let block = from_dir(&dir).expect("skills");
        assert!(block.contains("## Skills"));
        assert!(block.contains("Prefer rustfmt."));
        assert!(block.contains("Never commit secrets."));
        assert!(!block.contains("not a skill"));
        let listed = super::list_in(&dir);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "alpha");
        assert!(listed[0].preview.contains("rustfmt"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn trims_to_12kib() {
        let dir = std::env::temp_dir().join(format!(
            "flint-skills-cap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let blob = "x".repeat(SKILLS_CAP);
        fs::write(dir.join("huge.md"), &blob).expect("write");
        let block = from_dir(&dir).expect("skills");
        assert!(block.len() <= SKILLS_CAP);
        let _ = fs::remove_dir_all(&dir);
    }
}
