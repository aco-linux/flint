use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::alias;
use crate::auth;
use crate::calc;
use crate::clipboard::{self, Store as ClipStore};
use crate::config::Settings;
use crate::desktop;
use crate::favorites;
use crate::files;
use crate::hypr;
use crate::intent::{self, IntentKind};
use crate::item::{Action, Icon, Item, Kind, Live};
use crate::mode::Mode;
use crate::models;
use crate::notes;
use crate::quicklinks;
use crate::smart;
use crate::snippets;
use crate::store;
use crate::usage;
use crate::weather;

pub struct Catalog {
    apps: Vec<Item>,
    commands: Vec<Item>,
    extensions: Vec<Item>,
    installed: RefCell<Vec<Item>>,
    lexicon: Vec<String>,
    haystacks: RefCell<HashMap<String, String>>,
    windows: RefCell<Vec<Item>>,
    matcher: RefCell<Matcher>,
    pub(crate) usage: usage::Map,
    pub(crate) clips: Rc<RefCell<ClipStore>>,
    pub settings: Rc<RefCell<Settings>>,
}

#[derive(Clone)]
pub struct Scored {
    pub item: Item,
    pub score: u32,
    pub live: Live,
}

impl Scored {
    pub(crate) fn new(item: Item, score: u32) -> Self {
        let live = Live::from_item(&item);
        Self { item, score, live }
    }

    fn with_live(item: Item, score: u32, live: Live) -> Self {
        Self { item, score, live }
    }
}

#[derive(Clone, Default)]
pub struct LiveExtras {
    pub files: Vec<Scored>,
    pub weather: Option<Scored>,
}

impl Catalog {
    pub fn load(clips: Rc<RefCell<ClipStore>>, settings: Rc<RefCell<Settings>>) -> Self {
        let apps = desktop::load_apps();
        let commands = system_commands();
        let extensions = extension_items();
        let installed = crate::extension::command_items();
        let mut haystacks = HashMap::new();
        let mut lexicon = files::type_words();
        for item in apps
            .iter()
            .chain(commands.iter())
            .chain(extensions.iter())
            .chain(installed.iter())
        {
            haystacks.insert(item.id.clone(), item.haystack());
            lexicon.push(item.title.clone());
            for part in item
                .title
                .split(|c: char| !c.is_ascii_alphanumeric())
                .chain(item.keywords.split_whitespace())
            {
                if part.len() >= 4 {
                    lexicon.push(part.to_string());
                }
            }
        }
        let windows = hypr::load_windows();
        for item in &windows {
            haystacks.insert(item.id.clone(), item.haystack());
        }
        Self {
            apps,
            commands,
            extensions,
            installed: RefCell::new(installed),
            lexicon,
            haystacks: RefCell::new(haystacks),
            windows: RefCell::new(windows),
            matcher: RefCell::new(Matcher::new(Config::DEFAULT)),
            usage: usage::load(),
            clips,
            settings,
        }
    }

    /// Re-scan installed extension commands after a store install.
    pub fn reload_installed(&self) {
        let installed = crate::extension::command_items();
        let mut cache = self.haystacks.borrow_mut();
        cache.retain(|id, _| !id.starts_with("vx:"));
        for item in &installed {
            cache.insert(item.id.clone(), item.haystack());
        }
        *self.installed.borrow_mut() = installed;
    }

    pub fn adopt_windows(&self, windows: Vec<Item>) {
        let mut cache = self.haystacks.borrow_mut();
        cache.retain(|id, _| !id.starts_with("win:"));
        for item in &windows {
            cache.insert(item.id.clone(), item.haystack());
        }
        *self.windows.borrow_mut() = windows;
    }

    pub fn score_windows(&self, query: &str) -> Vec<Scored> {
        let windows = self.windows.borrow();
        self.score_pool(&windows, query, 18)
    }

    pub fn search_fast(&self, query: &str) -> (Mode, Vec<Scored>) {
        let (mode, rest) = Mode::parse(query);
        let results = match mode {
            Mode::Root => self.search_root(&rest),
            Mode::Files => self.search_files(&rest),
            Mode::Windows => self.search_windows(&rest),
            Mode::Clipboard => self.search_clipboard(&rest),
            Mode::Snippets => self.search_snippets(&rest),
            Mode::Notes => self.search_notes(&rest),
            Mode::Ask => self.search_ask(&rest),
            Mode::Voice => self.search_voice(&rest),
            Mode::Settings => self.search_settings(&rest),
            Mode::Store => self.search_store(&rest),
            Mode::Quicklink => self.search_quicklinks(&rest),
            Mode::Calc => self.search_calc(&rest),
            Mode::Emoji => self.search_emoji(&rest),
            Mode::Content => self.search_content(&rest),
            Mode::Extension => self.search_root(&rest),
        };
        (mode, results)
    }

