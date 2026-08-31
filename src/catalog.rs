use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::auth;
use crate::clipboard::{self, Store as ClipStore};
use crate::config::Settings;
use crate::desktop;
use crate::hypr;
use crate::item::{Action, Icon, Item, Kind};
use crate::mode::Mode;
use crate::models;
use crate::notes;
use crate::snippets;
use crate::store;
use crate::usage;

pub struct Catalog {
    apps: Vec<Item>,
    commands: Vec<Item>,
    extensions: Vec<Item>,
    usage: std::collections::HashMap<String, u32>,
    clips: Rc<RefCell<ClipStore>>,
    pub settings: Rc<RefCell<Settings>>,
}

#[derive(Clone)]
pub struct Scored {
    pub item: Item,
    pub score: u32,
}

impl Catalog {
    pub fn load(clips: Rc<RefCell<ClipStore>>, settings: Rc<RefCell<Settings>>) -> Self {
        Self {
            apps: desktop::load_apps(),
            commands: system_commands(),
            extensions: extension_items(),
            usage: usage::load(),
            clips,
            settings,
        }
    }

    pub fn search(&self, query: &str) -> (Mode, Vec<Scored>) {
        let (mode, rest) = Mode::parse(query);
        let results = match mode {
            Mode::Root => self.search_root(&rest),
            Mode::Windows => self.search_windows(&rest),
            Mode::Clipboard => self.search_clipboard(&rest),
            Mode::Snippets => self.search_snippets(&rest),
            Mode::Notes => self.search_notes(&rest),
            Mode::Ask => self.search_ask(&rest),
            Mode::Voice => self.search_voice(&rest),
            Mode::Settings => self.search_settings(&rest),
            Mode::Store => self.search_store(&rest),
        };
        (mode, results)
    }

    fn search_root(&self, query: &str) -> Vec<Scored> {
        let query = query.trim();
        let mut results = Vec::new();

        if query.is_empty() {
            return self.empty_state();
        }

        if let Some(item) = calculator_item(query) {
            results.push(Scored {
                item,
                score: 100_000,
            });
        }

        if query.starts_with('>') {
            let cmd = query.trim_start_matches('>').trim();
            if !cmd.is_empty() {
                results.push(Scored {
                    item: run_item(cmd.to_string(), false),
                    score: 90_000,
                });
            }
        } else if query.starts_with('$') {
            let cmd = query.trim_start_matches('$').trim();
            if !cmd.is_empty() {
                results.push(Scored {
                    item: run_item(cmd.to_string(), true),
                    score: 90_000,
                });
            }
        }

        if looks_like_uri(query) {
            let uri = if query.contains("://") {
                query.to_string()
            } else {
                format!("https://{query}")
            };
            results.push(Scored {
                item: Item {
                    id: format!("web:{uri}"),
                    title: format!("Open {uri}"),
                    subtitle: "Open in default browser".into(),
                    keywords: String::new(),
                    kind: Kind::Web,
                    icon: Icon::Name("web-browser".into()),
                    action: Action::OpenUri(uri),
                },
                score: 80_000,
            });
        }

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);

        let mut pool: Vec<&Item> = Vec::new();
        pool.extend(self.apps.iter());
        pool.extend(self.commands.iter());
        pool.extend(self.extensions.iter());
        let windows = hypr::load_windows();
        pool.extend(windows.iter());

        for item in pool {
            if let Some(score) = rank(&mut matcher, &pattern, query, item, &self.usage) {
                results.push(Scored {
                    item: item.clone(),
                    score,
                });
            }
        }

        if query.len() >= 2 {
            for item in file_items(query) {
                let score = rank(&mut matcher, &pattern, query, &item, &self.usage).unwrap_or(1_000);
                results.push(Scored { item, score });
            }
        }

        if !query.starts_with(['>', '$', '=', '/', '~', ';']) {
            let encoded = urlencoding_lite(query);
            results.push(Scored {
                item: Item {
                    id: format!("search:{query}"),
                    title: format!("Search the web for “{query}”"),
                    subtitle: "DuckDuckGo".into(),
                    keywords: "google ddg web".into(),
                    kind: Kind::Web,
                    icon: Icon::Name("system-search".into()),
                    action: Action::OpenUri(format!("https://duckduckgo.com/?q={encoded}")),
                },
                score: 400,
            });
        }

