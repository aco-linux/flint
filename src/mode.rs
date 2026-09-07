#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Root,
    Files,
    Windows,
    Clipboard,
    Snippets,
    Notes,
    Ask,
    Voice,
    Settings,
    Store,
    Quicklink,
    Calc,
    Emoji,
    Gif,
    Content,
    /// A running extension owns the list. Never parsed from text.
    Extension,
}

impl Mode {
    pub fn parse(query: &str) -> (Self, String) {
        let q = query.trim_start();
        if let Some(rest) = strip_kw(q, &["file ", "files ", "find ", "fs "]) {
            return (Mode::Files, rest);
        }
        if let Some(rest) = q
            .get(..2)
            .filter(|head| head.eq_ignore_ascii_case("f "))
            .map(|_| q[2..].trim_start().to_string())
        {
            return (Mode::Files, rest);
        }
        if q.eq_ignore_ascii_case("file")
            || q.eq_ignore_ascii_case("files")
            || q.eq_ignore_ascii_case("find")
            || q.eq_ignore_ascii_case("fs")
        {
            return (Mode::Files, String::new());
        }
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
        if let Some(rest) = strip_kw(q, &["link ", "links "]) {
            return (Mode::Quicklink, rest);
        }
        if q == "=" {
            return (Mode::Calc, String::new());
        }
        if let Some(rest) = q.strip_prefix('=') {
            return (Mode::Calc, rest.trim_start().to_string());
        }
        if let Some(rest) = strip_kw(q, &["calc "]) {
            return (Mode::Calc, rest);
        }
        if q.eq_ignore_ascii_case("emoji") || q.eq_ignore_ascii_case("emojis") {
            return (Mode::Emoji, String::new());
        }
        if let Some(rest) = strip_kw(q, &["emoji ", "emojis "]) {
            return (Mode::Emoji, rest);
        }
        if q.eq_ignore_ascii_case("gif") || q.eq_ignore_ascii_case("gifs") {
            return (Mode::Gif, String::new());
        }
        if let Some(rest) = strip_kw(q, &["gif ", "gifs "]) {
            return (Mode::Gif, rest);
        }
        if let Some(rest) = strip_content(q) {
            return (Mode::Content, rest);
        }
        (Mode::Root, q.to_string())
    }

    pub fn badge(self) -> Option<&'static str> {
        match self {
            Mode::Root => None,
            Mode::Files => Some("FILES"),
            Mode::Windows => Some("WINDOWS"),
            Mode::Clipboard => Some("CLIP"),
            Mode::Snippets => Some("SNIP"),
            Mode::Notes => Some("NOTES"),
            Mode::Ask => Some("ASK"),
            Mode::Voice => Some("VOICE"),
            Mode::Settings => Some("SETTINGS"),
            Mode::Store => Some("STORE"),
            Mode::Quicklink => Some("LINKS"),
            Mode::Calc => Some("CALC"),
            Mode::Emoji => Some("EMOJI"),
            Mode::Gif => Some("GIF"),
            Mode::Content => Some("CONTENT"),
            Mode::Extension => Some("EXT"),
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            Mode::Root => "Search apps, files, notes, ask AI…",
            Mode::Files => "Search files — markdown, pdf, or any name",
            Mode::Windows => "Filter open windows…",
            Mode::Clipboard => "Search clipboard history…",
            Mode::Snippets => "Snippets — type +name to save clipboard",
            Mode::Notes => "Search notes — type +title to create",
            Mode::Ask => "Ask anything… empty lists chats · Enter to send",
            Mode::Voice => "Enter starts. Empty query is history. Speak, then Enter.",
            Mode::Settings => "Search settings…",
            Mode::Store => "Browse extensions, MCP servers, Script Commands",
            Mode::Quicklink => "Quicklinks — type +name url to create",
            Mode::Calc => "Calculation history — type math, dates, or percents",
            Mode::Emoji => "Search emoji — smile, :smile:, or a keyword",
            Mode::Gif => "Search GIFs — cats, wow, shipit",
            Mode::Content => "Search file contents — ripgrep, cancelled on the next key",
            Mode::Extension => "Search…",
        }
    }

    pub fn empty_title(self) -> &'static str {
        match self {
            Mode::Root => "Nothing matches.",
            Mode::Files => "No files matched.",
            Mode::Windows => "No open windows.",
            Mode::Clipboard => "Clipboard is empty.",
            Mode::Snippets => "No snippets yet.",
            Mode::Notes => "No notes yet.",
            Mode::Ask => "Ask Flint.",
            Mode::Voice => "Ready when you are.",
            Mode::Settings => "No matching setting.",
            Mode::Store => "Store is empty.",
            Mode::Quicklink => "No quicklinks yet.",
            Mode::Calc => "No calculations yet.",
            Mode::Emoji => "No matching emoji.",
            Mode::Gif => "Type a search — cats, wow, shipit.",
            Mode::Content => "No matching file contents.",
            Mode::Extension => "Nothing to show.",
        }
    }

    pub fn empty_sub(self) -> &'static str {
        match self {
            Mode::Root => "Try an app, a file type like markdown, ?ask, note, win, or file.",
            Mode::Files => "Type markdown, *.pdf, or a name. Arrow keys scroll every match.",
            Mode::Windows => "Open something, then jump back here.",
            Mode::Clipboard => "Copy text anywhere and it lands here.",
            Mode::Snippets => "Type +email to save the current clipboard as “email”.",
            Mode::Notes => "Type +ship checklist to create a note.",
            Mode::Ask => "Empty lists chats. Esc or New chat starts another. Local models first.",
            Mode::Voice => "Enter starts in-bar. “Dictate to focused app” types with wtype.",
            Mode::Settings => "OAuth, local models, MCP servers, autostart.",
            Mode::Store => "Vicinae extensions, MCP servers, or Raycast Script Commands.",
            Mode::Quicklink => "Type +gh https://github.com/search?q={argument} to save a link.",
            Mode::Calc => "Try 20% of 80, today + 7d, or 2+2. History stays on this machine.",
            Mode::Emoji => "Type smile or :fire:. Enter pastes the glyph.",
            Mode::Gif => {
                "Add a Tenor API key in Settings for in-launcher GIFs, or Enter opens Tenor."
            }
            Mode::Content => "Type a phrase. Flint runs rg over $HOME, never /.",
            Mode::Extension => "Esc goes back.",
        }
    }

    pub fn prefix(self) -> &'static str {
        match self {
            Mode::Root => "",
            Mode::Files => "file ",
            Mode::Windows => "win ",
            Mode::Clipboard => "clip ",
            Mode::Snippets => "; ",
            Mode::Notes => "note ",
            Mode::Ask => "? ",
            Mode::Voice => "voice ",
            Mode::Settings => "set ",
            Mode::Store => "store ",
            Mode::Quicklink => "link ",
            Mode::Calc => "calc ",
            Mode::Emoji => "emoji ",
            Mode::Gif => "gif ",
            Mode::Content => "content ",
            Mode::Extension => "",
        }
    }
}

