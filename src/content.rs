use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::files;
use crate::item::{Action, Icon, Item, Kind};
use crate::mode::Mode;

const LIMIT: usize = 40;
const SNIPPET_BYTES: u64 = 64 * 1024;
const SNIPPET_CHARS: usize = 120;

/// Explicit content search only. Ordinary root typing never reaches ripgrep.
pub fn term_from_query(query: &str) -> Option<String> {
    let (mode, rest) = Mode::parse(query);
    match mode {
        Mode::Content => nonempty(&rest),
        Mode::Files => files_content_term(&rest),
        _ => root_prefix(&rest),
    }
}

fn files_content_term(rest: &str) -> Option<String> {
    let rest = rest.trim();
    after_prefix(rest, "content:")
        .or_else(|| after_prefix(rest, "content "))
        .or_else(|| after_prefix(rest, "in:"))
        .and_then(nonempty)
}

fn root_prefix(rest: &str) -> Option<String> {
    let rest = rest.trim();
    after_prefix(rest, "in:")
        .or_else(|| after_prefix(rest, "content:"))
        .and_then(nonempty)
}

fn after_prefix<'a>(raw: &'a str, prefix: &str) -> Option<&'a str> {
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with(prefix) {
        Some(raw[prefix.len()..].trim())
    } else {
        None
    }
}

fn nonempty(term: &str) -> Option<String> {
    let term = term.trim();
    if term.is_empty() {
        None
    } else {
        Some(term.to_string())
    }
}

pub fn search(term: &str, extra_roots: &[String], limit: usize) -> Vec<Item> {
    let roots = content_roots(extra_roots);
    search_in(&roots, term, limit.min(LIMIT))
}

pub(crate) fn content_roots(extra_roots: &[String]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir().filter(|path| path != Path::new("/")) {
        roots.push(home);
    }
    for raw in extra_roots {
        let expanded = files::expand_tilde(raw.trim());
        if expanded.is_empty() {
            continue;
        }
        let path = PathBuf::from(expanded);
        if path == Path::new("/") {
            continue;
        }
        if path.is_dir() && !roots.contains(&path) {
            roots.push(path);
        }
    }
    roots
}

fn search_in(roots: &[PathBuf], term: &str, limit: usize) -> Vec<Item> {
    if term.is_empty() || limit == 0 || !command_exists("rg") {
        return Vec::new();
    }
    if roots.is_empty() || files::cancelled() {
        return Vec::new();
    }
    let mut cmd = Command::new("rg");
    cmd.args([
        "-l",
        "-i",
        "--max-count",
        "1",
        "--color=never",
        "--no-config",
        "--no-heading",
    ]);
    for exclude in files::EXCLUDES {
        cmd.arg("-g");
        cmd.arg(format!("!**/{exclude}/**"));
    }
    cmd.arg("-e");
    cmd.arg(term);
    for root in roots {
        cmd.arg(root);
    }
    let Some(output) = files::run_attached(cmd) else {
        return Vec::new();
    };
    if files::cancelled() {
        return Vec::new();
    }
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if files::cancelled() {
            break;
        }
        if line.is_empty() {
            continue;
        }
        let path = PathBuf::from(line);
        if path.as_os_str() == "/" || !seen.insert(path.clone()) {
            continue;
        }
        items.push(hit_item(path, term));
        if items.len() >= limit {
            break;
        }
    }
    items
}

fn hit_item(path: PathBuf, term: &str) -> Item {
    let snippet = snippet_line(&path, term);
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let location = path.to_string_lossy().to_string();
    let subtitle = if snippet.is_empty() {
        location
    } else {
        format!("{location}  ·  {snippet}")
    };
    Item {
        id: format!("file:{}", path.display()),
        title: name,
        subtitle,
        keywords: "content search rg".into(),
        kind: Kind::File,
        icon: Icon::Name("text-x-generic".into()),
        action: Action::OpenPath(path),
    }
}

fn snippet_line(path: &Path, needle: &str) -> String {
    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut buf = String::new();
    let _ = file.take(SNIPPET_BYTES).read_to_string(&mut buf);
    let needle = needle.to_ascii_lowercase();
    for line in buf.lines() {
        if line.to_ascii_lowercase().contains(&needle) {
            return collapse_ws(line).chars().take(SNIPPET_CHARS).collect();
        }
    }
    String::new()
}

fn collapse_ws(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn command_exists(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{content_roots, files_content_term, root_prefix, search_in, term_from_query};
    use std::fs;
    use std::path::Path;

    #[test]
    fn root_needs_colon_prefix() {
        assert_eq!(term_from_query("firefox"), None);
        assert_eq!(term_from_query("markdown"), None);
        assert_eq!(term_from_query("contentment"), None);
        assert_eq!(term_from_query("in foo"), None);
        assert_eq!(term_from_query("content:"), None);
        assert_eq!(term_from_query("in:"), None);
        assert_eq!(
            term_from_query("content: hello world").as_deref(),
            Some("hello world")
        );
        assert_eq!(term_from_query("CONTENT:Secret").as_deref(), Some("Secret"));
        assert_eq!(term_from_query("in:token").as_deref(), Some("token"));
        assert_eq!(term_from_query("in: needle").as_deref(), Some("needle"));
    }

    #[test]
    fn files_mode_accepts_content_space() {
        assert_eq!(
            term_from_query("file content invoices").as_deref(),
            Some("invoices")
        );
        assert_eq!(
            term_from_query("file content:api_key").as_deref(),
            Some("api_key")
        );
        assert_eq!(term_from_query("file in:secret").as_deref(), Some("secret"));
        assert_eq!(term_from_query("file invoices"), None);
        assert_eq!(files_content_term("content foo").as_deref(), Some("foo"));
        assert_eq!(root_prefix("in:bar").as_deref(), Some("bar"));
    }

    #[test]
    fn content_mode_strips_prefix() {
        assert_eq!(
            term_from_query("content foo bar").as_deref(),
            Some("foo bar")
        );
        assert_eq!(term_from_query("content").as_deref(), None);
    }

    #[test]
    fn never_includes_slash() {
        let roots = content_roots(&["/".into(), "/tmp".into()]);
        assert!(roots.iter().all(|path| path != Path::new("/")));
    }

    #[test]
    fn missing_rg_is_no_hits() {
        if super::command_exists("rg") {
            return;
        }
        assert!(super::search("anything", &[], 10).is_empty());
    }

    #[test]
    fn rg_finds_a_line_in_an_extra_root() {
        if !super::command_exists("rg") {
            return;
        }
        let root = std::env::temp_dir().join(format!("flint-content-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let needle = format!("flint-content-needle-{}", std::process::id());
        fs::write(root.join("hit.txt"), format!("alpha {needle} omega\n")).unwrap();
        fs::write(root.join("miss.txt"), "nothing here\n").unwrap();
        let items = search_in(std::slice::from_ref(&root), &needle, 10);
        assert_eq!(items.len(), 1, "got {items:?}");
        assert_eq!(items[0].title, "hit.txt");
        assert!(
            items[0].subtitle.contains(&needle),
            "snippet missing: {}",
            items[0].subtitle
        );
        let _ = fs::remove_dir_all(&root);
    }
}