        finish(results)
    }

    fn search_windows(&self, query: &str) -> Vec<Scored> {
        score_pool(&hypr::load_windows(), query, &self.usage, 18)
    }

    fn search_clipboard(&self, query: &str) -> Vec<Scored> {
        let items: Vec<Item> = self
            .clips
            .borrow()
            .entries
            .iter()
            .map(|e| e.to_item())
            .collect();
        score_pool(&items, query, &self.usage, 18)
    }

    fn search_snippets(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let mut results = Vec::new();

        if let Some(keyword) = q.strip_prefix('+').map(str::trim) {
            if !keyword.is_empty() {
                let preview = clipboard::current_text()
                    .map(|t| t.chars().take(64).collect::<String>())
                    .unwrap_or_else(|| "clipboard is empty".into());
                results.push(Scored {
                    item: Item {
                        id: format!("snip-save:{keyword}"),
                        title: format!("Save snippet “{keyword}”"),
                        subtitle: preview,
                        keywords: keyword.to_string(),
                        kind: Kind::Snippet,
                        icon: Icon::Name("document-save".into()),
                        action: Action::SaveSnippet {
                            keyword: keyword.to_string(),
                        },
                    },
                    score: 100_000,
                });
            }
        }

        let items: Vec<Item> = snippets::load().into_iter().map(|s| s.to_item()).collect();
        let rest = q.strip_prefix('+').unwrap_or(q).trim();
        results.extend(score_pool(&items, rest, &self.usage, 18));
        finish(results)
    }

    fn search_notes(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let mut results = Vec::new();
        if let Some(title) = q.strip_prefix('+').map(str::trim) {
            if !title.is_empty() {
                results.push(Scored {
                    item: Item {
                        id: format!("note-new:{title}"),
                        title: format!("New note “{title}”"),
                        subtitle: "Create a quick note".into(),
                        keywords: title.to_string(),
                        kind: Kind::Note,
                        icon: Icon::Name("document-new".into()),
                        action: Action::CreateNote {
                            title: title.to_string(),
                        },
                    },
                    score: 100_000,
                });
            }
        }
        let items: Vec<Item> = notes::load().into_iter().map(|n| n.to_item()).collect();
        let rest = q.strip_prefix('+').unwrap_or(q).trim();
        results.extend(score_pool(&items, rest, &self.usage, 18));
        finish(results)
    }

    fn search_ask(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let settings = self.settings.borrow();
        let mut results = Vec::new();
        if !q.is_empty() {
            results.push(Scored {
                item: Item {
                    id: format!("ask:{q}"),
                    title: format!("Ask “{q}”"),
                    subtitle: format!("{} · {}", settings.ai.provider, settings.ai.model),
                    keywords: q.to_string(),
                    kind: Kind::Ai,
                    icon: Icon::Name("help-faq".into()),
                    action: Action::AskAi {
                        prompt: q.to_string(),
                    },
                },
                score: 100_000,
            });
        }
        results.push(Scored {
            item: Item {
                id: "ask:signin".into(),
                title: "Sign in with OAuth".into(),
                subtitle: auth::signed_in_label(),
                keywords: "oauth openai google subscription".into(),
                kind: Kind::Ai,
                icon: Icon::Name("network-workgroup".into()),
                action: Action::EnterMode(Mode::Settings),
            },
            score: 2_000,
        });
        for model in models::items().into_iter().take(8) {
            results.push(Scored {
                item: model,
                score: 5_000,
            });
        }
        results.push(Scored {
            item: Item {
                id: "ask:settings".into(),
                title: "AI settings".into(),
                subtitle: format!(
                    "Provider {} · model {}",
                    settings.ai.provider, settings.ai.model
                ),
                keywords: "ollama openai anthropic google oauth".into(),
                kind: Kind::Settings,
                icon: Icon::Name("preferences-system".into()),
                action: Action::EnterMode(Mode::Settings),
            },
            score: 1_000,
        });
        results
    }

    fn search_voice(&self, _query: &str) -> Vec<Scored> {
        vec![
            Scored {
                item: Item {
                    id: "voice:toggle".into(),
                    title: "Start dictation".into(),
                    subtitle: "Stay in Flint. Speak, then Enter — the transcript fills the search box."
                        .into(),
                    keywords: "voice dictate speech".into(),
                    kind: Kind::Voice,
                    icon: Icon::Name("audio-input-microphone".into()),
                    action: Action::ToggleVoice,
                },
                score: 100_000,
            },
            Scored {
                item: Item {
                    id: "voice:settings".into(),
                    title: "Voice settings".into(),
                    subtitle: "Language and Whisper model".into(),
                    keywords: "voxtype whisper".into(),
                    kind: Kind::Settings,
                    icon: Icon::Name("preferences-system".into()),
                    action: Action::EnterMode(Mode::Settings),
                },
                score: 1_000,
            },
        ]
    }

    fn search_settings(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let s = self.settings.borrow();
        let mut items = vec![
            setting_toggle(
                "autostart",
                "Launch at login",
                s.general.autostart,
                "daemon autostart",
            ),
            setting_toggle(
                "clip-ai",
                "Attach clipboard to Ask AI",
                s.general.attach_clipboard_to_ai,
                "clipboard context",
            ),
            setting_toggle(
                "scripts",
                "Run unsigned script-commands",
                s.general.allow_script_commands,
                "scripts raycast shell python",
            ),
            setting_toggle(
                "mcp",
                "Allow MCP tool listing",
                s.general.allow_mcp,
                "mcp npx spawn tools",
            ),
            setting_cycle(
                "provider",
                "AI provider",
                &s.ai.provider,
                "ollama openai anthropic google custom",
            ),
            setting_value("model", "AI model", &s.ai.model, q, "model"),
            setting_value("endpoint", "AI endpoint", &s.ai.endpoint, q, "url host"),
            Item {
                id: "set:signin-openai".into(),
                title: "Sign in with OpenAI".into(),
                subtitle: auth::signed_in_label(),
                keywords: "oauth openai chatgpt plus subscription".into(),
                kind: Kind::Ai,
                icon: Icon::Name("network-workgroup".into()),
                action: Action::SignIn {
                    provider: "openai".into(),
                },
            },
            Item {
                id: "set:signin-google".into(),
                title: "Sign in with Google".into(),
                subtitle: "Use a Gemini subscription via OAuth".into(),
                keywords: "oauth google gemini".into(),
                kind: Kind::Ai,
                icon: Icon::Name("network-workgroup".into()),
                action: Action::SignIn {
                    provider: "google".into(),
                },
            },
            Item {
                id: "set:signin-custom".into(),
                title: "Sign in with custom OAuth URL".into(),
                subtitle: "Uses the authorize / token URLs below".into(),
                keywords: "oauth custom url".into(),
                kind: Kind::Ai,
                icon: Icon::Name("network-workgroup".into()),
                action: Action::SignIn {
                    provider: "custom".into(),
                },
            },
            Item {
                id: "set:signout".into(),
                title: "Sign out".into(),
                subtitle: "Forget the stored OAuth token".into(),
                keywords: "logout oauth".into(),
                kind: Kind::Settings,
                icon: Icon::Name("system-log-out".into()),
                action: Action::SignOut,
            },
            setting_value(
                "client-id",
                "OAuth client ID",
                if s.ai.client_id.is_empty() {
                    "(required for sign-in)"
                } else {
                    "••••••••"
                },
                q,
                "oauth client",
            ),
            setting_value(
                "oauth-authorize",
                "OAuth authorize URL",
                if s.ai.oauth_authorize_url.is_empty() {
                    "(custom providers)"
                } else {
                    &s.ai.oauth_authorize_url
                },
                q,
                "oauth url",
            ),
            setting_value(
                "oauth-token",
                "OAuth token URL",
                if s.ai.oauth_token_url.is_empty() {
                    "(custom providers)"
                } else {
                    &s.ai.oauth_token_url
                },
                q,
                "oauth token",
            ),
            setting_value(
                "apikey",
                "AI API key (fallback)",
                if s.ai.api_key.is_empty() {
                    "(not set — prefer OAuth)"
                } else {
                    "••••••••"
                },
                q,
                "secret token",
            ),
            Item {
                id: "set:refresh-models".into(),
                title: "Scan local models".into(),
                subtitle: "Ollama, LM Studio, llama.cpp on this machine".into(),
                keywords: "ollama lmstudio local".into(),
                kind: Kind::Ai,
                icon: Icon::Name("view-refresh".into()),
                action: Action::RefreshModels,
            },
            setting_value("voice-lang", "Voice language", &s.voice.language, q, "locale"),
            setting_value(
                "voice-model",
                "Voice model",
                if s.voice.model.is_empty() {
                    "(voxtype default)"
                } else {
                    &s.voice.model
                },
                q,
                "whisper",
            ),
            Item {
                id: "set:config".into(),
                title: "Open config.json".into(),
                subtitle: crate::config::path().to_string_lossy().into_owned(),
                keywords: "config file".into(),
                kind: Kind::Settings,
                icon: Icon::Name("text-x-generic".into()),
                action: Action::OpenPath(crate::config::path()),
            },
            Item {
                id: "set:store".into(),
                title: "Open Store".into(),
                subtitle: "Vicinae extensions, MCP, Script Commands".into(),
                keywords: "extensions".into(),
                kind: Kind::Store,
                icon: Icon::Name("application-x-addon".into()),
                action: Action::EnterMode(Mode::Store),
            },
        ];
        drop(s);
        if !q.is_empty() && !q.contains('=') {
            items.insert(
                0,
                Item {
                    id: "set:query".into(),
                    title: format!("Set selected field to “{q}”"),
                    subtitle: "Applies to model, endpoint, API key, or language".into(),
                    keywords: q.to_string(),
                    kind: Kind::Settings,
                    icon: Icon::Name("document-edit".into()),
                    action: Action::SaveSettings,
                },
            );
        }
        score_pool(&items, q, &self.usage, 18)
    }

    fn search_store(&self, query: &str) -> Vec<Scored> {
        let settings = self.settings.borrow();
        let items = store::items(&settings);
        score_pool(&items, query, &self.usage, 24)
    }

    fn empty_state(&self) -> Vec<Scored> {
        let mut out = Vec::new();

        for item in &self.extensions {
            out.push(Scored {
                item: item.clone(),
                score: 20_000,
            });
        }

        let mut apps: Vec<&Item> = self.apps.iter().collect();
        apps.sort_by(|a, b| {
            usage_of(&self.usage, &a.id)
                .cmp(&usage_of(&self.usage, &b.id))
                .reverse()
                .then_with(|| a.title.cmp(&b.title))
        });
        for item in apps.into_iter().take(5) {
            out.push(Scored {
                score: 10_000 + usage_of(&self.usage, &item.id),
                item: item.clone(),
            });
        }

        for item in &self.commands {
            out.push(Scored {
                score: 1_000 + usage_of(&self.usage, &item.id),
                item: item.clone(),
            });
        }

        out.sort_by(|a, b| b.score.cmp(&a.score));
        out.truncate(12);
        out
    }
}

