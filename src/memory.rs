use crate::db;
use crate::item::{Action, Icon, Item, Kind};

const PROMPT_CAP: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub id: String,
    pub text: String,
    pub at: u64,
}

impl Fact {
    pub fn to_item(&self) -> Item {
        let preview: String = self.text.chars().take(80).collect();
        Item {
            id: format!("memory:{}", self.id),
            title: preview,
            subtitle: "Memory · Enter copies · Forget … to delete".into(),
            keywords: format!("memory remember {}", self.text),
            kind: Kind::Ai,
            icon: Icon::Name("help-about".into()),
            action: Action::Copy(self.text.clone()),
        }
    }
}

pub fn command_items() -> Vec<Item> {
    vec![Item {
        id: "cmd:show-memory".into(),
        title: "Show memory".into(),
        subtitle: "Facts Flint injects into Ask AI".into(),
        keywords: "remember memory forget ai facts".into(),
        kind: Kind::Ai,
        icon: Icon::Name("help-about".into()),
        action: Action::ShowMemory,
    }]
}

/// Root-search hits for `remember …` / `show memory` / `forget …`.
pub fn items(query: &str) -> Vec<Item> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let lower = q.to_ascii_lowercase();
    let mut out = Vec::new();
    if let Some(text) = strip_prefix_ci(q, &lower, &["remember that ", "remember "]) {
        let text = text.trim();
        if text.is_empty() {
            out.push(Item {
                id: "cmd:remember-hint".into(),
                title: "Remember …".into(),
                subtitle: "Type a fact after remember".into(),
                keywords: "remember memory".into(),
                kind: Kind::Ai,
                icon: Icon::Name("help-about".into()),
                action: Action::ShowMemory,
            });
        } else {
            out.push(Item {
                id: format!("cmd:remember:{text}"),
                title: format!("Remember “{text}”"),
                subtitle: "Save a fact for Ask AI".into(),
                keywords: format!("remember memory {text}"),
                kind: Kind::Ai,
                icon: Icon::Name("help-about".into()),
                action: Action::Remember {
                    text: text.to_string(),
                },
            });
        }
    }
    if is_show_memory(&lower) {
        out.push(command_items().into_iter().next().expect("show memory"));
        out.extend(list().into_iter().map(|f| f.to_item()));
    }
    if let Some(term) = strip_prefix_ci(q, &lower, &["forget that ", "forget "]) {
        let term = term.trim();
        if !term.is_empty() {
            out.push(Item {
                id: format!("cmd:forget:{term}"),
                title: format!("Forget “{term}”"),
                subtitle: "Delete matching memories".into(),
                keywords: format!("forget memory {term}"),
                kind: Kind::Ai,
                icon: Icon::Name("edit-delete".into()),
                action: Action::ForgetMemory {
                    query: term.to_string(),
                },
            });
        }
    }
    out
}

pub fn remember(text: &str) -> Option<Fact> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    db::memory_remember(text)
}

pub fn forget(query: &str) -> usize {
    let query = query.trim();
    if query.is_empty() {
        return 0;
    }
    db::memory_forget(query).unwrap_or(0)
}

pub fn list() -> Vec<Fact> {
    db::memory_load().unwrap_or_default()
}

pub fn list_items() -> Vec<Item> {
    list().into_iter().map(|f| f.to_item()).collect()
}

/// Newest facts first, capped at ~2 KiB for the system prompt.
pub fn prompt_block() -> Option<String> {
    let facts = list();
    if facts.is_empty() {
        return None;
    }
    let mut body = String::from("## Memory\n");
    for fact in &facts {
        let line = format!("- {}\n", fact.text.trim());
        if body.len() + line.len() > PROMPT_CAP {
            break;
        }
        body.push_str(&line);
    }
    if body.len() <= "## Memory\n".len() {
        None
    } else {
        Some(body)
    }
}

fn is_show_memory(lower: &str) -> bool {
    matches!(
        lower,
        "memory" | "show memory" | "memories" | "show memories" | "remembered"
    )
}

fn strip_prefix_ci<'a>(original: &'a str, lower: &str, prefixes: &[&str]) -> Option<&'a str> {
    for prefix in prefixes {
        if lower.starts_with(prefix) {
            return Some(&original[prefix.len()..]);
        }
        if lower == prefix.trim() {
            return Some("");
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{forget, items, prompt_block, remember};
    use crate::db;
    use crate::item::Action;

    #[test]
    fn remember_forget_and_prompt_cap() {
        db::with_temp(|dir| {
            db::open_path(&dir.join("flint.db")).expect("open");
            assert!(prompt_block().is_none());
            let fact = remember("I use Hyprland").expect("saved");
            assert_eq!(fact.text, "I use Hyprland");
            remember("I prefer dark mode").expect("saved");
            let block = prompt_block().expect("block");
            assert!(block.contains("## Memory"));
            assert!(block.contains("I use Hyprland"));
            assert!(block.contains("I prefer dark mode"));
            assert!(block.len() <= 2048);
            assert_eq!(forget("Hyprland"), 1);
            let left = super::list();
            assert_eq!(left.len(), 1);
            assert_eq!(left[0].text, "I prefer dark mode");
            assert_eq!(forget("nope"), 0);
        });
    }

    #[test]
    fn parse_remember_and_forget_commands() {
        let rem = items("remember that I ship from Omarchy");
        assert!(rem.iter().any(|item| matches!(
            &item.action,
            Action::Remember { text } if text == "I ship from Omarchy"
        )));
        let show = items("show memory");
        assert!(
            show.iter()
                .any(|item| matches!(item.action, Action::ShowMemory))
        );
        let forget_item = items("forget Hyprland");
        assert!(forget_item.iter().any(|item| matches!(
            &item.action,
            Action::ForgetMemory { query } if query == "Hyprland"
        )));
        assert!(items("firefox").is_empty());
        assert!(items("remembered-not-a-command").is_empty());
    }

    #[test]
    fn duplicate_remember_does_not_fork_rows() {
        db::with_temp(|dir| {
            db::open_path(&dir.join("flint.db")).expect("open");
            remember("same fact").expect("first");
            remember("same fact").expect("second");
            assert_eq!(super::list().len(), 1);
        });
    }
}