    fn search_root(&self, query: &str) -> Vec<Scored> {
        let query = query.trim();
        let mut results = Vec::new();
        let settings = self.settings.borrow();
        let include_in_root = settings.files.include_in_root;
        let file_limit = settings.files.max_results.min(files::ROOT_FILE_LIMIT);
        let mix_limit = settings.general.max_results.max(24);
        drop(settings);

        if query.is_empty() {
            return self.empty_state();
        }

        if let Some(item) = crate::focus::live_item() {
            let hay = format!("{} {}", item.title, item.keywords).to_ascii_lowercase();
            if hay.contains(&query.to_ascii_lowercase()) {
                results.push(Scored::new(item, 110_000));
            }
        }

        if let Some(id) = alias::Store::load().lookup(query)
            && let Some(item) = self.lookup_item(id)
        {
            results.push(Scored::new(item, 120_000));
        }

        if let Some(item) = calc::answer_item(query) {
            results.push(Scored::new(item, 100_000));
        }
        for item in smart::instant_items(query) {
            results.push(Scored::new(item, 95_000));
        }
        if let Some(item) = crate::translate::item(query) {
            results.push(Scored::new(item, 98_000));
        }
        for (i, item) in crate::memory::items(query).into_iter().enumerate() {
            results.push(Scored::new(item, 99_000u32.saturating_sub(i as u32)));
        }
        for item in crate::tz::items(query) {
            results.push(Scored::new(item, 96_000));
        }
        if query.chars().count() >= 2 {
            let looks = crate::emoji::looks_like_query(query);
            let n = if looks { 8 } else { 3 };
            for (i, item) in crate::emoji::search(query, n).into_iter().enumerate() {
                let score = if looks {
                    92_000u32.saturating_sub(i as u32 * 10)
                } else {
                    3_000
                };
                results.push(Scored::new(item, score));
            }
        }

        let lexicon: Vec<&str> = self.lexicon.iter().map(String::as_str).collect();
        let meaning = intent::resolve_with(query, &lexicon);
        for hit in &meaning.intents {
            match hit.kind {
                IntentKind::Weather => results.push(Scored::new(
                    weather::item(weather::cached().as_ref()),
                    70_000 + hit.score,
                )),
                IntentKind::Time => {
                    results.push(Scored::new(weather::time_item(), 60_000 + hit.score))
                }
            }
        }

        if query.starts_with('>') {
            let cmd = query.trim_start_matches('>').trim();
            if !cmd.is_empty() {
                results.push(Scored::new(run_item(cmd.to_string(), false), 90_000));
            }
        } else if query.starts_with('$') {
            let cmd = query.trim_start_matches('$').trim();
            if !cmd.is_empty() {
                results.push(Scored::new(run_item(cmd.to_string(), true), 90_000));
            }
        }

        if let Some(name) = crate::layout::parse_save_query(query) {
            results.push(Scored::new(crate::layout::save_item(&name), 100_000));
        }

        if looks_like_uri(query) {
            let uri = if query.contains("://") {
                query.to_string()
            } else {
                format!("https://{query}")
            };
            results.push(Scored::new(
                Item {
                    id: format!("web:{uri}"),
                    title: format!("Open {uri}"),
                    subtitle: "Open in default browser".into(),
                    keywords: String::new(),
                    kind: Kind::Web,
                    icon: Icon::Name("web-browser".into()),
                    action: Action::OpenUri(uri),
                },
                80_000,
            ));
        }

        if let Some(item) = smart::path_command(query) {
            results.push(Scored::new(item, 12_000));
        }

        let file_query = files::parse_query(query);
        let file_heavy = file_query.is_type_search() || file_query.path_like || file_query.explicit;
        let now = usage::now_secs();
        let hay = self.haystacks.borrow();
        let windows = self.windows.borrow();
        let installed = self.installed.borrow();
        let mut matcher = self.matcher.borrow_mut();
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let aliases = alias::Store::load();
        let favs = favorites::Store::load();
        let qlinks: Vec<Item> = quicklinks::load()
            .iter()
            .map(|link| link.to_item(&link.argument_for(query)))
            .collect();
        let custom_layouts = crate::layout::custom_items();

        let mut pool: Vec<&Item> = Vec::new();
        pool.extend(self.apps.iter());
        pool.extend(self.commands.iter());
        pool.extend(self.extensions.iter());
        pool.extend(installed.iter());
        pool.extend(windows.iter());
        pool.extend(qlinks.iter());
        pool.extend(custom_layouts.iter());

        let mut ranked: Vec<(u32, &Item)> = Vec::new();
        for item in pool {
            let glued = hay_with_alias(
                item,
                hay.get(&item.id).map(String::as_str),
                aliases.get(&item.id),
            );
            let haystack = glued.as_str();
            let input = RankInput {
                query,
                item,
                haystack,
                usage: &self.usage,
                now,
                file_heavy,
                alias: aliases.get(&item.id),
                favorite: favs.is_pinned(&item.id),
            };
            if let Some(score) = rank(&mut matcher, &pattern, input) {
                ranked.push((score, item));
            } else if let Some(score) = rank_expansions(
                &mut matcher,
                &meaning.expansions,
                RankInput {
                    query,
                    item,
                    haystack,
                    usage: &self.usage,
                    now,
                    file_heavy,
                    alias: aliases.get(&item.id),
                    favorite: favs.is_pinned(&item.id),
                },
            ) {
                ranked.push((score, item));
            }
        }

        results.extend(take_scored(ranked, mix_limit.max(file_limit)));

        if include_in_root && file_query.wants_files() {
            for item in files::well_known_folders(query) {
                let scratch = item.haystack();
                let score = rank(
                    &mut matcher,
                    &pattern,
                    RankInput {
                        query,
                        item: &item,
                        haystack: &scratch,
                        usage: &self.usage,
                        now,
                        file_heavy,
                        alias: aliases.get(&item.id),
                        favorite: favs.is_pinned(&item.id),
                    },
                )
                .unwrap_or(8_000);
                results.push(Scored::new(item, score));
            }
        }

        drop(matcher);
        drop(windows);
        drop(hay);

        if !query.starts_with(['>', '$', '=', '/', '~', ';']) {
            let encoded = urlencoding_lite(query);
            results.push(Scored::new(
                Item {
                    id: format!("search:{query}"),
                    title: format!("Search the web for “{query}”"),
                    subtitle: "DuckDuckGo".into(),
                    keywords: "google ddg web".into(),
                    kind: Kind::Web,
                    icon: Icon::Name("system-search".into()),
                    action: Action::OpenUri(format!("https://duckduckgo.com/?q={encoded}")),
                },
                400,
            ));
        }

        let limit = if file_heavy {
            file_limit.max(mix_limit)
        } else {
            mix_limit
        };
        finish_limited(results, limit)
    }

    fn search_files(&self, query: &str) -> Vec<Scored> {
        let settings = self.settings.borrow();
        let limit = settings.files.max_results.max(files::FILES_MODE_LIMIT);
        drop(settings);

        let q = query.trim();
        let mut results = Vec::new();

        if q.is_empty() {
            let now = usage::now_secs();
            let mut used: Vec<(String, u32)> = self
                .usage
                .iter()
                .filter(|(id, _)| id.starts_with("file:"))
                .map(|(id, rec)| (id.clone(), usage::score(Some(rec), now)))
                .collect();
            used.sort_by_key(|a| std::cmp::Reverse(a.1));
            for (id, score) in used.into_iter().take(24) {
                let path = std::path::PathBuf::from(id.trim_start_matches("file:"));
                if path.exists() {
                    let item = files::file_item(path, None);
                    results.push(Scored::new(item, 20_000 + score));
                }
            }
            return finish_limited(results, limit);
        }

        for item in files::well_known_folders(q) {
            results.push(Scored::new(item, 30_000));
        }
        finish_limited(results, limit)
    }

    fn search_windows(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let mut results = Vec::new();
        if let Some(name) =
            crate::layout::parse_save_short(q).or_else(|| crate::layout::parse_save_query(q))
        {
            results.push(Scored::new(crate::layout::save_item(&name), 100_000));
        }
        let layouts = crate::layout::all_items();
        if q.is_empty() {
            for (i, item) in layouts.iter().enumerate() {
                results.push(Scored::new(
                    item.clone(),
                    50_000u32.saturating_sub(i as u32),
                ));
            }
            results.extend(self.score_windows(q));
            return results;
        }
        results.extend(self.score_pool(&layouts, q, 24));
        results.extend(self.score_windows(q));
        finish(results)
    }