fn extension_items() -> Vec<Item> {
    vec![
        Item {
            id: "ext:ask".into(),
            title: "Ask AI".into(),
            subtitle: "Local models, or sign in with a subscription".into(),
            keywords: "ask ai chatgpt ollama claude gemini oauth".into(),
            kind: Kind::Extension,
            icon: Icon::Name("help-faq".into()),
            action: Action::EnterMode(Mode::Ask),
        },
        Item {
            id: "ext:notes".into(),
            title: "Notes".into(),
            subtitle: "Quick notes that stay on this machine".into(),
            keywords: "note notes memo".into(),
            kind: Kind::Extension,
            icon: Icon::Name("accessories-text-editor".into()),
            action: Action::EnterMode(Mode::Notes),
        },
        Item {
            id: "ext:voice".into(),
            title: "Dictation".into(),
            subtitle: "Speak into Flint — transcript fills the search box".into(),
            keywords: "voice dictate speech microphone".into(),
            kind: Kind::Extension,
            icon: Icon::Name("audio-input-microphone".into()),
            action: Action::EnterMode(Mode::Voice),
        },
        Item {
            id: "ext:windows".into(),
            title: "Window Switcher".into(),
            subtitle: "Jump to an open Hyprland client".into(),
            keywords: "win windows switcher alt tab".into(),
            kind: Kind::Extension,
            icon: Icon::Name("preferences-system-windows".into()),
            action: Action::EnterMode(Mode::Windows),
        },
        Item {
            id: "ext:clipboard".into(),
            title: "Clipboard History".into(),
            subtitle: "Search and paste recent copies".into(),
            keywords: "clip clipboard paste history".into(),
            kind: Kind::Extension,
            icon: Icon::Name("edit-paste".into()),
            action: Action::EnterMode(Mode::Clipboard),
        },
        Item {
            id: "ext:snippets".into(),
            title: "Snippets".into(),
            subtitle: "Expand saved text · type +name to save".into(),
            keywords: "snip snippet text expand".into(),
            kind: Kind::Extension,
            icon: Icon::Name("insert-text".into()),
            action: Action::EnterMode(Mode::Snippets),
        },
        Item {
            id: "ext:store".into(),
            title: "Store".into(),
            subtitle: "Vicinae extensions, MCP, Script Commands".into(),
            keywords: "store extensions mcp raycast vicinae".into(),
            kind: Kind::Extension,
            icon: Icon::Name("application-x-addon".into()),
            action: Action::EnterMode(Mode::Store),
        },
        Item {
            id: "ext:settings".into(),
            title: "Settings".into(),
            subtitle: "OAuth, local models, voice, autostart".into(),
            keywords: "prefs preferences config".into(),
            kind: Kind::Extension,
            icon: Icon::Name("preferences-system".into()),
            action: Action::EnterMode(Mode::Settings),
        },
    ]
}

