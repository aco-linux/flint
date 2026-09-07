use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::action;
use crate::db;
use crate::item::{Action, Icon, Item, Kind};
use crate::paths;
use crate::placeholder;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub name: String,
    #[serde(default)]
    pub title: String,
    pub target: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Link {
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            &self.name
        } else {
            &self.title
        }
    }

    pub fn argument_for(&self, query: &str) -> String {
        let q = query.trim();
        let name = self.name.trim();
        if q.eq_ignore_ascii_case(name) {
            return String::new();
        }
        let lower = q.to_ascii_lowercase();
        let prefix = name.to_ascii_lowercase();
        if lower.starts_with(&prefix)
            && q.get(name.len()..)
                .is_some_and(|rest| rest.starts_with(char::is_whitespace) || rest.is_empty())
        {
            return q[name.len()..].trim().to_string();
        }
        String::new()
    }

    pub fn resolved(&self, argument: &str) -> String {
        expand_tilde(&substitute(&self.target, argument))
    }

    pub fn to_item(&self, argument: &str) -> Item {
        let target = self.resolved(argument);
        let (kind, icon, action) = item_action(&target);
        let arg_hint = if argument.is_empty() {
            String::new()
        } else {
            format!(" · {argument}")
        };
        Item {
            id: format!("link:{}", self.name),
            title: self.display_title().to_string(),
            subtitle: format!("{}{arg_hint}", self.target),
            keywords: format!(
                "{} {} {} {}",
                self.name,
                self.title,
                self.tags.join(" "),
                "quicklink link"
            ),
            kind,
            icon: Icon::Name(icon.into()),
            action,
        }
    }
}

pub fn load() -> Vec<Link> {
    db::quicklinks_load().unwrap_or_default()
}

pub fn save(links: &[Link]) {
    for link in links {
        let _ = db::quicklink_upsert(link);
    }
}

pub fn upsert(link: Link) {
    save(&[link]);
}

pub fn parse_create(query: &str) -> Option<(String, String)> {
    let rest = query.trim().strip_prefix('+')?.trim();
    let (name, target) = rest.split_once(char::is_whitespace)?;
    let name = name.trim();
    let target = target.trim();
    if name.is_empty() || target.is_empty() {
        return None;
    }
    Some((name.to_string(), target.to_string()))
}

pub fn create(name: &str, target: &str) -> Result<Link, &'static str> {
    let name = name.trim();
    let target = target.trim();
    if name.is_empty() || target.is_empty() {
        return Err("empty");
    }
    if !is_safe_target(target) {
        return Err("unsafe");
    }
    Ok(Link {
        name: name.to_string(),
        title: title_from_name(name),
        target: target.to_string(),
        tags: Vec::new(),
    })
}

pub fn substitute(target: &str, argument: &str) -> String {
    placeholder::expand_quicklink(target, argument)
}

pub fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return dirs::home_dir()
            .map(|home| home.display().to_string())
            .unwrap_or_else(|| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).display().to_string();
    }
    path.to_string()
}

pub fn is_safe_target(target: &str) -> bool {
    let target = target.trim();
    if target.is_empty() || target.contains('\0') {
        return false;
    }
    let expanded = expand_tilde(target);
    if looks_like_uri(&expanded) {
        return action::is_safe_uri(&expanded);
    }
    true
}

fn looks_like_uri(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.contains("://") || lower.starts_with("javascript:")
}

fn item_action(target: &str) -> (Kind, &'static str, Action) {
    if looks_like_uri(target) {
        let action = if action::is_safe_uri(target) {
            Action::OpenUri(target.to_string())
        } else {
            Action::Copy(target.to_string())
        };
        (Kind::Web, "web-browser", action)
    } else {
        (
            Kind::File,
            "folder",
            Action::OpenPath(PathBuf::from(target)),
        )
    }
}

fn title_from_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => name.to_string(),
    }
}

pub(crate) fn default_links() -> Vec<Link> {
    vec![
        Link {
            name: "dl".into(),
            title: "Downloads".into(),
            target: "~/Downloads".into(),
            tags: vec!["folder".into()],
        },
        Link {
            name: "docs".into(),
            title: "Documents".into(),
            target: "~/Documents".into(),
            tags: vec!["folder".into()],
        },
        Link {
            name: "gh".into(),
            title: "GitHub".into(),
            target: "https://github.com/search?q={argument}".into(),
            tags: vec!["web".into()],
        },
    ]
}

#[allow(dead_code)]
pub fn file() -> std::path::PathBuf {
    paths::config_dir().join("quicklinks.json")
}

#[cfg(test)]
mod tests {
    use super::{Link, create, expand_tilde, parse_create, substitute};

    fn link(name: &str, target: &str) -> Link {
        Link {
            name: name.into(),
            title: name.into(),
            target: target.into(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn argument_substitution() {
        let gh = link("gh", "https://github.com/search?q={argument}");
        let resolved = substitute(&gh.target, "rust gtk");
        assert_eq!(resolved, "https://github.com/search?q=rust+gtk");
        assert!(crate::action::is_safe_uri(&resolved));
        assert_eq!(gh.argument_for("gh rust"), "rust");
        assert_eq!(gh.argument_for("gh"), "");
        let q = link("q", "https://example.com?q={Query}");
        assert_eq!(substitute(&q.target, "hi"), "https://example.com?q=hi");
    }

    #[test]
    fn tilde_expands_to_home() {
        let expanded = expand_tilde("~/Downloads");
        assert!(
            expanded.ends_with("Downloads"),
            "expanded {expanded} should end with Downloads"
        );
        assert!(!expanded.starts_with('~'));
        assert_eq!(expand_tilde("/tmp/x"), "/tmp/x");
    }

    #[test]
    fn reject_unsafe_uri() {
        assert!(create("ok", "https://example.com").is_ok());
        assert!(create("dl", "~/Downloads").is_ok());
        assert!(create("xss", "javascript:alert(1)").is_err());
        assert!(create("smb", "smb://evil").is_err());
        assert!(create("file", "file:///etc/passwd").is_ok());
    }

    #[test]
    fn parse_plus_create() {
        assert_eq!(
            parse_create("+gh https://github.com/search?q={argument}"),
            Some(("gh".into(), "https://github.com/search?q={argument}".into()))
        );
        assert_eq!(
            parse_create("+dl ~/Downloads"),
            Some(("dl".into(), "~/Downloads".into()))
        );
        assert!(parse_create("gh https://x").is_none());
        assert!(parse_create("+alone").is_none());
    }
}