    fn score_pool(&self, items: &[Item], query: &str, limit: usize) -> Vec<Scored> {
        score_pool(
            items,
            query,
            &self.usage,
            limit,
            &mut self.matcher.borrow_mut(),
            &self.haystacks.borrow(),
        )
    }

    fn search_clipboard(&self, query: &str) -> Vec<Scored> {
        let items: Vec<Item> = self
            .clips
            .borrow()
            .entries
            .iter()
            .map(|e| e.to_item())
            .collect();
        let limit = clipboard::MAX_ENTRIES.max(items.len());
        if query.trim().is_empty() {
            return items.into_iter().map(|item| Scored::new(item, 1)).collect();
        }
        self.score_pool(&items, query, limit)
    }

    fn search_snippets(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let mut results = Vec::new();

        if let Some(keyword) = q.strip_prefix('+').map(str::trim)
            && !keyword.is_empty()
        {
            let preview = match clipboard::current_text() {
                Some(text) if clipboard::looks_secret(&text) => {
                    "clipboard looks like a secret — will not save".into()
                }
                Some(text) => text.chars().take(64).collect::<String>(),
                None => "clipboard is empty".into(),
            };
            results.push(Scored::new(
                Item {
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
                100_000,
            ));
        }

        let items: Vec<Item> = snippets::load().into_iter().map(|s| s.to_item()).collect();
        let rest = q.strip_prefix('+').unwrap_or(q).trim();
        results.extend(self.score_pool(&items, rest, 18));
        finish(results)
    }

    fn search_notes(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let mut results = Vec::new();
        let selection = notes::from_selection_item();
        if q.is_empty() {
            results.push(Scored::new(selection.clone(), 90_000));
        }
        if let Some(title) = q.strip_prefix('+').map(str::trim)
            && !title.is_empty()
        {
            results.push(Scored::new(
                Item {
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
                100_000,
            ));
        }
        let mut items = vec![selection];
        items.extend(notes::load().into_iter().map(|n| n.to_item()));
        let rest = q.strip_prefix('+').unwrap_or(q).trim();
        results.extend(self.score_pool(&items, rest, 18));
        finish(results)
    }

    fn search_ask(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let settings = self.settings.borrow();
        let mut results = Vec::new();
        let follow = crate::ai::current_thread();
        if !q.is_empty() {
            let subtitle = if let Some(thread) = &follow {
                format!(
                    "Follow-up · {} · {} · {}",
                    thread.title, settings.ai.provider, settings.ai.model
                )
            } else {
                format!("{} · {}", settings.ai.provider, settings.ai.model)
            };
            results.push(Scored::new(
                Item {
                    id: format!("ask:{q}"),
                    title: format!("Ask “{q}”"),
                    subtitle,
                    keywords: q.to_string(),
                    kind: Kind::Ai,
                    icon: Icon::Name("help-faq".into()),
                    action: Action::AskAi {
                        prompt: q.to_string(),
                    },
                },
                100_000,
            ));
        }
        results.push(Scored::new(
            crate::ai::new_chat_item(),
            if follow.is_some() { 80_000 } else { 8_000 },
        ));
        let threads = crate::ai::thread_items(q);
        for (i, item) in threads.into_iter().enumerate() {
            results.push(Scored::new(item, 50_000u32.saturating_sub(i as u32)));
        }
        results.push(Scored::new(
            Item {
                id: "ask:signin".into(),
                title: "Sign in with OAuth".into(),
                subtitle: auth::signed_in_label(),
                keywords: "oauth openai google subscription".into(),
                kind: Kind::Ai,
                icon: Icon::Name("network-workgroup".into()),
                action: Action::EnterMode(Mode::Settings),
            },
            2_000,
        ));
        for model in models::items().into_iter().take(8) {
            results.push(Scored::new(model, 5_000));
        }
        results.push(Scored::new(
            Item {
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
            1_000,
        ));
        results
    }

    fn search_voice(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let mut results = vec![Scored::new(
            Item {
                id: "voice:toggle".into(),
                title: "Start dictation".into(),
                subtitle: "Stay in Flint. Speak, then Enter — the transcript fills the search box."
                    .into(),
                keywords: "voice dictate speech".into(),
                kind: Kind::Voice,
                icon: Icon::Name("audio-input-microphone".into()),
                action: Action::ToggleVoice,
            },
            100_000,
        )];
        for item in crate::voice::command_items() {
            results.push(Scored::new(item, 90_000));
        }
        if let Some(last) = crate::voice::last_text() {
            results.push(Scored::new(
                Item {
                    id: "voice:wtype-last".into(),
                    title: "Paste last dictation with wtype".into(),
                    subtitle: last.chars().take(64).collect::<String>(),
                    keywords: format!("wtype paste focused {last}"),
                    kind: Kind::Voice,
                    icon: Icon::Name("input-keyboard".into()),
                    action: Action::TypeText(last.clone()),
                },
                80_000,
            ));
            for item in crate::voice::postprocess_items(&last) {
                results.push(Scored::new(item, 70_000));
            }
        }
        results.push(Scored::new(
            Item {
                id: "voice:settings".into(),
                title: "Voice settings".into(),
                subtitle: "Language and Whisper model".into(),
                keywords: "voxtype whisper".into(),
                kind: Kind::Settings,
                icon: Icon::Name("preferences-system".into()),
                action: Action::EnterMode(Mode::Settings),
            },
            1_000,
        ));
        let hist = crate::voice::history_items();
        if q.is_empty() {
            for (i, item) in hist.into_iter().enumerate() {
                results.push(Scored::new(item, 50_000u32.saturating_sub(i as u32)));
            }
            return results;
        }
        results.extend(self.score_pool(&hist, q, 48));
        finish(results)
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
                "extensions",
                "Run installed extensions",
                s.general.allow_extensions,
                "extensions vicinae raycast node",
            ),
            setting_toggle(
                "mcp",
                "Allow MCP tool listing",
                s.general.allow_mcp,
                "mcp npx spawn tools",
            ),
            setting_toggle(
                "files-root",
                "Include files in root search",
                s.files.include_in_root,
                "raycast files launcher",
            ),
            setting_toggle(
                "files-system",
                "System-wide file search",
                s.files.system_wide,
                "locate plocate entire disk markdown",
            ),
            setting_toggle(
                "files-hidden",
                "Search hidden files",
                s.files.include_hidden,
                "dotfiles hidden",
            ),
            setting_value(
                "max-results",
                "Max search results",
                &format!(
                    "root {} · files {}",
                    s.general.max_results, s.files.max_results
                ),
                q,
                "limit scroll",
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
                id: "set:signin-google".into(),
                title: "Connect Google Gemini API with OAuth".into(),
                subtitle: auth::signed_in_label(),
                keywords: "oauth google gemini api cloud project".into(),
                kind: Kind::Ai,
                icon: Icon::Name("network-workgroup".into()),
                action: Action::SignIn {
                    provider: "google".into(),
                },
            },
            Item {
                id: "set:signin-custom".into(),
                title: "Connect a custom API with OAuth".into(),
                subtitle: "PKCE · authorize/token URLs · token bound to the API origin".into(),
                keywords: "oauth custom url api provider".into(),
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
                "oauth-scopes",
                "Custom OAuth scopes",
                if s.ai.oauth_scopes.is_empty() {
                    "(space-separated scopes)"
                } else {
                    &s.ai.oauth_scopes
                },
                q,
                "oauth scopes permissions",
            ),
            setting_value(
                "oauth-project",
                "Google OAuth quota project ID",
                if s.ai.oauth_project_id.is_empty() {
                    "(required for Google API OAuth)"
                } else {
                    &s.ai.oauth_project_id
                },
                q,
                "oauth google cloud quota project",
            ),
            setting_value(
                "apikey",
                "API key for selected provider",
                if auth::has_api_key(&s.ai.provider) {
                    "•••••••• (stored)"
                } else {
                    "(not set)"
                },
                q,
                "secret token",
            ),
            Item {
                id: "set:clear-apikey".into(),
                title: "Remove selected provider API key".into(),
                subtitle: format!("Delete the stored {} credential", s.ai.provider),
                keywords: "delete clear api key secret".into(),
                kind: Kind::Settings,
                icon: Icon::Name("edit-delete".into()),
                action: Action::SaveSettings,
            },
            Item {
                id: "set:refresh-models".into(),
                title: "Scan local models".into(),
                subtitle: "Ollama, LM Studio, llama.cpp on this machine".into(),
                keywords: "ollama lmstudio local".into(),
                kind: Kind::Ai,
                icon: Icon::Name("view-refresh".into()),
                action: Action::RefreshModels,
            },
            setting_value(
                "voice-lang",
                "Voice language",
                &s.voice.language,
                q,
                "locale",
            ),
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
        self.score_pool(&items, q, 36)
    }

    fn search_store(&self, query: &str) -> Vec<Scored> {
        let settings = self.settings.borrow();
        let items = store::items(&settings);
        self.score_pool(&items, query, 36)
    }

    fn search_quicklinks(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let mut results = Vec::new();
        if let Some((name, target)) = quicklinks::parse_create(q) {
            let ok = quicklinks::is_safe_target(&target);
            results.push(Scored::new(
                Item {
                    id: format!("link-save:{name}"),
                    title: format!("Save quicklink “{name}”"),
                    subtitle: if ok {
                        target.clone()
                    } else {
                        "Rejected — http, https, file, or a filesystem path only".into()
                    },
                    keywords: name.clone(),
                    kind: Kind::Web,
                    icon: Icon::Name("document-save".into()),
                    action: Action::SaveQuicklink { name, target },
                },
                100_000,
            ));
        }
        let items: Vec<Item> = quicklinks::load()
            .iter()
            .map(|link| {
                let rest = q.strip_prefix('+').unwrap_or(q).trim();
                link.to_item(&link.argument_for(rest))
            })
            .collect();
        let rest = q.strip_prefix('+').unwrap_or(q).trim();
        results.extend(self.score_pool(&items, rest, 24));
        finish(results)
    }

    fn search_emoji(&self, query: &str) -> Vec<Scored> {
        crate::emoji::search(query, 48)
            .into_iter()
            .enumerate()
            .map(|(i, item)| Scored::new(item, 50_000u32.saturating_sub(i as u32)))
            .collect()
    }

    fn search_content(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        vec![Scored::new(
            Item {
                id: format!("content:{q}"),
                title: format!("Search contents for “{q}”"),
                subtitle: "ripgrep · $HOME and extra folders · cancelled on the next key".into(),
                keywords: q.to_string(),
                kind: Kind::Command,
                icon: Icon::Name("system-search".into()),
                action: Action::Copy(q.to_string()),
            },
            1_000,
        )]
    }

    fn search_calc(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let mut results = Vec::new();
        if !q.is_empty() {
            if let Some(item) = calc::answer_item(q) {
                results.push(Scored::new(item, 100_000));
            }
            for item in smart::instant_items(q) {
                results.push(Scored::new(item, 95_000));
            }
        }
        let history = calc::History::load().items();
        results.extend(self.score_pool(&history, q, 24));
        finish(results)
    }

    pub(crate) fn lookup_item(&self, id: &str) -> Option<Item> {
        if let Some(item) = self
            .apps
            .iter()
            .chain(self.commands.iter())
            .chain(self.extensions.iter())
            .find(|item| item.id == id)
        {
            return Some(item.clone());
        }
        if let Some(item) = self.installed.borrow().iter().find(|item| item.id == id) {
            return Some(item.clone());
        }
        if let Some(item) = self.windows.borrow().iter().find(|item| item.id == id) {
            return Some(item.clone());
        }
        if let Some(clip) = id.strip_prefix("clip:") {
            return self.clips.borrow().get(clip).map(|e| e.to_item());
        }
        if let Some(keyword) = id.strip_prefix("snip:") {
            return snippets::load()
                .into_iter()
                .find(|s| s.keyword == keyword)
                .map(|s| s.to_item());
        }
        if let Some(name) = id.strip_prefix("link:") {
            return quicklinks::load()
                .into_iter()
                .find(|link| link.name == name)
                .map(|link| link.to_item(""));
        }
        if let Some(note_id) = id.strip_prefix("note:") {
            return notes::get(note_id).map(|n| n.to_item());
        }
        if let Some(hist_id) = id.strip_prefix("voice:hist:") {
            return crate::voice::history_item(hist_id);
        }
        if id == "cmd:focus-now" {
            return crate::focus::live_item();
        }
        None
    }

    fn empty_state(&self) -> Vec<Scored> {
        let mut out = Vec::new();
        if let Some(item) = crate::focus::live_item() {
            out.push(Scored::new(item, 40_000));
        }
        let now = usage::now_secs();
        let favs = favorites::Store::load();
        for id in favs.all() {
            if let Some(item) = self.lookup_item(id) {
                out.push(Scored::new(item, 32_000));
            }
        }

        for item in &self.extensions {
            out.push(Scored::new(item.clone(), 20_000));
        }
        for item in self.installed.borrow().iter() {
            out.push(Scored::new(item.clone(), 19_000));
        }

        let mut apps: Vec<&Item> = self.apps.iter().collect();
        apps.sort_by(|a, b| {
            usage::score(self.usage.get(&a.id), now)
                .cmp(&usage::score(self.usage.get(&b.id), now))
                .reverse()
                .then_with(|| a.title.cmp(&b.title))
        });
        for item in apps.into_iter().take(5) {
            out.push(Scored::new(
                item.clone(),
                10_000 + usage::score(self.usage.get(&item.id), now),
            ));
        }

        for item in &self.commands {
            out.push(Scored::new(
                item.clone(),
                1_000 + usage::score(self.usage.get(&item.id), now),
            ));
        }

        out.sort_by_key(|item| std::cmp::Reverse(item.score));
        let keep = favs.all().len().max(16);
        out.truncate(keep);
        out
    }
}

fn asked_for_files(rest: &str, mode: Mode, include_in_root: bool) -> bool {
    match mode {
        Mode::Files => true,
        Mode::Root if include_in_root => {
            let query = files::parse_query(rest);
            query.explicit || query.path_like || !query.extensions.is_empty()
        }
        Mode::Root
        | Mode::Windows
        | Mode::Clipboard
        | Mode::Snippets
        | Mode::Notes
        | Mode::Ask
        | Mode::Voice
        | Mode::Settings
        | Mode::Store
        | Mode::Quicklink
        | Mode::Calc
        | Mode::Emoji
        | Mode::Content
        | Mode::Extension => false,
    }
}

pub fn live_needed(query: &str, mode: Mode, include_in_root: bool) -> bool {
    let (_, rest) = Mode::parse(query);
    if crate::content::term_from_query(query).is_some() {
        return true;
    }
    if mode == Mode::Root
        && intent::resolve(&rest)
            .intents
            .iter()
            .any(|hit| hit.kind == IntentKind::Weather)
        && weather::cached().is_none()
    {
        return true;
    }
    asked_for_files(&rest, mode, include_in_root)
}

pub fn live_extras(
    query: &str,
    mode: Mode,
    settings: &crate::config::Settings,
    usage: &usage::Map,
) -> LiveExtras {
    let (parsed, rest) = Mode::parse(query);
    let mode = if mode == Mode::Root { parsed } else { mode };
    let q = rest;
    let mut extras = LiveExtras::default();

    if intent::resolve(&q)
        .intents
        .iter()
        .any(|hit| hit.kind == IntentKind::Weather)
        && let Ok(snap) = weather::fetch()
    {
        let item = weather::item(Some(&snap));
        extras.weather = Some(Scored::with_live(
            item,
            88_000,
            Live::Weather {
                summary: snap.summary,
                location: snap.location,
                extra: snap.extra,
            },
        ));
    }

    if let Some(term) = crate::content::term_from_query(query) {
        let items = crate::content::search(&term, &settings.files.search_roots, 40);
        for item in items {
            let snippet = item.subtitle.clone();
            extras.files.push(Scored::with_live(
                item,
                50_000,
                Live::Snippet { text: snippet },
            ));
        }
        return extras;
    }

    let want_files = asked_for_files(&q, mode, settings.files.include_in_root);
    if !want_files {
        return extras;
    }

    let limit = if mode == Mode::Files {
        settings.files.max_results.max(files::FILES_MODE_LIMIT)
    } else {
        settings.files.max_results.min(files::ROOT_FILE_LIMIT)
    };
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(
        if q.is_empty() { "file" } else { &q },
        CaseMatching::Smart,
        Normalization::Smart,
    );
    let items = if q.is_empty() {
        files::recent(
            40,
            settings.files.include_hidden,
            &settings.files.search_roots,
        )
    } else {
        files::search(
            &q,
            limit,
            settings.files.include_hidden,
            &settings.files.search_roots,
            settings.files.system_wide,
        )
    };
    let now = usage::now_secs();
    for item in items {
        let mut live = Live::from_item(&item);
        if let Action::OpenPath(path) | Action::PlayMedia { path } = &item.action {
            live.fill_snippet(path);
        }
        let score = rank_file(&mut matcher, &pattern, &q, &item, usage, now);
        extras.files.push(Scored::with_live(item, score, live));
    }
    extras
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
            subtitle: "In-bar, or dictate to the focused app with wtype".into(),
            keywords: "voice dictate speech microphone history wtype".into(),
            kind: Kind::Extension,
            icon: Icon::Name("audio-input-microphone".into()),
            action: Action::EnterMode(Mode::Voice),
        },
        Item {
            id: "ext:files".into(),
            title: "Search Files".into(),
            subtitle: "Every markdown, PDF, or named file — scroll the full list".into(),
            keywords: "file files find fd locate markdown pdf documents".into(),
            kind: Kind::Extension,
            icon: Icon::Name("system-file-manager".into()),
            action: Action::EnterMode(Mode::Files),
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
        Item {
            id: "ext:links".into(),
            title: "Quicklinks".into(),
            subtitle: "URLs, folders, and {argument} shortcuts · type +name to save".into(),
            keywords: "link links quicklink bookmark".into(),
            kind: Kind::Extension,
            icon: Icon::Name("web-browser".into()),
            action: Action::EnterMode(Mode::Quicklink),
        },
        Item {
            id: "ext:calc".into(),
            title: "Calculator".into(),
            subtitle: "Math, dates, percents, and recent answers".into(),
            keywords: "calc calculator math percent date history".into(),
            kind: Kind::Extension,
            icon: Icon::Name("accessories-calculator".into()),
            action: Action::EnterMode(Mode::Calc),
        },
    ]
}

fn setting_toggle(id: &str, title: &str, on: bool, keywords: &str) -> Item {
    Item {
        id: format!("set:{id}"),
        title: title.into(),
        subtitle: if on {
            "On · Enter to disable"
        } else {
            "Off · Enter to enable"
        }
        .into(),
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
    usage_map: &usage::Map,
    limit: usize,
    matcher: &mut Matcher,
    haystacks: &HashMap<String, String>,
) -> Vec<Scored> {
    let query = query.trim();
    let aliases = alias::Store::load();
    let favs = favorites::Store::load();
    if query.is_empty() {
        return items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let boost = if favs.is_pinned(&item.id) { 8_000 } else { 0 };
                Scored::new(
                    item.clone(),
                    10_000u32
                        .saturating_sub((i as u32) * 10)
                        .saturating_add(boost),
                )
            })
            .take(limit)
            .collect();
    }
    let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
    let meaning = intent::resolve_with(query, &titles);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let now = usage::now_secs();
    let mut ranked: Vec<(u32, &Item)> = Vec::new();
    for item in items {
        let owned = hay_with_alias(
            item,
            haystacks.get(&item.id).map(String::as_str),
            aliases.get(&item.id),
        );
        let input = RankInput {
            query,
            item,
            haystack: &owned,
            usage: usage_map,
            now,
            file_heavy: false,
            alias: aliases.get(&item.id),
            favorite: favs.is_pinned(&item.id),
        };
        if let Some(score) = rank(matcher, &pattern, input) {
            ranked.push((score, item));
        } else if let Some(score) = rank_expansions(matcher, &meaning.expansions, input) {
            ranked.push((score, item));
        }
    }
    take_scored(ranked, limit)
}