fn setting_toggle(id: &str, title: &str, on: bool, keywords: &str) -> Item {
    Item {
        id: format!("set:{id}"),
        title: title.into(),
        subtitle: if on { "On · Enter to disable" } else { "Off · Enter to enable" }.into(),
        keywords: keywords.into(),
        kind: Kind::Settings,
        icon: Icon::Name("preferences-system".into()),
        action: Action::SaveSettings,
    }
}

fn setting_cycle(id: &str, title: &str, value: &str, keywords: &str) -> Item {
    Item {
        id: format!("set:{id}"),
        title: title.into(),
        subtitle: format!("{value} · Enter to cycle"),
        keywords: keywords.into(),
        kind: Kind::Settings,
        icon: Icon::Name("preferences-system".into()),
        action: Action::SaveSettings,
    }
}

fn setting_value(id: &str, title: &str, current: &str, typed: &str, keywords: &str) -> Item {
    let subtitle = if typed.is_empty() {
        format!("{current} · type a value, then Enter")
    } else {
        format!("Set to “{typed}”")
    };
    Item {
        id: format!("set:{id}"),
        title: title.into(),
        subtitle,
        keywords: keywords.into(),
        kind: Kind::Settings,
        icon: Icon::Name("preferences-system".into()),
        action: Action::SaveSettings,
    }
}

