#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Root,
    Windows,
    Clipboard,
    Snippets,
    Notes,
    Ask,
    Voice,
    Settings,
    Store,
}

impl Mode {
    pub fn parse(query: &str) -> (Self, String) {
        let q = query.trim_start();
        if let Some(rest) = strip_kw(q, &["win ", "windows"]) {
            return (Mode::Windows, rest);
        }
        if let Some(rest) = strip_kw(q, &["clip ", "cb ", "clipboard"]) {
            return (Mode::Clipboard, rest);
        }
        if let Some(rest) = q.strip_prefix(';') {
            return (Mode::Snippets, rest.trim_start().to_string());
        }
        if let Some(rest) = strip_kw(q, &["snip ", "snippet ", "snippets"]) {
            return (Mode::Snippets, rest);
        }
        if let Some(rest) = q.strip_prefix('?') {
            return (Mode::Ask, rest.trim_start().to_string());
        }
        if let Some(rest) = strip_kw(q, &["ask ", "ai ", "ask ai", "askai"]) {
            return (Mode::Ask, rest);
        }
        if let Some(rest) = strip_kw(q, &["note ", "notes ", "n "]) {
            return (Mode::Notes, rest);
        }
        if let Some(rest) = strip_kw(q, &["voice ", "dictation ", "dictate"]) {
            return (Mode::Voice, rest);
        }
        if let Some(rest) = strip_kw(q, &["set ", "settings ", "prefs ", "preferences"]) {
            return (Mode::Settings, rest);
        }
        if let Some(rest) = strip_kw(q, &["store ", "extensions ", "ext "]) {
            return (Mode::Store, rest);
        }
        (Mode::Root, q.to_string())
    }

    pub fn badge(self) -> Option<&'static str> {
        match self {
            Mode::Root => None,
            Mode::Windows => Some("WINDOWS"),
            Mode::Clipboard => Some("CLIP"),
            Mode::Snippets => Some("SNIP"),
            Mode::Notes => Some("NOTES"),
            Mode::Ask => Some("ASK"),
            Mode::Voice => Some("VOICE"),
            Mode::Settings => Some("SETTINGS"),
            Mode::Store => Some("STORE"),
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            Mode::Root => "Search apps, notes, ask AI, windows…",
            Mode::Windows => "Filter open windows…",
            Mode::Clipboard => "Search clipboard history…",
            Mode::Snippets => "Snippets — type +name to save clipboard",
            Mode::Notes => "Search notes — type +title to create",
            Mode::Ask => "Ask anything… Enter to send",
            Mode::Voice => "Enter starts dictation. Speak, then Enter again.",
            Mode::Settings => "Search settings…",
            Mode::Store => "Browse extensions, MCP servers, Script Commands",
        }
    }

    pub fn empty_title(self) -> &'static str {
        match self {
            Mode::Root => "Nothing matches.",
            Mode::Windows => "No open windows.",
            Mode::Clipboard => "Clipboard is empty.",
            Mode::Snippets => "No snippets yet.",
            Mode::Notes => "No notes yet.",
            Mode::Ask => "Ask Flint.",
            Mode::Voice => "Ready when you are.",
            Mode::Settings => "No matching setting.",
            Mode::Store => "Store is empty.",
        }
    }

    pub fn empty_sub(self) -> &'static str {
        match self {
            Mode::Root => "Try an app, ?ask, note, win, clip, store, or settings.",
            Mode::Windows => "Open something, then jump back here.",
            Mode::Clipboard => "Copy text anywhere and it lands here.",
            Mode::Snippets => "Type +email to save the current clipboard as “email”.",
            Mode::Notes => "Type +ship checklist to create a note.",
            Mode::Ask => "Local models first. Sign in with OAuth for a cloud subscription.",
            Mode::Voice => "Enter starts. Speak. Enter again fills the search box.",
            Mode::Settings => "OAuth, local models, MCP servers, autostart.",
            Mode::Store => "Vicinae extensions, MCP servers, or Raycast Script Commands.",
        }
    }

    pub fn prefix(self) -> &'static str {
        match self {
            Mode::Root => "",
            Mode::Windows => "win ",
            Mode::Clipboard => "clip ",
            Mode::Snippets => "; ",
            Mode::Notes => "note ",
            Mode::Ask => "? ",
            Mode::Voice => "voice ",
            Mode::Settings => "set ",
            Mode::Store => "store ",
        }
    }
}

fn strip_kw(query: &str, prefixes: &[&str]) -> Option<String> {
    let lower = query.to_ascii_lowercase();
    for prefix in prefixes {
        if lower.starts_with(prefix) {
            return Some(query[prefix.len()..].trim_start().to_string());
        }
        let bare = prefix.trim();
        if lower == bare {
            return Some(String::new());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::Mode;

    #[test]
    fn parses_prefixes() {
        assert_eq!(Mode::parse("win steam"), (Mode::Windows, "steam".into()));
        assert_eq!(Mode::parse("CLIP foo"), (Mode::Clipboard, "foo".into()));
        assert_eq!(Mode::parse("; email"), (Mode::Snippets, "email".into()));
        assert_eq!(Mode::parse("firefox"), (Mode::Root, "firefox".into()));
        assert_eq!(Mode::parse("windows"), (Mode::Windows, "".into()));
        assert_eq!(Mode::parse("window manager").0, Mode::Root);
        assert_eq!(Mode::parse("? weather"), (Mode::Ask, "weather".into()));
        assert_eq!(Mode::parse("ask summarize this").0, Mode::Ask);
        assert_eq!(Mode::parse("note inbox").0, Mode::Notes);
        assert_eq!(Mode::parse("settings").0, Mode::Settings);
        assert_eq!(Mode::parse("store mcp").0, Mode::Store);
    }
}