fn take_scored(mut ranked: Vec<(u32, &Item)>, limit: usize) -> Vec<Scored> {
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
    ranked.dedup_by(|a, b| a.1.id == b.1.id);
    ranked.truncate(limit);
    ranked
        .into_iter()
        .map(|(score, item)| Scored::new(item.clone(), score))
        .collect()
}

fn finish(results: Vec<Scored>) -> Vec<Scored> {
    finish_limited(results, 48)
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

#[derive(Clone, Copy)]
struct RankInput<'a> {
    query: &'a str,
    item: &'a Item,
    haystack: &'a str,
    usage: &'a usage::Map,
    now: u64,
    file_heavy: bool,
    alias: Option<&'a str>,
    favorite: bool,
}

fn rank_expansions(
    matcher: &mut Matcher,
    expansions: &[String],
    input: RankInput<'_>,
) -> Option<u32> {
    for expansion in expansions {
        let expanded = Pattern::parse(expansion, CaseMatching::Smart, Normalization::Smart);
        let mut next = input;
        next.query = expansion;
        if let Some(score) = rank(matcher, &expanded, next) {
            return Some(score.saturating_sub(600));
        }
    }
    None
}

fn rank(matcher: &mut Matcher, pattern: &Pattern, input: RankInput<'_>) -> Option<u32> {
    let mut buf = Vec::new();
    let title = input.item.title.to_lowercase();
    let title_score = pattern.score(Utf32Str::new(&input.item.title, &mut buf), matcher);
    buf.clear();
    let hay_score = pattern.score(Utf32Str::new(input.haystack, &mut buf), matcher);
    let typo = typo_score(input.query, input.item);
    let q_lower = input.query.to_ascii_lowercase();
    let alias_hit = input.alias.is_some_and(|alias| {
        let a = alias.to_ascii_lowercase();
        !q_lower.is_empty() && (a == q_lower || a.starts_with(&q_lower))
    });
    let mut score = match hay_score {
        Some(value) => value,
        None if alias_hit => 1,
        None => typo?,
    };
    if hay_score.is_some()
        && let Some(typo) = typo
    {
        score = score.max(typo);
    }
    if let Some(ts) = title_score {
        score = score.saturating_add(ts.saturating_mul(2));
    }
    let q = input.query.to_lowercase();
    if title.starts_with(&q) {
        score = score.saturating_add(usage::PREFIX_BONUS);
    } else if title.split_whitespace().any(|word| word.starts_with(&q)) {
        score = score.saturating_add(usage::PREFIX_BONUS / 2);
    }
    score = score.saturating_add(usage::score(input.usage.get(&input.item.id), input.now));
    score = score.saturating_add(match input.item.kind {
        Kind::Weather => 220,
        Kind::Extension => 120,
        Kind::Ai | Kind::Note => 90,
        Kind::Media => 90,
        Kind::App => 80,
        Kind::Window => 70,
        Kind::Snippet => 70,
        Kind::Command | Kind::Settings | Kind::Store => 60,
        Kind::Clipboard | Kind::Voice | Kind::Script => 50,
        Kind::File if input.file_heavy => 2_400,
        Kind::File => 30,
        Kind::Calc | Kind::Web | Kind::Shell => 10,
    });
    if let Some(alias) = input.alias {
        let a = alias.to_ascii_lowercase();
        if !q_lower.is_empty() && a == q_lower {
            score = score.saturating_add(50_000);
        } else if !q_lower.is_empty() && a.starts_with(&q_lower) {
            score = score.saturating_add(40_000);
        }
    }
    if input.favorite {
        score = score.saturating_add(8_000);
    }
    Some(score)
}

