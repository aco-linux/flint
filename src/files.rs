use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::item::{Action, Icon, Item, Kind};

/// How many files root search can surface when the query looks like a type.
pub const ROOT_FILE_LIMIT: usize = 80;
/// Dedicated Search Files mode — Raycast's Search Files is a long, scrollable list.
pub const FILES_MODE_LIMIT: usize = 250;

const EXCLUDES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".cache",
    ".npm",
    ".cargo",
    ".rustup",
    ".local/share/Trash",
    ".local/share/flatpak",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".gradle",
    "dist",
    "build",
    ".swiftpm",
    ".codex",
];

const SKIP_PREFIXES: &[&str] = &["/proc/", "/sys/", "/dev/", "/run/", "/snap/"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileQuery {
    pub term: String,
    pub extensions: Vec<String>,
    pub type_label: Option<String>,
    pub path_like: bool,
    pub explicit: bool,
}

impl FileQuery {
    pub fn is_type_search(&self) -> bool {
        !self.extensions.is_empty() && self.term.is_empty()
    }

    pub fn wants_files(&self) -> bool {
        self.explicit
            || self.path_like
            || !self.extensions.is_empty()
            || self.term.chars().count() >= 2
    }
}

/// Map everyday words and extensions to the files people actually mean.
/// Raycast/Spotlight do this via UTIs; we do it with an explicit alias table.
pub fn type_alias(word: &str) -> Option<(&'static str, &'static [&'static str])> {
    let key = word
        .trim()
        .trim_start_matches('*')
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    for (label, aliases, exts) in TYPE_ALIASES {
        if aliases.iter().any(|alias| *alias == key) || exts.iter().any(|ext| *ext == key) {
            return Some((label, exts));
        }
    }
    None
}

const TYPE_ALIASES: &[(&str, &[&str], &[&str])] = &[
    (
        "markdown",
        &["markdown", "md", "gfm"],
        &["md", "markdown", "mdown", "mkd", "mdx"],
    ),
    ("pdf", &["pdf", "acrobat"], &["pdf"]),
    (
        "document",
        &["document", "documents", "docs", "word"],
        &["pdf", "odt", "doc", "docx", "rtf", "pages"],
    ),
    ("text", &["text", "txt", "plain"], &["txt", "text", "log"]),
    (
        "spreadsheet",
        &["spreadsheet", "spreadsheets", "excel", "csv", "sheet"],
        &["xlsx", "xls", "csv", "ods", "tsv"],
    ),
    (
        "presentation",
        &[
            "presentation",
            "presentations",
            "slides",
            "powerpoint",
            "ppt",
        ],
        &["pptx", "ppt", "odp", "key"],
    ),
    (
        "image",
        &[
            "image", "images", "photo", "photos", "picture", "pictures", "pic",
        ],
        &[
            "png", "jpg", "jpeg", "webp", "gif", "svg", "avif", "heic", "bmp",
        ],
    ),
    (
        "video",
        &["video", "videos", "movie", "movies", "film"],
        &["mp4", "mkv", "webm", "mov", "avi", "m4v"],
    ),
    (
        "audio",
        &["audio", "music", "song", "songs", "sound"],
        &["mp3", "flac", "wav", "opus", "ogg", "m4a", "aac"],
    ),
    ("javascript", &["javascript", "js"], &["js", "mjs", "cjs"]),
    (
        "typescript",
        &["typescript", "ts"],
        &["ts", "tsx", "mts", "cts"],
    ),
    ("python", &["python", "py"], &["py", "pyi"]),
    ("rust", &["rust", "rs"], &["rs"]),
    ("go", &["golang"], &["go"]),
    ("json", &["json"], &["json", "jsonc"]),
    ("yaml", &["yaml", "yml"], &["yaml", "yml"]),
    ("toml", &["toml"], &["toml"]),
    ("html", &["html", "htm"], &["html", "htm"]),
    ("css", &["css", "stylesheet"], &["css", "scss", "sass"]),
    (
        "code",
        &["code", "source", "sources"],
        &[
            "rs", "py", "js", "ts", "tsx", "go", "c", "h", "cpp", "java", "kt", "swift",
        ],
    ),
    (
        "config",
        &["config", "configs", "configuration", "conf", "ini"],
        &["conf", "cfg", "ini", "toml", "yaml", "yml", "json"],
    ),
    (
        "archive",
        &["archive", "archives", "zip", "tarball"],
        &["zip", "tar", "gz", "tgz", "7z", "rar", "bz2", "xz"],
    ),
    ("log", &["log", "logs"], &["log"]),
    ("csv", &["csv"], &["csv", "tsv"]),
];

pub fn parse_query(raw: &str) -> FileQuery {
    let trimmed = raw.trim();
    let explicit = trimmed.starts_with("file ")
        || trimmed.starts_with("f ")
        || trimmed.starts_with("files ")
        || trimmed.starts_with("find ")
        || trimmed.starts_with("fs ");
    let body = if let Some(rest) = strip_file_prefix(trimmed) {
        rest
    } else {
        trimmed
    };
    let path_like = body.starts_with('/') || body.starts_with("~/") || body.starts_with("./");

    if let Some(query) = parse_filter(body) {
        return FileQuery {
            explicit,
            path_like,
            ..query
        };
    }

    if path_like {
        return FileQuery {
            term: body.to_string(),
            extensions: Vec::new(),
            type_label: None,
            path_like: true,
            explicit,
        };
    }

    let tokens: Vec<&str> = body.split_whitespace().collect();
    if tokens.is_empty() {
        return FileQuery {
            term: String::new(),
            extensions: Vec::new(),
            type_label: None,
            path_like: false,
            explicit,
        };
    }

    if tokens.len() == 1 {
        if let Some((label, exts)) = type_alias(tokens[0]) {
            return FileQuery {
                term: String::new(),
                extensions: owned_exts(exts),
                type_label: Some(label.to_string()),
                path_like: false,
                explicit,
            };
        }
        if let Some(exts) = lone_extension(tokens[0]) {
            let label = type_alias(tokens[0]).map(|(label, _)| label.to_string());
            return FileQuery {
                term: String::new(),
                extensions: exts,
                type_label: label,
                path_like: false,
                explicit,
            };
        }
    }

    if let Some((label, exts)) = type_alias(tokens[0]) {
        return FileQuery {
            term: tokens[1..].join(" "),
            extensions: owned_exts(exts),
            type_label: Some(label.to_string()),
            path_like: false,
            explicit,
        };
    }
    if tokens.len() > 1
        && let Some((label, exts)) = type_alias(tokens[tokens.len() - 1])
    {
        return FileQuery {
            term: tokens[..tokens.len() - 1].join(" "),
            extensions: owned_exts(exts),
            type_label: Some(label.to_string()),
            path_like: false,
            explicit,
        };
    }

    FileQuery {
        term: body.to_string(),
        extensions: Vec::new(),
        type_label: None,
        path_like: false,
        explicit,
    }
}

fn parse_filter(body: &str) -> Option<FileQuery> {
    for prefix in ["type:", "kind:", "ext:"] {
        if let Some(rest) = body
            .to_ascii_lowercase()
            .strip_prefix(prefix)
            .map(|_| &body[prefix.len()..])
        {
            let rest = rest.trim();
            let (filter, term) = rest
                .split_once(char::is_whitespace)
                .map(|(a, b)| (a, b.trim().to_string()))
                .unwrap_or((rest, String::new()));
            if let Some((label, exts)) = type_alias(filter) {
                return Some(FileQuery {
                    term,
                    extensions: owned_exts(exts),
                    type_label: Some(label.to_string()),
                    path_like: false,
                    explicit: true,
                });
            }
            let ext = filter.trim_start_matches('.').to_string();
            if !ext.is_empty() {
                return Some(FileQuery {
                    term,
                    extensions: vec![ext.clone()],
                    type_label: Some(ext),
                    path_like: false,
                    explicit: true,
                });
            }
        }
    }
    None
}

fn lone_extension(token: &str) -> Option<Vec<String>> {
    let cleaned = token.trim();
    if cleaned.starts_with("*.") || (cleaned.starts_with('.') && cleaned.len() > 1) {
        if let Some((_, exts)) = type_alias(cleaned) {
            return Some(owned_exts(exts));
        }
        let ext = cleaned.trim_start_matches("*.").trim_start_matches('.');
        if !ext.is_empty() {
            return Some(vec![ext.to_ascii_lowercase()]);
        }
    }
    None
}

fn owned_exts(exts: &[&str]) -> Vec<String> {
    exts.iter().map(|ext| (*ext).to_string()).collect()
}

fn strip_file_prefix(query: &str) -> Option<&str> {
    for prefix in ["file ", "files ", "find ", "fs ", "f "] {
        if let Some(rest) = query
            .get(..prefix.len())
            .filter(|head| head.eq_ignore_ascii_case(prefix))
        {
            return Some(query[rest.len()..].trim_start());
        }
    }
    None
}

pub fn search(
    query: &str,
    limit: usize,
    include_hidden: bool,
    extra_roots: &[String],
    system_wide: bool,
) -> Vec<Item> {
    let parsed = parse_query(query);
    if !parsed.wants_files() && !parsed.path_like {
        return Vec::new();
    }

    if parsed.path_like {
        return path_results(&parsed.term, limit);
    }

    let mut seen = HashSet::new();
    let mut items = Vec::new();

    for path in discover(&parsed, limit, include_hidden, extra_roots, system_wide) {
        let key = path.to_string_lossy().to_string();
        if !seen.insert(key) {
            continue;
        }
        items.push(file_item(path, parsed.type_label.as_deref()));
        if items.len() >= limit {
            break;
        }
    }
    items
}

pub fn recent(limit: usize, include_hidden: bool, extra_roots: &[String]) -> Vec<Item> {
    let parsed = FileQuery {
        term: String::new(),
        extensions: Vec::new(),
        type_label: None,
        path_like: false,
        explicit: true,
    };
    let mut paths = discover_recent(
        &parsed,
        limit.saturating_mul(2),
        include_hidden,
        extra_roots,
    );
    paths.sort_by_key(|a| std::cmp::Reverse(mtime(a)));
    paths.truncate(limit);
    paths
        .into_iter()
        .map(|path| file_item(path, None))
        .collect()
}

pub fn well_known_folders(query: &str) -> Vec<Item> {
    let q = query.trim().to_ascii_lowercase();
    if q.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (names, path) in known_dirs() {
        let hit = names
            .iter()
            .any(|name| *name == q || name.starts_with(&q) || q.starts_with(*name));
        if !hit || !path.is_dir() {
            continue;
        }
        let key = path.to_string_lossy().to_string();
        if seen.insert(key) {
            out.push(file_item(path, Some("folder")));
        }
    }
    out
}

fn known_dirs() -> Vec<(&'static [&'static str], PathBuf)> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push((&["home", "homedir"][..], home.clone()));
        dirs.push((&["desktop"][..], home.join("Desktop")));
        dirs.push((&["downloads", "download"][..], home.join("Downloads")));
        dirs.push((&["documents", "docs"][..], home.join("Documents")));
        dirs.push((&["pictures", "photos"][..], home.join("Pictures")));
        dirs.push((&["music"][..], home.join("Music")));
        dirs.push((&["videos", "movies"][..], home.join("Videos")));
        dirs.push((&["config", "dotconfig"][..], home.join(".config")));
    }
    if let Some(dir) = dirs::document_dir() {
        dirs.push((&["documents", "docs"][..], dir));
    }
    if let Some(dir) = dirs::download_dir() {
        dirs.push((&["downloads", "download"][..], dir));
    }
    if let Some(dir) = dirs::desktop_dir() {
        dirs.push((&["desktop"][..], dir));
    }
    if let Some(dir) = dirs::picture_dir() {
        dirs.push((&["pictures", "photos"][..], dir));
    }
    if let Some(dir) = dirs::audio_dir() {
        dirs.push((&["music"][..], dir));
    }
    if let Some(dir) = dirs::video_dir() {
        dirs.push((&["videos", "movies"][..], dir));
    }
    dirs
}

