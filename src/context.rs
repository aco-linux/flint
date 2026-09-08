//! Show-time search context: focused Hyprland window, fresh clipboard, editor paths.
//! Never AT-SPI. Captured once when Flint opens, not on every keystroke.

use std::path::PathBuf;

use crate::calc;
use crate::hypr;
use crate::item::{Action, Icon, Item, Kind};
use crate::usage;

pub const FRESH_SECS: u64 = 10;
const CONTEXT_APP_BONUS: u32 = 15_000;

#[derive(Debug, Clone, Default)]
pub struct Context {
    pub class: String,
    pub title: String,
    pub primary: Option<String>,
    pub captured_at: u64,
}

impl Context {
    /// Snapshot the window behind Flint (skip the launcher) plus primary selection.
    pub fn capture() -> Self {
        let (class, title) = hypr::focused_window().unwrap_or_default();
        Self {
            class,
            title,
            primary: crate::clipboard::primary_text()
                .filter(|t| !crate::clipboard::looks_secret(t)),
            captured_at: usage::now_secs(),
        }
    }

    pub fn is_fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.captured_at) < FRESH_SECS
    }
}

pub fn app_bonus(item_app: &str, class: &str) -> u32 {
    if item_app.is_empty() || class.is_empty() {
        return 0;
    }
    if item_app.eq_ignore_ascii_case(class) {
        CONTEXT_APP_BONUS
    } else {
        0
    }
}

/// Paths mentioned in a window title (`foo.rs — Code`, `/home/me/a.rs`).
pub fn editor_paths(title: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for token in title.split([' ', '\t', '|', '—', '–', ':']) {
        let token = token.trim().trim_matches(['"', '\'', ',', ';']);
        if token.is_empty() || token.len() < 3 {
            continue;
        }
        if !token.contains('/') && !token.contains('.') {
            continue;
        }
        if looks_like_url(token) {
            continue;
        }
        let path = expand_path(token);
        if path.is_file() {
            out.push(path);
        }
    }
    out
}

fn expand_path(token: &str) -> PathBuf {
    if let Some(rest) = token.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(token)
}

fn looks_like_url(token: &str) -> bool {
    token.contains("://") || token.starts_with("www.")
}

pub fn fresh_clipboard_items(text: &str) -> Vec<Item> {
    let preview: String = text.chars().take(48).collect();
    let mut items = vec![
        Item {
            id: "ctx:paste".into(),
            title: "Paste".into(),
            subtitle: preview.clone(),
            keywords: "paste clipboard primary".into(),
            kind: Kind::Clipboard,
            icon: Icon::Name("edit-paste".into()),
            action: Action::Paste(text.to_string()),
        },
        Item {
            id: format!("ctx:web:{text}"),
            title: format!("Search the web for “{preview}”"),
            subtitle: "DuckDuckGo Instant Answer".into(),
            keywords: "search web".into(),
            kind: Kind::Web,
            icon: Icon::Name("web-browser".into()),
            action: crate::web::search_item(text).action,
        },
        Item {
            id: format!("ctx:ask:{text}"),
            title: format!("Ask AI “{preview}”"),
            subtitle: "Uses the configured model".into(),
            keywords: "ask ai".into(),
            kind: Kind::Ai,
            icon: Icon::Name("help-faq".into()),
            action: Action::AskAi {
                prompt: text.to_string(),
            },
        },
    ];
    if looks_like_uri(text) {
        let uri = if text.contains("://") {
            text.to_string()
        } else {
            format!("https://{text}")
        };
        items.push(Item {
            id: format!("ctx:url:{uri}"),
            title: format!("Open {uri}"),
            subtitle: "Open in default browser".into(),
            keywords: "url open".into(),
            kind: Kind::Web,
            icon: Icon::Name("web-browser".into()),
            action: Action::OpenUri(uri),
        });
    }
    if let Some(calc) = calc::root_item(text) {
        items.push(calc);
    }
    items
}

fn looks_like_uri(query: &str) -> bool {
    let q = query.trim();
    if q.contains(' ') {
        return false;
    }
    q.starts_with("http://")
        || q.starts_with("https://")
        || q.starts_with("file://")
        || (q.contains('.')
            && q.chars().any(|c| c.is_ascii_alphabetic())
            && q.chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-_:/?#=&".contains(c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_bonus_matches_class_case_insensitively() {
        assert_eq!(app_bonus("firefox", "Firefox"), CONTEXT_APP_BONUS);
        assert_eq!(app_bonus("firefox", "code"), 0);
        assert_eq!(app_bonus("", "firefox"), 0);
    }

    #[test]
    fn editor_paths_skips_urls_and_short_tokens() {
        assert!(editor_paths("https://example.com/foo.rs").is_empty());
        assert!(editor_paths("hi").is_empty());
    }

    #[test]
    fn editor_paths_finds_existing_file() {
        let dir = std::env::temp_dir().join(format!(
            "flint-ctx-{}-{}",
            std::process::id(),
            usage::now_secs()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("catalog.rs");
        std::fs::write(&file, b"fn").unwrap();
        let title = format!("{} — Visual Studio Code", file.display());
        let found = editor_paths(&title);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found, vec![file]);
    }

    #[test]
    fn fresh_clipboard_offers_paste_web_ask() {
        let items = fresh_clipboard_items("hello world");
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains(&"ctx:paste"));
        assert!(ids.iter().any(|id| id.starts_with("ctx:web:")));
        assert!(ids.iter().any(|id| id.starts_with("ctx:ask:")));
        assert!(!ids.iter().any(|id| id.starts_with("ctx:url:")));
    }

    #[test]
    fn fresh_clipboard_url_and_calc() {
        let url = fresh_clipboard_items("https://example.com");
        assert!(url.iter().any(|i| i.id.starts_with("ctx:url:")));
        let math = fresh_clipboard_items("1+1");
        assert!(math.iter().any(|i| i.kind == Kind::Calc));
    }

    #[test]
    fn context_freshness_window() {
        let ctx = Context {
            captured_at: 1_000,
            ..Context::default()
        };
        assert!(ctx.is_fresh(1_009));
        assert!(!ctx.is_fresh(1_010));
    }
}