fn score_pool(
    items: &[Item],
    query: &str,
    usage_map: &std::collections::HashMap<String, u32>,
    limit: usize,
) -> Vec<Scored> {
    let query = query.trim();
    if query.is_empty() {
        return items
            .iter()
            .enumerate()
            .map(|(i, item)| Scored {
                item: item.clone(),
                score: 10_000u32.saturating_sub((i as u32) * 10),
            })
            .take(limit)
            .collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut results = Vec::new();
    for item in items {
        if let Some(score) = rank(&mut matcher, &pattern, query, item, usage_map) {
            results.push(Scored {
                item: item.clone(),
                score,
            });
        }
    }
    finish_limited(results, limit)
}

fn finish(results: Vec<Scored>) -> Vec<Scored> {
    finish_limited(results, 12)
}

fn finish_limited(mut results: Vec<Scored>, limit: usize) -> Vec<Scored> {
    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.item.title.cmp(&b.item.title))
    });
    results.dedup_by(|a, b| a.item.id == b.item.id);
    results.truncate(limit);
    results
}

fn rank(
    matcher: &mut Matcher,
    pattern: &Pattern,
    query: &str,
    item: &Item,
    usage_map: &std::collections::HashMap<String, u32>,
) -> Option<u32> {
    let mut buf = Vec::new();
    let title = item.title.to_lowercase();
    let hay = item.haystack();
    let title_score = pattern.score(Utf32Str::new(&item.title, &mut buf), matcher);
    buf.clear();
    let hay_score = pattern.score(Utf32Str::new(&hay, &mut buf), matcher)?;
    let mut score = hay_score as u32;
    if let Some(ts) = title_score {
        score = score.saturating_add((ts as u32).saturating_mul(2));
    }
    let q = query.to_lowercase();
    if title.starts_with(&q) {
        score = score.saturating_add(8_000);
    }
    score = score.saturating_add(usage_of(usage_map, &item.id).saturating_mul(40));
    score = score.saturating_add(match item.kind {
        Kind::Extension => 120,
        Kind::Ai | Kind::Note => 90,
        Kind::App => 80,
        Kind::Window => 70,
        Kind::Snippet => 70,
        Kind::Command | Kind::Settings | Kind::Store => 60,
        Kind::Clipboard | Kind::Voice | Kind::Script => 50,
        Kind::File => 30,
        _ => 10,
    });
    Some(score)
}