fn typo_score(query: &str, item: &Item) -> Option<u32> {
    if let Some(score) = intent::title_typo_score(query, &item.title) {
        return Some(score);
    }
    let q = query.trim().to_ascii_lowercase();
    let title = item.title.to_ascii_lowercase();
    if intent::is_adjacent_swap(&q, &title)
        || title
            .split_whitespace()
            .next()
            .is_some_and(|word| intent::is_adjacent_swap(&q, word))
    {
        return Some(2_400);
    }
    item.keywords
        .split_whitespace()
        .find_map(|word| intent::title_typo_score(query, word))
}

fn rank_file(
    matcher: &mut Matcher,
    pattern: &Pattern,
    query: &str,
    item: &Item,
    usage_map: &usage::Map,
    now: u64,
) -> u32 {
    let path = std::path::Path::new(item.id.trim_start_matches("file:"));
    let hay = item.haystack();
    let nucleo = rank(
        matcher,
        pattern,
        RankInput {
            query,
            item,
            haystack: &hay,
            usage: usage_map,
            now,
            file_heavy: true,
            alias: None,
            favorite: false,
        },
    )
    .unwrap_or(800);
    let mut score = nucleo;
    score = score.saturating_add(4_000);
    let title = item.title.to_ascii_lowercase();
    let q = query.to_ascii_lowercase();
    if title == q || title.starts_with(&q) {
        score = score.saturating_add(6_000);
    }
    if let Some(ext) = path.extension().and_then(|s| s.to_str())
        && q.contains(ext)
    {
        score = score.saturating_add(1_500);
    }
    score = score.saturating_add(files::recency_bonus(path));
    let depth = path.components().count() as u32;
    score = score.saturating_add(800u32.saturating_sub(depth.saturating_mul(20)));
    score
}