fn path_results(query: &str, limit: usize) -> Vec<Item> {
    let expanded = expand_tilde(query);
    let path = PathBuf::from(&expanded);
    if path.is_file() {
        return vec![file_item(path, None)];
    }
    if path.is_dir() {
        return list_dir(&path, limit);
    }
    if let Some(parent) = path.parent().filter(|p| p.is_dir()) {
        let prefix = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut items = list_dir(parent, limit.saturating_mul(2));
        if !prefix.is_empty() {
            items.retain(|item| item.title.to_ascii_lowercase().starts_with(&prefix));
        }
        items.truncate(limit);
        return items;
    }
    Vec::new()
}

fn list_dir(path: &Path, limit: usize) -> Vec<Item> {
    let mut items = Vec::new();
    let Ok(entries) = std::fs::read_dir(path) else {
        return items;
    };
    for entry in entries.flatten() {
        items.push(file_item(entry.path(), None));
        if items.len() >= limit {
            break;
        }
    }
    items.sort_by(|a, b| {
        a.title
            .to_ascii_lowercase()
            .cmp(&b.title.to_ascii_lowercase())
    });
    items
}

fn discover(
    query: &FileQuery,
    limit: usize,
    include_hidden: bool,
    extra_roots: &[String],
    system_wide: bool,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    push_unique(
        &mut paths,
        &mut seen,
        fd_search(query, limit, include_hidden, extra_roots),
    );
    if paths.len() < limit && system_wide {
        let remain = limit.saturating_sub(paths.len());
        push_unique(&mut paths, &mut seen, locate_search(query, remain));
    }
    if paths.is_empty() {
        push_unique(
            &mut paths,
            &mut seen,
            find_search(query, limit, include_hidden, extra_roots),
        );
    }
    paths.truncate(limit);
    paths
}