fn usage_of(map: &std::collections::HashMap<String, u32>, id: &str) -> u32 {
    map.get(id).copied().unwrap_or(0)
}

fn calculator_item(query: &str) -> Option<Item> {
    let trimmed = query.trim();
    let expr = trimmed.strip_prefix('=').unwrap_or(trimmed).trim();
    if expr.is_empty() || expr.len() > 200 {
        return None;
    }
    let forced = trimmed.starts_with('=');
    if !forced && !looks_like_math(expr) {
        return None;
    }
    let value = evalexpr::eval(expr).ok()?;
    let rendered = value.to_string();
    Some(Item {
        id: format!("calc:{expr}"),
        title: rendered.clone(),
        subtitle: format!("{expr}  →  copy result"),
        keywords: "calculator math".into(),
        kind: Kind::Calc,
        icon: Icon::Name("accessories-calculator".into()),
        action: Action::Copy(rendered),
    })
}

fn looks_like_math(expr: &str) -> bool {
    let has_digit = expr.chars().any(|c| c.is_ascii_digit());
    let has_op = expr.chars().any(|c| "+-*/%^()".contains(c))
        || expr.contains("sqrt")
        || expr.contains("sin")
        || expr.contains("cos")
        || expr.contains("pi");
    has_digit && has_op
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
            && q
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-_:/?#=&".contains(c)))
}

fn run_item(command: String, terminal: bool) -> Item {
    let kind = if terminal { Kind::Shell } else { Kind::Command };
    Item {
        id: format!("run:{command}"),
        title: command.clone(),
        subtitle: if terminal {
            "Run in terminal".into()
        } else {
            "Run command".into()
        },
        keywords: String::new(),
        kind,
        icon: Icon::Name("utilities-terminal".into()),
        action: Action::Shell { command, terminal },
    }
}

fn file_items(query: &str) -> Vec<Item> {
    let q = query.trim();
    let path_like = q.starts_with('/') || q.starts_with("~/") || q.starts_with("./");
    let explicit = q.starts_with("f ") || q.starts_with("file ");
    if !path_like && !explicit && q.len() < 3 {
        return Vec::new();
    }

    if path_like {
        let expanded = expand_tilde(q);
        let path = PathBuf::from(&expanded);
        if path.is_file() {
            return vec![file_item(path)];
        }
        if path.is_dir() {
            return list_dir(&path);
        }
    }

    let expanded = expand_tilde(q);
    let term = if let Some(rest) = q.strip_prefix("file ").or_else(|| q.strip_prefix("f ")) {
        rest.trim()
    } else if path_like {
        Path::new(&expanded)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(q)
    } else {
        q
    };
    fd_search(term)
}

fn list_dir(path: &Path) -> Vec<Item> {
    let mut items = Vec::new();
    let Ok(entries) = std::fs::read_dir(path) else {
        return items;
    };
    for entry in entries.flatten().take(20) {
        items.push(file_item(entry.path()));
    }
    items.sort_by(|a, b| a.title.cmp(&b.title));
    items
}