fn hay_with_alias(item: &Item, cached: Option<&str>, alias: Option<&str>) -> String {
    let mut hay = cached
        .map(str::to_string)
        .unwrap_or_else(|| item.haystack());
    if let Some(alias) = alias.filter(|a| !a.is_empty()) {
        hay.push(' ');
        hay.push_str(alias);
    }
    hay
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

    items.extend(crate::layout::builtin_items());
    items.extend(crate::capture::items(which));
    items.extend(crate::quit::items());
    items.extend(crate::hypr::resolution_items());
    items.extend(crate::ocr::items(which));
    items.extend(crate::voice::command_items());
    items.push(notes::from_selection_item());
    items.extend(crate::focus::items());
    items.extend(crate::ai::command_items(which));
    items.extend(crate::memory::command_items());
    if let Some(picker) = smart::picker_item(which) {
        items.push(picker);
    }
    items.retain(|item| match &item.action {
        Action::Spawn { program, .. } => program.contains('/') || which(program),
        _ => true,
    });
    let mut confetti = cmd(
        "confetti",
        "Throw confetti",
        "Celebrate a small win",
        "face-smile",
        Action::Confetti,
    );
    confetti.keywords = "party celebrate confetti".into();
    items.push(confetti);
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
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::looks_like_uri;
    use crate::mode::Mode;

    #[test]
    fn math_detection() {
        assert!(crate::calc::looks_like_math("2+2"));
        assert!(crate::calc::looks_like_math("sqrt(9)"));
        assert!(!crate::calc::looks_like_math("firefox"));
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
        assert_eq!(Mode::parse("file markdown").0, Mode::Files);
    }

    #[test]
    fn we_surfaces_live_weather() {
        use crate::clipboard::Store;
        use crate::config::Settings;
        use std::cell::RefCell;
        use std::rc::Rc;

        let catalog = super::Catalog::load(
            Rc::new(RefCell::new(Store::load())),
            Rc::new(RefCell::new(Settings::default())),
        );
        let (mode, results) = catalog.search_fast("we");
        assert_eq!(mode, crate::mode::Mode::Root);
        assert!(
            results.iter().any(|row| row.item.id == "live:weather"),
            "typing we should surface weather"
        );
        let weahter = catalog.search_fast("weahter").1;
        assert!(
            weahter.iter().any(|row| row.item.id == "live:weather"),
            "swapped letters should still surface weather"
        );
        assert!(
            !super::live_needed("firefox", crate::mode::Mode::Root, true),
            "an app name must not schedule a worker"
        );
        assert!(
            !super::live_needed("smile", crate::mode::Mode::Root, true),
            "emoji keywords stay in-memory"
        );
        assert!(
            !super::live_needed("we", crate::mode::Mode::Windows, true),
            "window mode is in-memory now — no live pass"
        );
    }

    #[test]
    fn live_needed_is_files_or_uncached_weather() {
        assert!(super::live_needed(
            "markdown",
            crate::mode::Mode::Root,
            true
        ));
        assert!(super::live_needed("", crate::mode::Mode::Files, false));
        assert!(!super::live_needed(
            "clip rust",
            crate::mode::Mode::Clipboard,
            true
        ));
        if crate::weather::cached().is_none() {
            assert!(super::live_needed("we", crate::mode::Mode::Root, false));
        }
        assert!(super::live_needed(
            "content:needle",
            crate::mode::Mode::Root,
            false
        ));
        assert!(super::live_needed(
            "in:secret",
            crate::mode::Mode::Root,
            false
        ));
        assert!(super::live_needed(
            "file content invoices",
            crate::mode::Mode::Files,
            false
        ));
        assert!(!super::live_needed(
            "contentment",
            crate::mode::Mode::Root,
            false
        ));
    }

    #[test]
    fn wave4_commands_are_searchable() {
        use crate::clipboard::Store;
        use crate::config::Settings;
        use std::cell::RefCell;
        use std::rc::Rc;

        crate::focus::reset();
        let catalog = super::Catalog::load(
            Rc::new(RefCell::new(Store::load())),
            Rc::new(RefCell::new(Settings::default())),
        );
        let dictate = catalog.search_fast("dictate focused").1;
        assert!(
            dictate.iter().any(|row| row.item.id == "cmd:dictate-app"),
            "Dictate to focused app must be a root command"
        );
        assert!(
            dictate
                .iter()
                .any(|row| matches!(row.item.action, crate::item::Action::DictateFocused))
        );
        let note = catalog.search_fast("note from selection").1;
        assert!(note.iter().any(|row| row.item.id == "cmd:note-selection"));
        let focus = catalog.search_fast("start focus").1;
        assert!(focus.iter().any(|row| row.item.id == "cmd:focus-start"));
        assert!(
            catalog
                .search_fast("unfocus")
                .1
                .iter()
                .any(|row| row.item.id == "cmd:unfocus")
        );
        assert_eq!(catalog.search_fast("voice").0, crate::mode::Mode::Voice);
        let voice = catalog.search_fast("voice").1;
        assert!(voice.iter().any(|row| row.item.id == "voice:toggle"));
        assert!(voice.iter().any(|row| row.item.id == "cmd:dictate-app"));
        assert!(crate::focus::live_item().is_none());
    }

    #[test]
    fn wave5_ask_threads_memory_and_selection_prompts() {
        use crate::clipboard::Store;
        use crate::config::Settings;
        use std::cell::RefCell;
        use std::rc::Rc;

        crate::db::with_temp(|dir| {
            crate::db::open_path(&dir.join("flint.db")).expect("open");
            crate::ai::reset();
            let thread = crate::ai::ensure_thread("Weather in Tokyo");
            crate::ai::append_turn(&thread.id, "user", "Weather in Tokyo");
            crate::ai::append_turn(&thread.id, "assistant", "Sunny.");
            crate::memory::remember("I use Hyprland").expect("mem");
            let catalog = super::Catalog::load(
                Rc::new(RefCell::new(Store::load())),
                Rc::new(RefCell::new(Settings::default())),
            );
            let empty = catalog.search_fast("?").1;
            assert!(empty.iter().any(|row| row.item.id == "ask:new"));
            assert!(
                empty
                    .iter()
                    .any(|row| row.item.title.contains("Weather in Tokyo")),
                "empty Ask query must list recent threads"
            );
            let follow = catalog.search_fast("? and tomorrow").1;
            assert!(follow.iter().any(|row| {
                matches!(row.item.action, crate::item::Action::AskAi { .. })
                    && row.item.subtitle.contains("Follow-up")
            }));
            let search = catalog.search_fast("? Sunny").1;
            assert!(search.iter().any(|row| row.item.title.contains("Weather")));
            let grammar = catalog.search_fast("fix grammar").1;
            assert!(
                grammar
                    .iter()
                    .any(|row| matches!(row.item.action, crate::item::Action::AskSelection { .. }))
            );
            let rem = catalog.search_fast("remember I ship from Omarchy").1;
            assert!(rem.iter().any(|row| matches!(
                &row.item.action,
                crate::item::Action::Remember { text } if text == "I ship from Omarchy"
            )));
            let mem = catalog.search_fast("show memory").1;
            assert!(
                mem.iter()
                    .any(|row| matches!(row.item.action, crate::item::Action::ShowMemory))
            );
        });
    }

    #[test]
    fn voice_mode_empty_query_lists_history() {
        use crate::clipboard::Store;
        use crate::config::Settings;
        use std::cell::RefCell;
        use std::rc::Rc;

        crate::db::with_temp(|dir| {
            crate::db::open_path(&dir.join("flint.db")).expect("open");
            crate::voice::remember("remember this utterance");
            let catalog = super::Catalog::load(
                Rc::new(RefCell::new(Store::load())),
                Rc::new(RefCell::new(Settings::default())),
            );
            let rows = catalog.search_fast("voice").1;
            assert!(
                rows.iter()
                    .any(|row| row.item.keywords.contains("remember this utterance")),
                "empty Voice query must list dictation history"
            );
            assert!(rows.iter().any(|row| matches!(
                &row.item.action,
                crate::item::Action::Paste(text) if text == "remember this utterance"
            )));
            let styles = catalog.search_fast("voice formal").1;
            assert!(
                styles
                    .iter()
                    .any(|row| matches!(row.item.action, crate::item::Action::AskAi { .. }))
            );
        });
    }

    #[test]
    fn root_instant_answers_for_wave3() {
        use crate::clipboard::Store;
        use crate::config::Settings;
        use std::cell::RefCell;
        use std::rc::Rc;

        let catalog = super::Catalog::load(
            Rc::new(RefCell::new(Store::load())),
            Rc::new(RefCell::new(Settings::default())),
        );
        let smile = catalog.search_fast("smile").1;
        assert!(
            smile.iter().any(|row| row.item.id.starts_with("emoji:")),
            "smile should surface built-in emoji"
        );
        assert!(
            smile
                .iter()
                .find(|row| row.item.id.starts_with("emoji:"))
                .map(|row| row.score)
                .unwrap_or(0)
                > smile
                    .iter()
                    .find(|row| row.item.id == "cmd:emoji")
                    .map(|row| row.score)
                    .unwrap_or(0),
            "built-in emoji must rank above omarchy-menu-emoji"
        );
        assert_eq!(
            catalog.search_fast("emoji smile").0,
            crate::mode::Mode::Emoji
        );
        let tokyo = catalog.search_fast("time in tokyo").1;
        assert!(tokyo.iter().any(|row| row.item.id.starts_with("tz:")));
        let tr = catalog.search_fast("tr fr hello").1;
        assert!(
            tr.iter()
                .any(|row| matches!(row.item.action, crate::item::Action::AskAi { .. }))
        );
        let rgb = catalog.search_fast("rgb(255, 90, 31)").1;
        assert!(rgb.iter().any(|row| row.item.title == "#ff5a1f"));
    }

    #[test]
    fn recent_use_outranks_stale_prefix_habit() {
        use crate::item::{Action, Icon, Item, Kind};
        use crate::usage::{self, Record};

        let weather = Item {
            id: "app:weather.desktop".into(),
            title: "Weather".into(),
            subtitle: "Forecast".into(),
            keywords: String::new(),
            kind: Kind::App,
            icon: Icon::None,
            action: Action::Copy(String::new()),
        };
        let web = Item {
            id: "app:web.desktop".into(),
            title: "Web Search Helper".into(),
            subtitle: "Search".into(),
            keywords: String::new(),
            kind: Kind::App,
            icon: Icon::None,
            action: Action::Copy(String::new()),
        };
        let now = 1_800_000_000;
        let mut usage = usage::Map::new();
        usage.insert(
            weather.id.clone(),
            Record {
                count: 2,
                last: now - 3_600,
            },
        );
        usage.insert(
            web.id.clone(),
            Record {
                count: 200,
                last: now - 2 * 365 * 24 * 3600,
            },
        );
        let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
        let pattern = nucleo_matcher::pattern::Pattern::parse(
            "we",
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let hay_w = weather.haystack();
        let hay_web = web.haystack();
        let weather_score = super::rank(
            &mut matcher,
            &pattern,
            super::RankInput {
                query: "we",
                item: &weather,
                haystack: &hay_w,
                usage: &usage,
                now,
                file_heavy: false,
                alias: None,
                favorite: false,
            },
        )
        .unwrap();
        let web_score = super::rank(
            &mut matcher,
            &pattern,
            super::RankInput {
                query: "we",
                item: &web,
                haystack: &hay_web,
                usage: &usage,
                now,
                file_heavy: false,
                alias: None,
                favorite: false,
            },
        )
        .unwrap();
        assert!(
            weather_score > web_score,
            "yesterday's Weather ({weather_score}) should beat 200 ancient Web uses ({web_score})"
        );
    }

    #[test]
    fn swapped_letters_rank_an_app_title() {
        use crate::item::{Action, Icon, Item, Kind};

        let item = Item {
            id: "app:weather.desktop".into(),
            title: "Weather".into(),
            subtitle: String::new(),
            keywords: String::new(),
            kind: Kind::App,
            icon: Icon::None,
            action: Action::Copy(String::new()),
        };
        let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
        let pattern = nucleo_matcher::pattern::Pattern::parse(
            "weahter",
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let hay = item.haystack();
        let usage = crate::usage::Map::new();
        let score = super::rank(
            &mut matcher,
            &pattern,
            super::RankInput {
                query: "weahter",
                item: &item,
                haystack: &hay,
                usage: &usage,
                now: 1_800_000_000,
                file_heavy: false,
                alias: None,
                favorite: false,
            },
        );
        assert!(
            score.is_some(),
            "weahter must match Weather (transposition), not only missing letters"
        );
    }

    #[test]
    fn live_slot_is_more_than_title_and_action() {
        use crate::item::Live;
        let item = crate::weather::item(Some(&crate::weather::Snapshot {
            location: "Boardman".into(),
            extra: "12%".into(),
            summary: "+79°F ☀️ Sunny".into(),
        }));
        assert!(matches!(Live::from_item(&item), Live::Weather { .. }));
    }
}