fn discover_recent(
    query: &FileQuery,
    limit: usize,
    include_hidden: bool,
    extra_roots: &[String],
) -> Vec<PathBuf> {
    let mut paths = fd_recent(limit, include_hidden, extra_roots);
    if paths.is_empty() {
        paths = find_search(query, limit, include_hidden, extra_roots);
    }
    paths
}

fn interleave(buckets: Vec<Vec<PathBuf>>, limit: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let max_len = buckets.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..max_len {
        for bucket in &buckets {
            if let Some(path) = bucket.get(i) {
                let key = path.to_string_lossy().to_string();
                if seen.insert(key) {
                    out.push(path.clone());
                    if out.len() >= limit {
                        return out;
                    }
                }
            }
        }
    }
    out
}

fn push_unique(out: &mut Vec<PathBuf>, seen: &mut HashSet<String>, incoming: Vec<PathBuf>) {
    for path in incoming {
        let key = path.to_string_lossy().to_string();
        if seen.insert(key) {
            out.push(path);
        }
    }
}

fn search_roots(extra_roots: &[String]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    }
    for raw in extra_roots {
        let expanded = expand_tilde(raw.trim());
        if expanded.is_empty() {
            continue;
        }
        let path = PathBuf::from(expanded);
        if path.is_dir() && !roots.contains(&path) {
            roots.push(path);
        }
    }
    if roots.is_empty() {
        roots.push(PathBuf::from("/"));
    }
    roots
}