fn strip_content(query: &str) -> Option<String> {
    let lower = query.to_ascii_lowercase();
    if lower == "content" {
        return Some(String::new());
    }
    if lower.starts_with("content:") {
        return Some(query["content:".len()..].trim_start().to_string());
    }
    if lower.starts_with("content ") {
        return Some(query["content ".len()..].trim_start().to_string());
    }
    None
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
        assert_eq!(
            Mode::parse("file markdown"),
            (Mode::Files, "markdown".into())
        );
        assert_eq!(Mode::parse("files"), (Mode::Files, "".into()));
        assert_eq!(Mode::parse("f invoices").0, Mode::Files);
        assert_eq!(Mode::parse("firefox").0, Mode::Root);
        assert_eq!(Mode::parse("find notes.md").0, Mode::Files);
        assert_eq!(Mode::parse("link gh"), (Mode::Quicklink, "gh".into()));
        assert_eq!(Mode::parse("links"), (Mode::Quicklink, "".into()));
        assert_eq!(Mode::parse("linkedin").0, Mode::Root);
        assert_eq!(Mode::parse("calc"), (Mode::Calc, "".into()));
        assert_eq!(Mode::parse("calc 2+2"), (Mode::Calc, "2+2".into()));
        assert_eq!(Mode::parse("="), (Mode::Calc, "".into()));
        assert_eq!(Mode::parse("= 20% of 80"), (Mode::Calc, "20% of 80".into()));
        assert_eq!(Mode::parse("calculator").0, Mode::Root);
        assert_eq!(Mode::parse("emoji smile"), (Mode::Emoji, "smile".into()));
        assert_eq!(Mode::parse("emoji"), (Mode::Emoji, "".into()));
        assert_eq!(Mode::parse("emojis"), (Mode::Emoji, "".into()));
        assert_eq!(Mode::parse("gif cats"), (Mode::Gif, "cats".into()));
        assert_eq!(Mode::parse("gif"), (Mode::Gif, "".into()));
        assert_eq!(
            Mode::parse("content invoices"),
            (Mode::Content, "invoices".into())
        );
        assert_eq!(
            Mode::parse("content:api_key"),
            (Mode::Content, "api_key".into())
        );
        assert_eq!(Mode::parse("contentment").0, Mode::Root);
        assert_eq!(Mode::parse("tr fr hello").0, Mode::Root);
        assert_eq!(Mode::parse("translate es hola").0, Mode::Root);
        assert_eq!(Mode::parse("try firefox").0, Mode::Root);
        assert_eq!(Mode::parse("in:secret").0, Mode::Root);
    }
}