fn fd_search(term: &str) -> Vec<Item> {
    if term.is_empty() {
        return Vec::new();
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let output = Command::new("fd")
        .args([
            "--color=never",
            "--max-results",
            "20",
            "--exclude",
            ".git",
            "--exclude",
            "node_modules",
            "--exclude",
            "target",
            term,
        ])
        .current_dir(&home)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let path = if Path::new(line).is_absolute() {
                PathBuf::from(line)
            } else {
                home.join(line)
            };
            file_item(path)
        })
        .collect()
}

fn file_item(path: PathBuf) -> Item {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let subtitle = path.to_string_lossy().to_string();
    let icon = if path.is_dir() {
        Icon::Name("folder".into())
    } else {
        Icon::Name("text-x-generic".into())
    };
    Item {
        id: format!("file:{}", path.display()),
        title: name,
        subtitle,
        keywords: String::new(),
        kind: Kind::File,
        icon,
        action: Action::OpenPath(path),
    }
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().into_owned();
        }
    }
    path.to_string()
}

fn urlencoding_lite(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn system_commands() -> Vec<Item> {
    let mut items = vec![
        cmd(
            "lock",
            "Lock screen",
            "Sleep the session behind a lock",
            "system-lock-screen",
            spawn("omarchy-system-lock"),
        ),
        cmd(
            "shot",
            "Screenshot",
            "Capture the current screen",
            "applets-screenshooter",
            spawn("omarchy-capture-screenshot"),
        ),
        cmd(
            "region",
            "Capture region",
            "Select an area to screenshot",
            "applets-screenshooter",
            spawn("omarchy-capture-region"),
        ),
        cmd(
            "sleep",
            "Sleep",
            "Suspend the machine",
            "system-suspend",
            Action::Spawn {
                program: "systemctl".into(),
                args: vec!["suspend".into()],
            },
        ),
        cmd(
            "term",
            "Terminal",
            "Open a new terminal",
            "utilities-terminal",
            spawn("omarchy-launch-terminal"),
        ),
        cmd(
            "files",
            "Files",
            "Open the file manager",
            "system-file-manager",
            spawn("omarchy-launch-nautilus"),
        ),
        cmd(
            "theme",
            "Theme switcher",
            "Cycle or pick an Omarchy theme",
            "preferences-desktop-theme",
            spawn("omarchy-theme-switcher"),
        ),
        cmd(
            "wifi",
            "Wi-Fi",
            "Open network settings",
            "network-wireless",
            Action::Spawn {
                program: "omarchy-launch-or-focus".into(),
                args: vec!["nmtui".into()],
            },
        ),
        cmd(
            "emoji",
            "Emoji",
            "Insert an emoji",
            "face-smile",
            spawn("omarchy-menu-emoji"),
        ),
    ];

    items.retain(|item| match &item.action {
        Action::Spawn { program, .. } => program.contains('/') || which(program),
        _ => true,
    });
    items
}

fn cmd(id: &str, title: &str, subtitle: &str, icon: &str, action: Action) -> Item {
    Item {
        id: format!("cmd:{id}"),
        title: title.into(),
        subtitle: subtitle.into(),
        keywords: "system command".into(),
        kind: Kind::Command,
        icon: Icon::Name(icon.into()),
        action,
    }
}

fn spawn(program: &str) -> Action {
    Action::Spawn {
        program: program.into(),
        args: Vec::new(),
    }
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .any(|dir| dir.join(bin).is_file())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{looks_like_math, looks_like_uri};
    use crate::mode::Mode;

    #[test]
    fn math_detection() {
        assert!(looks_like_math("2+2"));
        assert!(looks_like_math("sqrt(9)"));
        assert!(!looks_like_math("firefox"));
    }

    #[test]
    fn uri_detection() {
        assert!(looks_like_uri("https://example.com"));
        assert!(looks_like_uri("omarchy.org"));
        assert!(!looks_like_uri("open firefox please"));
    }

    #[test]
    fn mode_prefix_still_works() {
        assert_eq!(Mode::parse("clip rust").0, Mode::Clipboard);
    }
}