fn fd_search(
    query: &FileQuery,
    limit: usize,
    include_hidden: bool,
    extra_roots: &[String],
) -> Vec<PathBuf> {
    if !command_exists("fd") {
        return Vec::new();
    }
    let mut buckets = Vec::new();
    for root in search_roots(extra_roots) {
        buckets.push(run_fd(&root, query, limit, include_hidden, false));
    }
    interleave(buckets, limit)
}

fn fd_recent(limit: usize, include_hidden: bool, extra_roots: &[String]) -> Vec<PathBuf> {
    if !command_exists("fd") {
        return Vec::new();
    }
    let query = FileQuery {
        term: String::new(),
        extensions: Vec::new(),
        type_label: None,
        path_like: false,
        explicit: true,
    };
    let mut paths = Vec::new();
    for root in search_roots(extra_roots) {
        paths.extend(run_fd(&root, &query, limit, include_hidden, true));
    }
    paths
}

fn run_fd(
    root: &Path,
    query: &FileQuery,
    limit: usize,
    include_hidden: bool,
    recent_only: bool,
) -> Vec<PathBuf> {
    let mut cmd = Command::new("fd");
    cmd.args([
        "--color=never",
        "--follow",
        "--max-results",
        &limit.to_string(),
    ]);
    if include_hidden {
        cmd.arg("--hidden");
    }
    if recent_only {
        cmd.args(["--type", "f", "--changed-within", "30d"]);
    }
    for exclude in EXCLUDES {
        cmd.args(["--exclude", exclude]);
    }
    for ext in &query.extensions {
        cmd.args(["--extension", ext]);
    }
    if !query.term.is_empty() {
        cmd.args(["--fixed-strings", &query.term]);
    }
    cmd.arg(".");
    cmd.current_dir(root);
    read_paths(cmd, root)
}

fn locate_search(query: &FileQuery, limit: usize) -> Vec<PathBuf> {
    let bin = if command_exists("plocate") {
        "plocate"
    } else if command_exists("locate") {
        "locate"
    } else {
        return Vec::new();
    };

    let pattern = if !query.extensions.is_empty() {
        let alts = query.extensions.join("|");
        if query.term.is_empty() {
            format!(r"\.({alts})$")
        } else {
            let needle = regex_escape(&query.term);
            format!(r"(?i){needle}.*\.({alts})$")
        }
    } else if !query.term.is_empty() {
        regex_escape(&query.term)
    } else {
        return Vec::new();
    };

    let mut cmd = Command::new(bin);
    cmd.args([
        "--ignore-case",
        "--limit",
        &limit.to_string(),
        "--regex",
        &pattern,
    ]);
    read_paths(cmd, Path::new("/"))
        .into_iter()
        .filter(|path| !is_skipped(path))
        .collect()
}

fn find_search(
    query: &FileQuery,
    limit: usize,
    include_hidden: bool,
    extra_roots: &[String],
) -> Vec<PathBuf> {
    if !command_exists("find") {
        return Vec::new();
    }
    let mut buckets = Vec::new();
    for root in search_roots(extra_roots) {
        buckets.push(run_find(&root, query, limit, include_hidden));
    }
    interleave(buckets, limit)
}

fn run_find(root: &Path, query: &FileQuery, limit: usize, include_hidden: bool) -> Vec<PathBuf> {
    let mut cmd = Command::new("find");
    cmd.arg(root);
    if !include_hidden {
        cmd.args(["-name", ".*", "-prune", "-o"]);
    }
    cmd.arg("(");
    for (i, exclude) in EXCLUDES.iter().enumerate() {
        if i > 0 {
            cmd.arg("-o");
        }
        cmd.args(["-name", exclude]);
    }
    cmd.args([")", "-prune", "-o"]);
    if !query.extensions.is_empty() {
        cmd.arg("(");
        for (i, ext) in query.extensions.iter().enumerate() {
            if i > 0 {
                cmd.arg("-o");
            }
            cmd.args(["-iname", &format!("*.{ext}")]);
        }
        cmd.arg(")");
    }
    if !query.term.is_empty() {
        cmd.args(["-iname", &format!("*{}*", query.term)]);
    } else if query.extensions.is_empty() {
        cmd.args(["-type", "f", "-mtime", "-30"]);
    }
    cmd.args(["-print"]);
    read_paths(cmd, root).into_iter().take(limit).collect()
}

fn read_paths(mut cmd: Command, root: &Path) -> Vec<PathBuf> {
    cmd.stdin(Stdio::null());
    cmd.stderr(Stdio::null());
    let Ok(output) = cmd.output() else {
        return Vec::new();
    };
    if !output.status.success() && output.stdout.is_empty() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let path = PathBuf::from(line);
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .filter(|path| !is_skipped(path))
        .collect()
}

fn is_skipped(path: &Path) -> bool {
    let text = path.to_string_lossy();
    SKIP_PREFIXES.iter().any(|prefix| text.starts_with(prefix))
        || path.components().any(|part| {
            let name = part.as_os_str().to_string_lossy();
            EXCLUDES.iter().any(|exclude| {
                *exclude == name
                    || (*exclude == ".local/share/Trash" && text.contains("/.local/share/Trash/"))
            })
        })
}

pub fn file_item(path: PathBuf, type_label: Option<&str>) -> Item {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let subtitle = path.to_string_lossy().to_string();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let alias = type_label.unwrap_or("");
    let keywords = format!("{alias} {ext} {parent} file folder");
    Item {
        id: format!("file:{}", path.display()),
        title: name,
        subtitle,
        keywords,
        kind: Kind::File,
        icon: file_icon(&path),
        action: Action::OpenPath(path),
    }
}

fn file_icon(path: &Path) -> Icon {
    if path.is_dir() {
        return Icon::Name("folder".into());
    }
    let name = match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
    {
        "md" | "markdown" | "mdx" | "txt" | "rst" => "accessories-text-editor",
        "pdf" => "application-pdf",
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "avif" => "image-x-generic",
        "mp4" | "mkv" | "webm" | "mov" => "video-x-generic",
        "mp3" | "flac" | "wav" | "opus" | "ogg" => "audio-x-generic",
        "zip" | "tar" | "gz" | "7z" | "rar" => "package-x-generic",
        "rs" | "py" | "js" | "ts" | "go" | "c" | "h" | "cpp" => "text-x-script",
        _ => "text-x-generic",
    };
    Icon::Name(name.into())
}

pub fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().into_owned();
    }
    if path == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home.to_string_lossy().into_owned();
    }
    path.to_string()
}

fn mtime(path: &Path) -> SystemTime {
    path.metadata()
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

pub fn recency_bonus(path: &Path) -> u32 {
    let Ok(modified) = path.metadata().and_then(|meta| meta.modified()) else {
        return 0;
    };
    let Ok(age) = SystemTime::now().duration_since(modified) else {
        return 80;
    };
    if age < Duration::from_secs(60 * 60 * 24) {
        2_400
    } else if age < Duration::from_secs(60 * 60 * 24 * 7) {
        1_200
    } else if age < Duration::from_secs(60 * 60 * 24 * 30) {
        400
    } else {
        0
    }
}

fn regex_escape(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ".+*?^$()[]{}|\\".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn command_exists(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{FileQuery, parse_query, type_alias};
    use std::fs;

    #[test]
    fn markdown_is_a_type_query() {
        let q = parse_query("markdown");
        assert!(q.is_type_search());
        assert_eq!(q.type_label.as_deref(), Some("markdown"));
        assert!(q.extensions.iter().any(|ext| ext == "md"));
        assert!(q.extensions.iter().any(|ext| ext == "markdown"));
    }

    #[test]
    fn extension_and_filter_forms() {
        assert_eq!(parse_query(".md").type_label.as_deref(), Some("markdown"));
        assert_eq!(parse_query("*.pdf").extensions, vec!["pdf".to_string()]);
        let filtered = parse_query("type:md readme");
        assert_eq!(filtered.type_label.as_deref(), Some("markdown"));
        assert_eq!(filtered.term, "readme");
        assert_eq!(parse_query("kind:pdf").extensions, vec!["pdf".to_string()]);
    }

    #[test]
    fn type_plus_name_keeps_both() {
        let q = parse_query("markdown readme");
        assert_eq!(q.type_label.as_deref(), Some("markdown"));
        assert_eq!(q.term, "readme");
        let trailing = parse_query("notes markdown");
        assert_eq!(trailing.type_label.as_deref(), Some("markdown"));
        assert_eq!(trailing.term, "notes");
    }

    #[test]
    fn ordinary_names_are_not_types() {
        let q = parse_query("firefox");
        assert!(!q.is_type_search());
        assert!(q.extensions.is_empty());
        assert_eq!(q.term, "firefox");
    }

    #[test]
    fn file_prefix_is_explicit() {
        let q = parse_query("file invoices");
        assert!(q.explicit);
        assert_eq!(q.term, "invoices");
    }

    #[test]
    fn aliases_cover_common_raycast_kinds() {
        assert!(type_alias("pdf").is_some());
        assert!(type_alias("images").is_some());
        assert!(type_alias("javascript").is_some());
        assert!(type_alias("rs").is_some());
        assert!(type_alias("firefox").is_none());
    }

    #[test]
    fn find_fallback_walks_a_temp_tree() {
        let root = std::env::temp_dir().join(format!("flint-files-{}", std::process::id()));
        let nested = root.join("docs").join("nested");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("README.md"), "# hi").unwrap();
        fs::write(nested.join("notes.markdown"), "x").unwrap();
        fs::write(root.join("skip.txt"), "no").unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::write(root.join("node_modules").join("dep.md"), "no").unwrap();

        let query = FileQuery {
            term: String::new(),
            extensions: vec!["md".into(), "markdown".into()],
            type_label: Some("markdown".into()),
            path_like: false,
            explicit: true,
        };
        let found = super::run_find(&root, &query, 50, false);
        let names: Vec<String> = found
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(str::to_string))
            .collect();
        assert!(names.iter().any(|n| n == "README.md"));
        assert!(names.iter().any(|n| n == "notes.markdown"));
        assert!(!names.iter().any(|n| n == "dep.md"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_finds_type_hits_in_an_extra_root() {
        let root = std::env::temp_dir().join(format!("flint-md-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("deep/nested")).unwrap();
        fs::write(root.join("deep/nested/alpha.md"), "one").unwrap();
        fs::write(root.join("beta.markdown"), "two").unwrap();
        fs::write(root.join("ignore.txt"), "no").unwrap();
        let items = super::search(
            "markdown",
            50,
            false,
            &[root.to_string_lossy().into_owned()],
            false,
        );
        let titles: Vec<String> = items.into_iter().map(|item| item.title).collect();
        assert!(
            titles.iter().any(|title| title == "alpha.md")
                || titles.iter().any(|title| title == "beta.markdown"),
            "expected markdown files from extra root, got {titles:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
