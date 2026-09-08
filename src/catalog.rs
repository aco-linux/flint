use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::alias;
use crate::auth;
use crate::calc;
use crate::choices;
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
use crate::quicklinks::{self, Link};
use crate::smart;
use crate::snippets;
use crate::store;
use crate::tz;
use crate::usage;
use crate::weather;

// Ranking ladder (higher wins). Nucleo is length-normalized and sits in the
// thousands; these steps are the order the user actually sees.
//
//   120_000  explicit alias
//   118_000  complete calc (`1+1`, `20% of 80`) — not a bare `1`
//   115_000  learned query→item choice (decayed)
//   110_000  live weather / first-frame weather placeholder / focus timer
//   100_000  layout-save
//    92_000  emoji (only when the query looks like emoji)
//    90_000  `>` / `$` shell
//    85_000  memory / translate / tz / instant answers
//    80_000  typed URI
//    70_000  intents (weather/time/calendar/email/gif) + hit.score
//    50_000  Ask AI · type-word file search (files sit here and above)
//    20_000  prefix-learned choice (`s` after `sl` → Slack)
//     8_000  apps/commands while a type-word file search is running
//       400  web fallback (last)
//
// Title-first fuzzy pool (applied inside `rank`, before usage/kind):
//   +10_000  exact title
//    +4_000  title starts with query
//    +2_500  title-word prefix or initials (`vsc`, `gc`)
//    +1_500  keyword exact/prefix
//    ≤2_400  typo / adjacent-swap (capped below prefix)
const TIER_ALIAS: u32 = 120_000;
const TIER_CALC: u32 = 100_000;
const TIER_WEATHER: u32 = 110_000;
const TIER_FOCUS: u32 = 110_000;
const TIER_LAYOUT_SAVE: u32 = 100_000;
const TIER_EMOJI: u32 = 92_000;
const TIER_SHELL: u32 = 90_000;
const TIER_INSTANT: u32 = 85_000;
const TIER_URI: u32 = 80_000;
const TIER_INTENT: u32 = 70_000;
const TIER_ASK: u32 = 50_000;
const TIER_FILE_TYPE: u32 = 50_000;
const TIER_APP_IN_TYPE_SEARCH: u32 = 8_000;
const TIER_WEB_INTENT: u32 = 12_000;
const TIER_WEB_ASK: u32 = 8_000;
const TIER_WEB_FALLBACK: u32 = 400;
const TIER_PATH: u32 = 12_000;
const TIER_GIF: u32 = 65_000;
const TIER_TIME: u32 = 60_000;

const TITLE_EXACT: u32 = 10_000;
const TITLE_PREFIX: u32 = 4_000;
const TITLE_WORD_OR_INITIALS: u32 = 2_500;
const KEYWORD_PREFIX: u32 = 1_500;
const TYPO_CAP: u32 = 2_400;

/// Precomputed lowercase fields so `rank` never calls `to_lowercase` per keystroke.
#[derive(Clone, Debug)]
struct IndexEntry {
    title_lc: String,
    words: Vec<String>,
    initials: String,
    keywords: String,
    keywords_lc: String,
    subtitle_lc: String,
    alias_lc: String,
}

impl IndexEntry {
    fn from_item(item: &Item, alias: Option<&str>) -> Self {
        let title_lc = item.title.to_lowercase();
        let words: Vec<String> = title_lc
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_string)
            .collect();
        let initials: String = words
            .iter()
            .filter_map(|word| word.chars().next())
            .collect();
        let alias_lc = alias
            .map(str::trim)
            .filter(|alias| !alias.is_empty())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let keywords = if alias_lc.is_empty() {
            item.keywords.clone()
        } else {
            format!("{} {alias_lc}", item.keywords)
        };
        let keywords_lc = keywords.to_lowercase();
        Self {
            title_lc,
            words,
            initials,
            keywords,
            keywords_lc,
            subtitle_lc: item.subtitle.to_lowercase(),
            alias_lc,
        }
    }
}

pub struct Catalog {
    apps: Vec<Item>,
    commands: Vec<Item>,
    extensions: Vec<Item>,
    installed: RefCell<Vec<Item>>,
    lexicon: Vec<String>,
    index: RefCell<HashMap<String, IndexEntry>>,
    aliases: RefCell<alias::Store>,
    favorites: RefCell<favorites::Store>,
    quicklinks: RefCell<Vec<Link>>,
    custom_layouts: RefCell<Vec<Item>>,
    windows: RefCell<Vec<Item>>,
    matcher: RefCell<Matcher>,
    nucleo_buf: RefCell<Vec<char>>,
    context: RefCell<crate::context::Context>,
    app_tags: RefCell<HashMap<String, String>>,
    pub(crate) usage: usage::Map,
    pub(crate) choices: choices::Map,
    pub(crate) choice_ctx: choices::ContextMap,
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
    pub gifs: Vec<Scored>,
    pub calendar: Vec<Scored>,
    pub mail: Vec<Scored>,
    pub web: Vec<Scored>,
}

impl Catalog {
    pub fn load(clips: Rc<RefCell<ClipStore>>, settings: Rc<RefCell<Settings>>) -> Self {
        let apps = desktop::load_apps();
        let commands = system_commands();
        let extensions = extension_items();
        let installed = crate::extension::command_items();
        let aliases = alias::Store::load();
        let favorites = favorites::Store::load();
        let quicklinks = quicklinks::load();
        let custom_layouts = crate::layout::custom_items();
        let windows = hypr::load_windows();
        let mut index = HashMap::new();
        let mut lexicon = files::type_words();
        for item in apps
            .iter()
            .chain(commands.iter())
            .chain(extensions.iter())
            .chain(installed.iter())
            .chain(windows.iter())
            .chain(custom_layouts.iter())
        {
            index.insert(
                item.id.clone(),
                IndexEntry::from_item(item, aliases.get(&item.id)),
            );
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
        for link in &quicklinks {
            let item = link.to_item("");
            index.insert(
                item.id.clone(),
                IndexEntry::from_item(&item, aliases.get(&item.id)),
            );
        }
        let app_tags = build_app_tags(&quicklinks);
        Self {
            apps,
            commands,
            extensions,
            installed: RefCell::new(installed),
            lexicon,
            index: RefCell::new(index),
            aliases: RefCell::new(aliases),
            favorites: RefCell::new(favorites),
            quicklinks: RefCell::new(quicklinks),
            custom_layouts: RefCell::new(custom_layouts),
            windows: RefCell::new(windows),
            matcher: RefCell::new(Matcher::new(Config::DEFAULT)),
            nucleo_buf: RefCell::new(Vec::new()),
            context: RefCell::new(crate::context::Context::default()),
            app_tags: RefCell::new(app_tags),
            usage: usage::load(),
            choices: choices::load(),
            choice_ctx: choices::load_context(),
            clips,
            settings,
        }
    }

    /// Re-scan installed extension commands after a store install.
    pub fn reload_installed(&self) {
        *self.aliases.borrow_mut() = alias::Store::load();
        *self.favorites.borrow_mut() = favorites::Store::load();
        self.reload_quicklinks();
        self.reload_layouts();
        let installed = crate::extension::command_items();
        {
            let aliases = self.aliases.borrow();
            let mut index = self.index.borrow_mut();
            index.retain(|id, _| !id.starts_with("vx:"));
            for item in &installed {
                index.insert(
                    item.id.clone(),
                    IndexEntry::from_item(item, aliases.get(&item.id)),
                );
            }
        }
        *self.installed.borrow_mut() = installed;
    }

    pub fn adopt_windows(&self, windows: Vec<Item>) {
        {
            let aliases = self.aliases.borrow();
            let mut index = self.index.borrow_mut();
            index.retain(|id, _| !id.starts_with("win:"));
            for item in &windows {
                index.insert(
                    item.id.clone(),
                    IndexEntry::from_item(item, aliases.get(&item.id)),
                );
            }
        }
        *self.windows.borrow_mut() = windows;
    }

    pub fn set_alias(&self, id: &str, name: &str) {
        {
            let mut store = self.aliases.borrow_mut();
            store.set(id, name);
            store.persist();
        }
        self.reindex_id(id);
    }

    pub fn toggle_favorite(&self, id: &str) -> bool {
        let mut store = self.favorites.borrow_mut();
        let pinned = store.toggle(id);
        store.persist();
        pinned
    }

    pub fn is_favorite(&self, id: &str) -> bool {
        self.favorites.borrow().is_pinned(id)
    }

    pub fn reload_quicklinks(&self) {
        let links = quicklinks::load();
        {
            let aliases = self.aliases.borrow();
            let mut index = self.index.borrow_mut();
            index.retain(|id, _| !id.starts_with("link:"));
            for link in &links {
                let item = link.to_item("");
                index.insert(
                    item.id.clone(),
                    IndexEntry::from_item(&item, aliases.get(&item.id)),
                );
            }
        }
        *self.quicklinks.borrow_mut() = links;
        self.refresh_app_tags();
    }

    pub fn reload_layouts(&self) {
        let next = crate::layout::custom_items();
        let old_ids: Vec<String> = self
            .custom_layouts
            .borrow()
            .iter()
            .map(|item| item.id.clone())
            .collect();
        {
            let aliases = self.aliases.borrow();
            let mut index = self.index.borrow_mut();
            for id in &old_ids {
                index.remove(id);
            }
            for item in &next {
                index.insert(
                    item.id.clone(),
                    IndexEntry::from_item(item, aliases.get(&item.id)),
                );
            }
        }
        *self.custom_layouts.borrow_mut() = next;
        self.refresh_app_tags();
    }

    pub fn set_context(&self, ctx: crate::context::Context) {
        *self.context.borrow_mut() = ctx;
    }

    fn context_aware(&self) -> bool {
        self.settings.borrow().general.context_aware
    }

    fn context_class(&self) -> String {
        if self.context_aware() {
            self.context.borrow().class.clone()
        } else {
            String::new()
        }
    }

    fn refresh_app_tags(&self) {
        *self.app_tags.borrow_mut() = build_app_tags(&self.quicklinks.borrow());
    }

    fn reindex_id(&self, id: &str) {
        let Some(item) = self.lookup_item(id) else {
            return;
        };
        let alias = self.aliases.borrow().get(id).map(str::to_string);
        self.index.borrow_mut().insert(
            item.id.clone(),
            IndexEntry::from_item(&item, alias.as_deref()),
        );
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
            Mode::Gif => self.search_gif(&rest),
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
        let mix_limit = settings.general.max_results;
        drop(settings);

        if query.is_empty() {
            return self.empty_state();
        }

        let file_query = files::parse_query(query);
        let type_search = file_query.is_type_search();
        let file_heavy = type_search || file_query.path_like || file_query.explicit;

        let lexicon: Vec<&str> = self.lexicon.iter().map(String::as_str).collect();
        let meaning = intent::resolve_with(query, &lexicon);
        let complete_calc = calc::root_item(query);

        if let Some(item) = crate::focus::live_item() {
            let hay = format!("{} {}", item.title, item.keywords).to_ascii_lowercase();
            if hay.contains(&query.to_ascii_lowercase()) {
                results.push(Scored::new(item, TIER_FOCUS));
            }
        }

        if let Some(id) = self.aliases.borrow().lookup(query).map(str::to_string)
            && let Some(item) = self.lookup_item(&id)
        {
            results.push(Scored::new(item, TIER_ALIAS));
        }

        if let Some(item) = complete_calc.clone() {
            results.push(Scored::new(item, TIER_CALC));
        }
        for item in smart::instant_items(query) {
            results.push(Scored::new(item, TIER_INSTANT));
        }
        if let Some(item) = crate::translate::item(query) {
            results.push(Scored::new(item, TIER_INSTANT));
        }
        for (i, item) in crate::memory::items(query).into_iter().enumerate() {
            results.push(Scored::new(item, TIER_INSTANT.saturating_sub(i as u32)));
        }
        for item in tz::items(query) {
            results.push(Scored::new(item, TIER_INSTANT));
        }

        let suppress_emoji = type_search
            || meaning.has(IntentKind::Weather)
            || meaning.has(IntentKind::Time)
            || meaning.has(IntentKind::Calendar)
            || meaning.has(IntentKind::Email);
        if !suppress_emoji && crate::emoji::looks_like_query(query) {
            for (i, item) in crate::emoji::search(query, 8).into_iter().enumerate() {
                results.push(Scored::new(item, TIER_EMOJI.saturating_sub(i as u32 * 10)));
            }
        }

        for hit in &meaning.intents {
            match hit.kind {
                IntentKind::Weather => results.push(Scored::new(
                    weather::item(weather::cached().as_ref()),
                    TIER_WEATHER,
                )),
                IntentKind::Time => {
                    results.push(Scored::new(weather::time_item(), TIER_TIME + hit.score))
                }
                IntentKind::Calendar => {
                    results.push(Scored::new(calendar_stub(), TIER_INTENT + hit.score));
                }
                IntentKind::Email => {
                    results.push(Scored::new(
                        crate::mail::stub(&self.settings.borrow()),
                        TIER_INTENT + hit.score,
                    ));
                }
                IntentKind::Gif => {
                    let terms = intent::gif_terms(query);
                    results.push(Scored::new(
                        crate::gif::search_item(if terms.is_empty() { query } else { &terms }),
                        TIER_GIF + hit.score,
                    ));
                }
                IntentKind::Web => {
                    results.push(Scored::new(
                        crate::web::search_item(query),
                        TIER_WEB_INTENT + hit.score,
                    ));
                }
                IntentKind::Ask => {
                    if !query.is_empty() {
                        results.push(Scored::new(
                            ask_prompt_item(query, &self.settings.borrow()),
                            TIER_ASK,
                        ));
                    }
                }
            }
        }

        if query.starts_with('>') {
            let cmd = query.trim_start_matches('>').trim();
            if !cmd.is_empty() {
                results.push(Scored::new(run_item(cmd.to_string(), false), TIER_SHELL));
            }
        } else if query.starts_with('$') {
            let cmd = query.trim_start_matches('$').trim();
            if !cmd.is_empty() {
                results.push(Scored::new(run_item(cmd.to_string(), true), TIER_SHELL));
            }
        }

        if let Some(name) = crate::layout::parse_save_query(query) {
            results.push(Scored::new(
                crate::layout::save_item(&name),
                TIER_LAYOUT_SAVE,
            ));
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
                TIER_URI,
            ));
        }

        if let Some(item) = smart::path_command(query) {
            results.push(Scored::new(item, TIER_PATH));
        }

        if type_search {
            let label = file_query.type_label.as_deref().unwrap_or("Files");
            results.push(Scored::new(files_searching_item(label), TIER_FILE_TYPE));
        }

        let now = usage::now_secs();
        let windows = self.windows.borrow();
        let installed = self.installed.borrow();
        let mut matcher = self.matcher.borrow_mut();
        let mut nucleo_buf = self.nucleo_buf.borrow_mut();
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let aliases = self.aliases.borrow();
        let favs = self.favorites.borrow();
        let qlinks: Vec<Item> = self
            .quicklinks
            .borrow()
            .iter()
            .map(|link| link.to_item(&link.argument_for(query)))
            .collect();
        let custom_layouts = self.custom_layouts.borrow();
        let index = self.index.borrow();
        let app_tags = self.app_tags.borrow();
        let ctx_class = self.context_class();
        let q_lc = query.to_lowercase();

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
            let owned;
            let entry = match index.get(&item.id) {
                Some(entry) => entry,
                None => {
                    owned = IndexEntry::from_item(item, aliases.get(&item.id));
                    &owned
                }
            };
            let app_bonus = crate::context::app_bonus(
                app_tags.get(&item.id).map(String::as_str).unwrap_or(""),
                &ctx_class,
            );
            let input = RankInput {
                query,
                q_lc: &q_lc,
                item,
                entry,
                usage: &self.usage,
                now,
                file_heavy,
                favorite: favs.is_pinned(&item.id),
            };
            if let Some(score) = rank(&mut matcher, &pattern, &mut nucleo_buf, input) {
                ranked.push((score.saturating_add(app_bonus), item));
            } else if let Some(score) =
                rank_expansions(&mut matcher, &mut nucleo_buf, &meaning.expansions, input)
            {
                ranked.push((score.saturating_add(app_bonus), item));
            }
        }

        results.extend(take_scored(ranked, mix_limit.max(file_limit), &self.usage));

        if include_in_root && file_query.wants_files() {
            for item in files::well_known_folders(query) {
                let owned = IndexEntry::from_item(&item, aliases.get(&item.id));
                let score = rank(
                    &mut matcher,
                    &pattern,
                    &mut nucleo_buf,
                    RankInput {
                        query,
                        q_lc: &q_lc,
                        item: &item,
                        entry: &owned,
                        usage: &self.usage,
                        now,
                        file_heavy,
                        favorite: favs.is_pinned(&item.id),
                    },
                )
                .unwrap_or(8_000);
                results.push(Scored::new(item, score));
            }
        }

        drop(index);
        drop(custom_layouts);
        drop(favs);
        drop(aliases);
        drop(nucleo_buf);
        drop(matcher);
        drop(windows);

        if type_search {
            for row in &mut results {
                if matches!(
                    row.item.kind,
                    Kind::App | Kind::Command | Kind::Extension | Kind::Window | Kind::Script
                ) {
                    row.score = row.score.min(TIER_APP_IN_TYPE_SEARCH);
                }
            }
        }

        if complete_calc.is_none() {
            self.apply_learned_choice(query, now, &mut results);
        }

        if !query.starts_with(['>', '$', '=', '/', '~', ';'])
            && !meaning.tool_intent()
            && !meaning.has(IntentKind::Web)
        {
            let encoded_score = if meaning.has(IntentKind::Ask) {
                TIER_WEB_ASK
            } else {
                TIER_WEB_FALLBACK
            };
            results.push(Scored::new(crate::web::search_item(query), encoded_score));
        }

        let limit = if file_heavy {
            file_limit.max(mix_limit)
        } else {
            mix_limit
        };
        let out = finish_limited(results, limit, &self.usage);
        log_rank_debug(query, &out);
        out
    }

    pub(crate) fn touch_usage(&mut self, id: &str) {
        if id.is_empty() {
            return;
        }
        let now = usage::now_secs();
        let rec = self
            .usage
            .entry(id.to_string())
            .or_insert(usage::Record { count: 0, last: 0 });
        rec.count = rec.count.saturating_add(1);
        rec.last = now;
        usage::bump(id);
    }

    pub(crate) fn learn_choice(&mut self, query: &str, id: &str) {
        choices::bump(&mut self.choices, query, id);
        let class = self.context_class();
        if !class.is_empty() {
            choices::bump_class(&mut self.choice_ctx, &class, query, id);
        }
    }

    pub(crate) fn clear_choices(&mut self) {
        choices::clear(&mut self.choices);
        choices::clear_context(&mut self.choice_ctx);
    }

    fn apply_learned_choice(&self, query: &str, now: u64, results: &mut Vec<Scored>) {
        let class = self.context_class();
        let hit = choices::best_class(&self.choice_ctx, &class, query)
            .or_else(|| choices::best(&self.choices, query));
        let Some((id, rec)) = hit else {
            return;
        };
        let id = id.to_string();
        let bonus = choices::bonus(&rec, now);
        if bonus == 0 {
            return;
        }
        if let Some(row) = results.iter_mut().find(|row| row.item.id == id) {
            row.score = row.score.max(bonus);
            return;
        }
        if let Some(item) = self.lookup_item(&id) {
            results.push(Scored::new(item, bonus));
        }
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
            return finish_limited(results, limit, &self.usage);
        }

        for item in files::well_known_folders(q) {
            results.push(Scored::new(item, 30_000));
        }
        finish_limited(results, limit, &self.usage)
    }

    fn search_windows(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let mut results = Vec::new();
        if let Some(name) =
            crate::layout::parse_save_short(q).or_else(|| crate::layout::parse_save_query(q))
        {
            results.push(Scored::new(crate::layout::save_item(&name), 100_000));
        }
        let mut layouts = crate::layout::builtin_items();
        layouts.extend(self.custom_layouts.borrow().iter().cloned());
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
        finish(results, &self.usage)
    }

    fn score_pool(&self, items: &[Item], query: &str, limit: usize) -> Vec<Scored> {
        let mut rows = score_pool(
            items,
            query,
            &self.usage,
            limit,
            &mut self.matcher.borrow_mut(),
            &mut self.nucleo_buf.borrow_mut(),
            &self.index.borrow(),
            &self.aliases.borrow(),
            &self.favorites.borrow(),
        );
        if self.context_aware() {
            let class = self.context_class();
            let tags = self.app_tags.borrow();
            for row in &mut rows {
                row.score = row.score.saturating_add(crate::context::app_bonus(
                    tags.get(&row.item.id).map(String::as_str).unwrap_or(""),
                    &class,
                ));
            }
        }
        rows
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
        finish(results, &self.usage)
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
        finish(results, &self.usage)
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
        finish(results, &self.usage)
    }

    fn search_settings(&self, query: &str) -> Vec<Scored> {
        let q = query.trim();
        let s = self.settings.borrow();
        let mut items = vec![
            Item {
                id: "set:window".into(),
                title: "Open Settings window".into(),
                subtitle: "Connections, plugins, skills, Ask AI, files".into(),
                keywords: "prefs window settings".into(),
                kind: Kind::Settings,
                icon: Icon::Name("preferences-system".into()),
                action: Action::OpenPrefs { page: None },
            },
            Item {
                id: "set:clear-choices".into(),
                title: "Clear learned choices".into(),
                subtitle: "Forget query→app ranking memory".into(),
                keywords: "forget ranking memory choices query".into(),
                kind: Kind::Settings,
                icon: Icon::Name("edit-clear".into()),
                action: Action::ClearChoices,
            },
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
                "context",
                "Context-aware search",
                s.general.context_aware,
                "hyprland focused window class clipboard",
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
                "ollama openai anthropic google xai custom",
            ),
            setting_value("model", "AI model", &s.ai.model, q, "model"),
            setting_value("endpoint", "AI endpoint", &s.ai.endpoint, q, "url host"),
            Item {
                id: "set:signin-xai".into(),
                title: "Connect Grok (xAI) with your subscription".into(),
                subtitle: auth::signed_in_label(),
                keywords: "oauth grok xai x.ai subscription super grok".into(),
                kind: Kind::Ai,
                icon: Icon::Name("network-workgroup".into()),
                action: Action::SignIn {
                    provider: "xai".into(),
                },
            },
            Item {
                id: "set:import-grok".into(),
                title: "Use existing Grok CLI login".into(),
                subtitle: "Imports ~/.grok/auth.json from grok login".into(),
                keywords: "grok cli auth.json import xai".into(),
                kind: Kind::Ai,
                icon: Icon::Name("document-open".into()),
                action: Action::ImportGrok,
            },
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
            crate::connectors::connect_item(&crate::connectors::PRESETS[0]),
            crate::connectors::connect_item(&crate::connectors::PRESETS[1]),
            crate::connectors::connect_item(&crate::connectors::PRESETS[2]),
            crate::connectors::connect_item(&crate::connectors::PRESETS[3]),
            crate::caldav::connect_item("apple-calendar", "Apple Calendar"),
            crate::caldav::connect_item("proton-calendar", "Proton Calendar"),
            setting_value(
                "tenor-key",
                "Tenor API key (GIF picker)",
                if crate::auth::has_api_key("tenor") || !s.connectors.tenor_key.is_empty() {
                    "••••••••"
                } else {
                    "(optional · free at tenor.com)"
                },
                q,
                "gif tenor giphy",
            ),
            setting_value(
                "notion-client",
                "Notion OAuth client ID",
                if s.connectors.notion_client_id.is_empty() {
                    "(for Connect Notion)"
                } else {
                    "••••••••"
                },
                q,
                "notion oauth",
            ),
            setting_value(
                "todoist-client",
                "Todoist OAuth client ID",
                if s.connectors.todoist_client_id.is_empty() {
                    "(for Connect Todoist)"
                } else {
                    "••••••••"
                },
                q,
                "todoist oauth",
            ),
            setting_value(
                "notion-secret",
                "Notion OAuth client secret",
                if crate::auth::has_api_key("notion-secret") {
                    "••••••••"
                } else {
                    "(required for Connect Notion)"
                },
                q,
                "notion oauth secret",
            ),
            setting_value(
                "todoist-secret",
                "Todoist OAuth client secret",
                if crate::auth::has_api_key("todoist-secret") {
                    "••••••••"
                } else {
                    "(required for Connect Todoist)"
                },
                q,
                "todoist oauth secret",
            ),
            setting_value(
                "outlook-client",
                "Outlook OAuth client ID",
                if s.connectors.outlook_client_id.is_empty() {
                    "(for Connect Outlook)"
                } else {
                    "••••••••"
                },
                q,
                "outlook microsoft oauth",
            ),
            setting_value(
                "apple-id",
                "Apple ID (iCloud Calendar)",
                if s.connectors.apple_id.is_empty() {
                    "(for Apple Calendar)"
                } else {
                    &s.connectors.apple_id
                },
                q,
                "apple icloud caldav",
            ),
            setting_value(
                "apple-password",
                "Apple app-specific password",
                if crate::auth::has_api_key("apple-calendar") {
                    "••••••••"
                } else {
                    "(type the password after selecting this)"
                },
                q,
                "apple password caldav",
            ),
            setting_value(
                "proton-user",
                "Proton Calendar user",
                if s.connectors.proton_user.is_empty() {
                    "(for Proton CalDAV)"
                } else {
                    &s.connectors.proton_user
                },
                q,
                "proton caldav",
            ),
            setting_value(
                "proton-password",
                "Proton Calendar app password",
                if crate::auth::has_api_key("proton-calendar") {
                    "••••••••"
                } else {
                    "(type the password after selecting this)"
                },
                q,
                "proton password caldav",
            ),
            setting_value(
                "caldav-url",
                "CalDAV URL",
                if s.connectors.caldav_url.is_empty() {
                    "https://caldav.icloud.com/"
                } else {
                    &s.connectors.caldav_url
                },
                q,
                "caldav url apple proton",
            ),
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
        items.extend(crate::extension::preference_items());
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
        let items: Vec<Item> = self
            .quicklinks
            .borrow()
            .iter()
            .map(|link| {
                let rest = q.strip_prefix('+').unwrap_or(q).trim();
                link.to_item(&link.argument_for(rest))
            })
            .collect();
        let rest = q.strip_prefix('+').unwrap_or(q).trim();
        results.extend(self.score_pool(&items, rest, 24));
        finish(results, &self.usage)
    }

    fn search_emoji(&self, query: &str) -> Vec<Scored> {
        crate::emoji::search(query, 48)
            .into_iter()
            .enumerate()
            .map(|(i, item)| Scored::new(item, 50_000u32.saturating_sub(i as u32)))
            .collect()
    }

    fn search_gif(&self, query: &str) -> Vec<Scored> {
        vec![Scored::new(crate::gif::search_item(query), 20_000)]
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
        finish(results, &self.usage)
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
        if let Some(item) = self
            .custom_layouts
            .borrow()
            .iter()
            .find(|item| item.id == id)
        {
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
            return self
                .quicklinks
                .borrow()
                .iter()
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
        let now = usage::now_secs();
        if let Some(item) = crate::focus::live_item() {
            out.push(Scored::new(item, 40_000));
        }
        if self.context_aware() {
            let ctx = self.context.borrow().clone();
            if ctx.is_fresh(now) {
                let mut text = None;
                if let Some(entry) = self.clips.borrow().entries.first()
                    && now.saturating_sub(entry.copied_at) < crate::context::FRESH_SECS
                    && !clipboard::looks_secret(&entry.text)
                {
                    text = Some(entry.text.clone());
                }
                if text.is_none() {
                    text = ctx.primary.clone();
                }
                if let Some(text) = text {
                    for (i, item) in crate::context::fresh_clipboard_items(&text)
                        .into_iter()
                        .enumerate()
                    {
                        out.push(Scored::new(item, 38_000u32.saturating_sub(i as u32 * 10)));
                    }
                }
                for path in crate::context::editor_paths(&ctx.title) {
                    out.push(Scored::new(files::file_item(path, None), 36_000));
                }
            }
        }
        let favs = self.favorites.borrow();
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
        | Mode::Gif
        | Mode::Content
        | Mode::Extension => false,
    }
}

pub fn live_needed(query: &str, mode: Mode, include_in_root: bool) -> bool {
    let (_, rest) = Mode::parse(query);
    if crate::content::term_from_query(query).is_some() {
        return true;
    }
    if Mode::parse(query).0 == Mode::Gif && !Mode::parse(query).1.trim().is_empty() {
        return true;
    }
    let meaning = intent::resolve(&rest);
    if mode == Mode::Root && meaning.has(IntentKind::Weather) && weather::cached().is_none() {
        return true;
    }
    if mode == Mode::Root
        && (meaning.has(IntentKind::Calendar)
            || meaning.has(IntentKind::Email)
            || meaning.has(IntentKind::Web)
            || (meaning.has(IntentKind::Gif) && !intent::gif_terms(&rest).is_empty()))
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
            TIER_WEATHER,
            Live::Weather {
                summary: snap.summary,
                location: snap.location,
                extra: snap.extra,
            },
        ));
    }

    let meaning = intent::resolve(&q);
    if meaning.has(IntentKind::Calendar) {
        extras.calendar = live_calendar(settings);
    }
    if meaning.has(IntentKind::Email) {
        extras.mail = live_mail(settings);
    }
    if meaning.has(IntentKind::Web) && !meaning.tool_intent() {
        extras.web = live_web(&q);
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

    if (mode == Mode::Gif && !q.trim().is_empty())
        || (mode == Mode::Root && meaning.has(IntentKind::Gif) && !intent::gif_terms(&q).is_empty())
    {
        let terms = if mode == Mode::Gif {
            q.to_string()
        } else {
            intent::gif_terms(&q)
        };
        let key = crate::auth::api_key("tenor")
            .or_else(|| {
                let legacy = settings.connectors.tenor_key.trim();
                (!legacy.is_empty()).then(|| legacy.to_string())
            })
            .or_else(|| std::env::var("TENOR_API_KEY").ok())
            .unwrap_or_default();
        if let Ok(hits) = crate::gif::search(&terms, &key, 16) {
            for (i, hit) in hits.iter().enumerate() {
                let item = crate::gif::to_item(hit);
                let live = crate::gif::cache_preview(hit)
                    .map(|path| Live::Image { path })
                    .unwrap_or(Live::None);
                extras.gifs.push(Scored::with_live(
                    item,
                    70_000u32.saturating_sub(i as u32 * 10),
                    live,
                ));
            }
        }
        if mode == Mode::Gif {
            return extras;
        }
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
    let mut nucleo_buf = Vec::new();
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
        let score = rank_file(
            &mut matcher,
            &pattern,
            &mut nucleo_buf,
            &q,
            &item,
            usage,
            now,
        );
        extras.files.push(Scored::with_live(item, score, live));
    }
    extras
}

fn extension_items() -> Vec<Item> {
    let mut items = vec![
        Item {
            id: "ext:emoji".into(),
            title: "Emoji".into(),
            subtitle: "Type emoji, pick one, Enter pastes it".into(),
            keywords: "emoji picker smile".into(),
            kind: Kind::Extension,
            icon: Icon::Name("face-smile".into()),
            action: Action::EnterMode(Mode::Emoji),
        },
        Item {
            id: "ext:gif".into(),
            title: "GIFs".into(),
            subtitle: "Type gif cats · pick one · Enter copies it".into(),
            keywords: "gif giphy tenor".into(),
            kind: Kind::Extension,
            icon: Icon::Name("image-x-generic".into()),
            action: Action::EnterMode(Mode::Gif),
        },
        Item {
            id: "ext:signin-grok".into(),
            title: "Sign in with Grok (xAI)".into(),
            subtitle: "Opens your browser · uses your SuperGrok / xAI subscription".into(),
            keywords: "sign in login oauth grok xai x.ai subscription super grok cursor".into(),
            kind: Kind::Ai,
            icon: Icon::Name("network-workgroup".into()),
            action: Action::SignIn {
                provider: "xai".into(),
            },
        },
        Item {
            id: "ext:ask".into(),
            title: "Ask AI".into(),
            subtitle: "Local models, Grok subscription, or an API key".into(),
            keywords: "ask ai chatgpt ollama claude gemini grok xai oauth".into(),
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
            subtitle: "Connections, plugins, skills, Ask AI — opens a window".into(),
            keywords: "prefs preferences config settings window".into(),
            kind: Kind::Extension,
            icon: Icon::Name("preferences-system".into()),
            action: Action::OpenPrefs { page: None },
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
    ];
    items.extend(crate::connectors::items());
    items
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

#[allow(clippy::too_many_arguments)]
fn score_pool(
    items: &[Item],
    query: &str,
    usage_map: &usage::Map,
    limit: usize,
    matcher: &mut Matcher,
    buf: &mut Vec<char>,
    index: &HashMap<String, IndexEntry>,
    aliases: &alias::Store,
    favs: &favorites::Store,
) -> Vec<Scored> {
    let query = query.trim();
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
    let q_lc = query.to_lowercase();
    let now = usage::now_secs();
    let mut ranked: Vec<(u32, &Item)> = Vec::new();
    for item in items {
        let owned;
        let entry = match index.get(&item.id) {
            Some(entry) => entry,
            None => {
                owned = IndexEntry::from_item(item, aliases.get(&item.id));
                &owned
            }
        };
        let input = RankInput {
            query,
            q_lc: &q_lc,
            item,
            entry,
            usage: usage_map,
            now,
            file_heavy: false,
            favorite: favs.is_pinned(&item.id),
        };
        if let Some(score) = rank(matcher, &pattern, buf, input) {
            ranked.push((score, item));
        } else if let Some(score) = rank_expansions(matcher, buf, &meaning.expansions, input) {
            ranked.push((score, item));
        }
    }
    take_scored(ranked, limit, usage_map)
}

fn usage_last(usage: &usage::Map, id: &str) -> u64 {
    usage.get(id).map(|rec| rec.last).unwrap_or(0)
}

fn take_scored(mut ranked: Vec<(u32, &Item)>, limit: usize, usage: &usage::Map) -> Vec<Scored> {
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| usage_last(usage, &b.1.id).cmp(&usage_last(usage, &a.1.id)))
            .then_with(|| a.1.title.cmp(&b.1.title))
    });
    ranked.dedup_by(|a, b| a.1.id == b.1.id);
    ranked.truncate(limit);
    ranked
        .into_iter()
        .map(|(score, item)| Scored::new(item.clone(), score))
        .collect()
}

fn finish(results: Vec<Scored>, usage: &usage::Map) -> Vec<Scored> {
    finish_limited(results, 48, usage)
}

fn finish_limited(mut results: Vec<Scored>, limit: usize, usage: &usage::Map) -> Vec<Scored> {
    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| usage_last(usage, &b.item.id).cmp(&usage_last(usage, &a.item.id)))
            .then_with(|| a.item.title.cmp(&b.item.title))
    });
    results.dedup_by(|a, b| a.item.id == b.item.id);
    results.truncate(limit);
    results
}

fn build_app_tags(links: &[crate::quicklinks::Link]) -> HashMap<String, String> {
    let mut tags = HashMap::new();
    for link in links {
        if !link.app.is_empty() {
            tags.insert(format!("link:{}", link.name), link.app.clone());
        }
    }
    for layout in crate::layout::load() {
        if !layout.app.is_empty() {
            tags.insert(format!("cmd:layout-{}", layout.name), layout.app);
        }
    }
    for snip in snippets::load() {
        if !snip.app.is_empty() {
            tags.insert(format!("snip:{}", snip.keyword), snip.app);
        }
    }
    tags
}

/// Insert live rows without reordering already-visible results.
/// New ids insert at their scored position only when that index is at or below
/// the current selection; otherwise they append. Existing ids update in place.
pub(crate) fn insert_live_stable(
    results: &mut Vec<Scored>,
    incoming: Vec<Scored>,
    selected_id: &str,
) {
    for row in incoming {
        if let Some(existing) = results
            .iter_mut()
            .find(|existing| existing.item.id == row.item.id)
        {
            *existing = row;
            continue;
        }
        let selected = results
            .iter()
            .position(|existing| existing.item.id == selected_id)
            .unwrap_or(0);
        let pos = scored_insert_pos(results, &row);
        if pos >= selected {
            results.insert(pos.min(results.len()), row);
        } else {
            results.push(row);
        }
    }
}

fn scored_insert_pos(results: &[Scored], row: &Scored) -> usize {
    results.partition_point(|existing| {
        existing.score > row.score
            || (existing.score == row.score && existing.item.title <= row.item.title)
    })
}

pub(crate) fn pin_weather_row_zero(results: &mut Vec<Scored>) {
    if let Some(idx) = results.iter().position(|row| row.item.id == "live:weather")
        && idx != 0
    {
        let row = results.remove(idx);
        results.insert(0, row);
    }
}

#[derive(Clone, Copy)]
struct RankInput<'a> {
    query: &'a str,
    q_lc: &'a str,
    item: &'a Item,
    entry: &'a IndexEntry,
    usage: &'a usage::Map,
    now: u64,
    file_heavy: bool,
    favorite: bool,
}

fn rank_expansions(
    matcher: &mut Matcher,
    buf: &mut Vec<char>,
    expansions: &[String],
    input: RankInput<'_>,
) -> Option<u32> {
    for expansion in expansions {
        let expanded = Pattern::parse(expansion, CaseMatching::Smart, Normalization::Smart);
        let exp_lc = expansion.to_lowercase();
        let mut next = input;
        next.query = expansion;
        next.q_lc = &exp_lc;
        if let Some(score) = rank(matcher, &expanded, buf, next) {
            return Some(score.saturating_sub(600));
        }
    }
    None
}

fn nucleo_norm(raw: u32, text: &str) -> u32 {
    let len = text.chars().count().max(4) as u32;
    raw / len
}

fn nucleo_field(matcher: &mut Matcher, pattern: &Pattern, buf: &mut Vec<char>, text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    buf.clear();
    let raw = pattern
        .score(Utf32Str::new(text, buf), matcher)
        .unwrap_or(0);
    nucleo_norm(raw, text)
}

fn rank(
    matcher: &mut Matcher,
    pattern: &Pattern,
    buf: &mut Vec<char>,
    input: RankInput<'_>,
) -> Option<u32> {
    let q_lc = input.q_lc;
    let entry = input.entry;

    let title_score = nucleo_field(matcher, pattern, buf, &input.item.title);
    let keyword_score = nucleo_field(matcher, pattern, buf, &entry.keywords).saturating_mul(3) / 5;
    let subtitle_score =
        nucleo_field(matcher, pattern, buf, &entry.subtitle_lc).saturating_mul(3) / 10;
    let nucleo = title_score.max(keyword_score).max(subtitle_score);

    let typo = typo_score(q_lc, entry).map(|s| s.min(TYPO_CAP));
    let alias_hit = !entry.alias_lc.is_empty()
        && !q_lc.is_empty()
        && (entry.alias_lc == q_lc || entry.alias_lc.starts_with(q_lc));

    let exact_title = !q_lc.is_empty() && entry.title_lc == q_lc;
    let title_prefix = !q_lc.is_empty() && entry.title_lc.starts_with(q_lc);
    let word_prefix = !q_lc.is_empty() && entry.words.iter().any(|word| word.starts_with(q_lc));
    let initials = !q_lc.is_empty()
        && (entry.initials == q_lc
            || (!entry.initials.is_empty() && entry.initials.starts_with(q_lc)));
    let keyword_hit = !q_lc.is_empty()
        && entry
            .keywords_lc
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| word == q_lc || word.starts_with(q_lc));

    let title_tier = if exact_title {
        TITLE_EXACT
    } else if title_prefix {
        TITLE_PREFIX
    } else if word_prefix || initials {
        TITLE_WORD_OR_INITIALS
    } else if keyword_hit {
        KEYWORD_PREFIX
    } else {
        0
    };

    if nucleo == 0 && typo.is_none() && !alias_hit && title_tier == 0 {
        return None;
    }

    let mut score = nucleo;
    if let Some(typo) = typo {
        score = score.max(typo);
    }
    score = score.saturating_add(title_tier);
    score = score.saturating_add(usage::score(input.usage.get(&input.item.id), input.now));
    score = score.saturating_add(match input.item.kind {
        Kind::Weather | Kind::Calendar | Kind::Mail => 220,
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
    if !entry.alias_lc.is_empty() {
        if !q_lc.is_empty() && entry.alias_lc == q_lc {
            score = score.saturating_add(50_000);
        } else if !q_lc.is_empty() && entry.alias_lc.starts_with(q_lc) {
            score = score.saturating_add(40_000);
        }
    }
    if input.favorite {
        score = score.saturating_add(8_000);
    }
    Some(score)
}

fn typo_score(q_lc: &str, entry: &IndexEntry) -> Option<u32> {
    if let Some(score) = intent::title_typo_score_lc(q_lc, &entry.title_lc) {
        return Some(score.min(TYPO_CAP));
    }
    if intent::is_adjacent_swap(q_lc, &entry.title_lc)
        || entry
            .words
            .first()
            .is_some_and(|word| intent::is_adjacent_swap(q_lc, word))
    {
        return Some(2_400);
    }
    entry
        .keywords_lc
        .split_whitespace()
        .find_map(|word| intent::title_typo_score_lc(q_lc, word))
        .map(|score| score.min(TYPO_CAP))
}

fn rank_file(
    matcher: &mut Matcher,
    pattern: &Pattern,
    buf: &mut Vec<char>,
    query: &str,
    item: &Item,
    usage_map: &usage::Map,
    now: u64,
) -> u32 {
    let path = std::path::Path::new(item.id.trim_start_matches("file:"));
    let type_search = files::parse_query(query).is_type_search();
    let q_lc = query.to_lowercase();
    let entry = IndexEntry::from_item(item, None);
    let nucleo = rank(
        matcher,
        pattern,
        buf,
        RankInput {
            query,
            q_lc: &q_lc,
            item,
            entry: &entry,
            usage: usage_map,
            now,
            file_heavy: true,
            favorite: false,
        },
    )
    .unwrap_or(800);
    let mut score = nucleo;
    if type_search {
        score = score.saturating_add(TIER_FILE_TYPE);
    } else {
        score = score.saturating_add(4_000);
    }
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

fn files_searching_item(label: &str) -> Item {
    Item {
        id: format!("files:searching:{label}"),
        title: format!("{label} · searching…"),
        subtitle: "Recent files will fill this list".into(),
        keywords: format!("{label} files search type"),
        kind: Kind::File,
        icon: Icon::Name("system-search".into()),
        action: Action::EnterMode(Mode::Files),
    }
}

fn log_rank_debug(query: &str, results: &[Scored]) {
    if !rank_debug_enabled() {
        return;
    }
    eprintln!("flint-rank query={query:?}");
    for (i, row) in results.iter().take(10).enumerate() {
        eprintln!(
            "flint-rank {i} ({}, {}, kind={:?} title={:?})",
            row.score, row.item.id, row.item.kind, row.item.title
        );
    }
}

fn rank_debug_enabled() -> bool {
    match std::env::var_os("FLINT_RANK_DEBUG") {
        Some(value) => value == "1",
        None => false,
    }
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

fn calendar_stub() -> Item {
    if crate::connectors::calendar_connected() {
        Item {
            id: "live:calendar".into(),
            title: "Today’s calendar".into(),
            subtitle: "Loading events…".into(),
            keywords: "calendar agenda schedule meetings".into(),
            kind: Kind::Calendar,
            icon: Icon::Name("office-calendar".into()),
            action: Action::Copy("Today’s calendar".into()),
        }
    } else {
        Item {
            id: "live:calendar".into(),
            title: "Today’s calendar".into(),
            subtitle: "Set Apple ID and app password in Settings".into(),
            keywords: "calendar agenda schedule meetings".into(),
            kind: Kind::Calendar,
            icon: Icon::Name("office-calendar".into()),
            action: Action::OpenPrefs {
                page: Some("connections".into()),
            },
        }
    }
}

fn ask_prompt_item(query: &str, settings: &crate::config::Settings) -> Item {
    let follow = crate::ai::current_thread();
    let subtitle = if let Some(thread) = &follow {
        format!(
            "Follow-up · {} · {} · {}",
            thread.title, settings.ai.provider, settings.ai.model
        )
    } else {
        format!("{} · {}", settings.ai.provider, settings.ai.model)
    };
    Item {
        id: format!("ask:{query}"),
        title: format!("Ask “{query}”"),
        subtitle,
        keywords: query.to_string(),
        kind: Kind::Ai,
        icon: Icon::Name("help-faq".into()),
        action: Action::AskAi {
            prompt: query.to_string(),
        },
    }
}

fn live_calendar(settings: &crate::config::Settings) -> Vec<Scored> {
    if !crate::connectors::calendar_connected() {
        return vec![Scored::new(calendar_stub(), 88_000)];
    }
    let mut out = Vec::new();
    for id in [
        "apple-calendar",
        "google-calendar",
        "outlook",
        "proton-calendar",
    ] {
        if let Ok(items) = crate::connectors::fetch(id, settings) {
            for (i, item) in items.into_iter().take(16).enumerate() {
                let live = if item.subtitle.is_empty() {
                    Live::None
                } else {
                    Live::Snippet {
                        text: item.subtitle.clone(),
                    }
                };
                out.push(Scored::with_live(
                    item,
                    86_000u32.saturating_sub(i as u32 * 10),
                    live,
                ));
            }
        }
    }
    if out.is_empty() {
        out.push(Scored::new(calendar_stub(), 88_000));
    }
    out
}

fn live_mail(settings: &crate::config::Settings) -> Vec<Scored> {
    match crate::mail::fetch(settings) {
        Ok(items) => items
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                let live = if item.subtitle.is_empty() {
                    Live::None
                } else {
                    Live::Snippet {
                        text: item.subtitle.clone(),
                    }
                };
                Scored::with_live(item, 86_000u32.saturating_sub(i as u32 * 10), live)
            })
            .collect(),
        Err(_) => vec![Scored::new(crate::mail::stub(settings), 88_000)],
    }
}

fn live_web(query: &str) -> Vec<Scored> {
    match crate::web::fetch(query) {
        Ok(rows) => {
            let mut out: Vec<Scored> = rows
                .into_iter()
                .enumerate()
                .map(|(i, (item, live))| {
                    Scored::with_live(item, 40_000u32.saturating_sub(i as u32 * 10), live)
                })
                .collect();
            if out.is_empty() {
                out.push(Scored::new(crate::web::browser_fallback(query), 500));
            } else {
                out.push(Scored::new(crate::web::browser_fallback(query), 300));
            }
            out
        }
        Err(_) => vec![Scored::new(crate::web::browser_fallback(query), 500)],
    }
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
    let mut forget = cmd(
        "clear-choices",
        "Clear learned choices",
        "Forget which apps you pick for a query",
        "edit-clear",
        Action::ClearChoices,
    );
    forget.keywords = "forget ranking memory choices query".into();
    items.push(forget);
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
    use crate::item::{Action, Icon, Item, Kind};
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
        let cal = catalog.search_fast("what's on my calendar today?").1;
        assert!(
            cal.iter().any(|row| row.item.id == "live:calendar"
                || row.item.kind == crate::item::Kind::Calendar
                || matches!(row.item.action, crate::item::Action::AskAi { .. })),
            "natural-language calendar must not fall through to a browser search"
        );
        assert!(
            !cal.iter().any(|row| matches!(
                row.item.action,
                crate::item::Action::OpenUri(ref url) if url.contains("duckduckgo")
            )),
            "calendar questions must stay in Flint"
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
        let weather_score = rank_item("we", &weather, &usage, now);
        let web_score = rank_item("we", &web, &usage, now);
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
        let usage = crate::usage::Map::new();
        let score = rank_item("weahter", &item, &usage, 1_800_000_000);
        assert!(
            score > 0,
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

    #[test]
    fn emoji_query_lists_glyphs_you_can_paste() {
        use crate::clipboard::Store;
        use crate::config::Settings;
        use crate::item::Action;
        use std::cell::RefCell;
        use std::rc::Rc;

        let catalog = super::Catalog::load(
            Rc::new(RefCell::new(Store::load())),
            Rc::new(RefCell::new(Settings::default())),
        );
        let (mode, rows) = catalog.search_fast("emoji");
        assert_eq!(mode, crate::mode::Mode::Emoji);
        assert!(rows.len() >= 8, "empty emoji query should list the picker");
        assert!(
            rows.iter()
                .any(|row| matches!(row.item.action, Action::Paste(ref g) if !g.is_empty())),
            "emoji rows must paste a glyph"
        );
        let smile = catalog.search_fast("emoji smile").1;
        assert!(smile.iter().any(|row| row.item.id.contains("smile")
            || row.item.title.contains('😄')
            || row.item.title.contains('😊')));
    }

    #[test]
    fn gif_query_offers_an_https_tenor_search() {
        use crate::clipboard::Store;
        use crate::config::Settings;
        use crate::item::Action;
        use std::cell::RefCell;
        use std::rc::Rc;

        let catalog = super::Catalog::load(
            Rc::new(RefCell::new(Store::load())),
            Rc::new(RefCell::new(Settings::default())),
        );
        let (mode, rows) = catalog.search_fast("gif cats");
        assert_eq!(mode, crate::mode::Mode::Gif);
        assert!(
            rows.iter()
                .any(|row| row.item.id.contains("gif")
                    && !matches!(row.item.action, Action::OpenUri(_))),
            "gif cats must stay in Flint, got {:?}",
            rows.iter()
                .map(|r| (&r.item.title, format!("{:?}", r.item.action)))
                .collect::<Vec<_>>()
        );
        assert!(super::live_needed(
            "gif cats",
            crate::mode::Mode::Gif,
            false
        ));
    }

    #[test]
    fn settings_command_opens_the_window() {
        use crate::clipboard::Store;
        use crate::config::Settings;
        use crate::item::Action;
        use std::cell::RefCell;
        use std::rc::Rc;

        let catalog = super::Catalog::load(
            Rc::new(RefCell::new(Store::load())),
            Rc::new(RefCell::new(Settings::default())),
        );
        let settings = catalog.search_fast("settings").1;
        assert!(
            settings
                .iter()
                .any(|row| matches!(row.item.action, Action::OpenPrefs { .. })),
            "Settings must open the window, not only a search list"
        );
    }

    #[test]
    fn sign_in_with_grok_is_findable() {
        use crate::clipboard::Store;
        use crate::config::Settings;
        use crate::item::Action;
        use std::cell::RefCell;
        use std::rc::Rc;

        let catalog = super::Catalog::load(
            Rc::new(RefCell::new(Store::load())),
            Rc::new(RefCell::new(Settings::default())),
        );
        let grok = catalog.search_fast("sign in grok").1;
        assert!(
            grok.iter().any(|row| matches!(
                row.item.action,
                Action::SignIn { ref provider } if provider == "xai"
            )),
            "typing sign in grok should surface Connect Grok"
        );
        let settings = catalog.search_fast("set grok").1;
        assert!(settings.iter().any(|row| row.item.id == "set:signin-xai"
            || row.item.id == "ext:signin-grok"
            || matches!(row.item.action, Action::SignIn { ref provider } if provider == "xai")));
    }

    fn test_item(id: &str, title: &str, keywords: &str) -> Item {
        Item {
            id: id.into(),
            title: title.into(),
            subtitle: String::new(),
            keywords: keywords.into(),
            kind: Kind::App,
            icon: Icon::None,
            action: Action::Copy(String::new()),
        }
    }

    fn test_catalog(apps: Vec<Item>) -> super::Catalog {
        let mut index = std::collections::HashMap::new();
        for item in &apps {
            index.insert(item.id.clone(), super::IndexEntry::from_item(item, None));
        }
        super::Catalog {
            apps,
            commands: Vec::new(),
            extensions: Vec::new(),
            installed: std::cell::RefCell::new(Vec::new()),
            lexicon: Vec::new(),
            index: std::cell::RefCell::new(index),
            aliases: std::cell::RefCell::new(crate::alias::Store::default()),
            favorites: std::cell::RefCell::new(crate::favorites::Store::default()),
            quicklinks: std::cell::RefCell::new(Vec::new()),
            custom_layouts: std::cell::RefCell::new(Vec::new()),
            windows: std::cell::RefCell::new(Vec::new()),
            matcher: std::cell::RefCell::new(nucleo_matcher::Matcher::new(
                nucleo_matcher::Config::DEFAULT,
            )),
            nucleo_buf: std::cell::RefCell::new(Vec::new()),
            context: std::cell::RefCell::new(crate::context::Context::default()),
            app_tags: std::cell::RefCell::new(std::collections::HashMap::new()),
            usage: crate::usage::Map::new(),
            choices: crate::choices::Map::new(),
            choice_ctx: crate::choices::ContextMap::new(),
            clips: std::rc::Rc::new(std::cell::RefCell::new(crate::clipboard::Store::default())),
            settings: std::rc::Rc::new(std::cell::RefCell::new(crate::config::Settings::default())),
        }
    }

    fn rank_item(query: &str, item: &Item, usage: &crate::usage::Map, now: u64) -> u32 {
        let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
        let pattern = nucleo_matcher::pattern::Pattern::parse(
            query,
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let entry = super::IndexEntry::from_item(item, None);
        let q_lc = query.trim().to_lowercase();
        let mut buf = Vec::new();
        super::rank(
            &mut matcher,
            &pattern,
            &mut buf,
            super::RankInput {
                query,
                q_lc: &q_lc,
                item,
                entry: &entry,
                usage,
                now,
                file_heavy: false,
                favorite: false,
            },
        )
        .unwrap_or(0)
    }

    #[test]
    fn learned_choice_is_row_zero() {
        let slack = test_item("app:slack", "Slack", "");
        let sleep = test_item("app:sleep", "Sleep", "");
        let helper = test_item("app:slack-helper", "Slack Helper", "");
        let mut catalog = test_catalog(vec![slack, sleep, helper]);
        let now = 1_800_000_000;
        let rec = crate::usage::Record {
            count: 2,
            last: now - 12 * crate::usage::HOUR,
        };
        catalog
            .choices
            .entry("sl".into())
            .or_default()
            .insert("app:slack".into(), rec);
        catalog.choices.entry("s".into()).or_default().insert(
            "app:slack".into(),
            crate::usage::Record {
                count: 0,
                last: rec.last,
            },
        );
        let sl = catalog.search_root("sl");
        assert_eq!(
            sl[0].item.id, "app:slack",
            "learned sl → Slack must be row 0"
        );
        let s = catalog.search_root("s");
        assert_eq!(s[0].item.id, "app:slack", "prefix s must still pick Slack");

        catalog
            .choices
            .entry("1+1".into())
            .or_default()
            .insert("app:slack".into(), rec);
        let calc = catalog.search_root("1+1");
        assert_eq!(
            calc[0].item.kind,
            Kind::Calc,
            "a complete calc expression stays above a learned choice"
        );
        assert_ne!(
            catalog.search_root("sl")[0].item.kind,
            Kind::Calc,
            "non-calc queries let the learned choice win"
        );
    }

    #[test]
    fn short_exact_title_beats_long_fuzzy_title() {
        let short = test_item("app:cat", "Cat", "");
        let long = test_item(
            "app:catalog",
            "Application Catalog Helper Service",
            "cat feline",
        );
        let usage = crate::usage::Map::new();
        let now = 1_800_000_000;
        let short_score = rank_item("cat", &short, &usage, now);
        let long_score = rank_item("cat", &long, &usage, now);
        assert!(
            short_score > long_score,
            "exact Cat ({short_score}) must beat a long haystack ({long_score})"
        );
    }

    #[test]
    fn same_day_use_cannot_overturn_exact_title() {
        let exact = test_item("app:fi", "Fi", "");
        let prefix = test_item("app:firefox", "Firefox", "");
        let now = 1_800_000_000;
        let mut usage = crate::usage::Map::new();
        usage.insert(
            prefix.id.clone(),
            crate::usage::Record {
                count: 1,
                last: now - 60,
            },
        );
        let exact_score = rank_item("fi", &exact, &usage, now);
        let prefix_score = rank_item("fi", &prefix, &usage, now);
        assert!(
            exact_score > prefix_score,
            "exact Fi ({exact_score}) must beat same-day Firefox prefix ({prefix_score})"
        );
    }

    #[test]
    fn same_day_use_overturns_prefix_vs_prefix() {
        let firefox = test_item("app:firefox", "Firefox", "");
        let files = test_item("app:files", "Files", "");
        let now = 1_800_000_000;
        let mut usage = crate::usage::Map::new();
        usage.insert(
            firefox.id.clone(),
            crate::usage::Record {
                count: 1,
                last: now - 60,
            },
        );
        let firefox_score = rank_item("fi", &firefox, &usage, now);
        let files_score = rank_item("fi", &files, &usage, now);
        assert!(
            firefox_score > files_score,
            "same-day Firefox ({firefox_score}) should beat unused Files ({files_score})"
        );
    }

    #[test]
    fn initials_match_visual_studio_code() {
        let vsc = test_item("app:code", "Visual Studio Code", "");
        let usage = crate::usage::Map::new();
        assert!(rank_item("vsc", &vsc, &usage, 1_800_000_000) > 0);
        let chrome = test_item("app:chrome", "Google Chrome", "");
        assert!(rank_item("gc", &chrome, &usage, 1_800_000_000) > 0);
    }

    #[test]
    fn bare_one_is_not_a_calc_row() {
        let one = test_item("app:1password", "1Password", "");
        let catalog = test_catalog(vec![one]);
        let rows = catalog.search_root("1");
        assert!(
            !rows.iter().any(|row| row.item.kind == Kind::Calc),
            "typing 1 must not put a calc row above 1Password"
        );
        assert_eq!(rows[0].item.id, "app:1password");
        let plus = catalog.search_root("1+1");
        assert_eq!(plus[0].item.kind, Kind::Calc);
        assert_eq!(plus[0].item.title, "2");
    }

    #[test]
    fn plain_app_query_has_no_emoji_rows() {
        let firefox = test_item("app:firefox", "Firefox", "");
        let catalog = test_catalog(vec![firefox]);
        let rows = catalog.search_root("fi");
        assert!(
            !rows.iter().any(|row| row.item.id.starts_with("emoji:")),
            "plain app queries must not leak emoji rows, got {:?}",
            rows.iter().map(|r| &r.item.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn type_word_video_puts_files_above_apps() {
        let player = test_item("app:player", "Movies", "Video Player");
        let catalog = test_catalog(vec![player]);
        let rows = catalog.search_root("video");
        assert!(
            rows[0].item.id.starts_with("files:searching"),
            "type-word video should reserve row 0 for files, got {}",
            rows[0].item.id
        );
        assert!(
            !rows.iter().any(|row| row.item.id.starts_with("emoji:")),
            "type-word queries suppress emoji"
        );
        let app = rows.iter().find(|row| row.item.id == "app:player");
        if let Some(app) = app {
            assert!(
                app.score <= rows[0].score,
                "keyword-Video app ({}) must not outrank files ({})",
                app.score,
                rows[0].score
            );
            assert!(app.score <= super::TIER_APP_IN_TYPE_SEARCH);
        }
    }

    #[test]
    fn weather_query_row_zero_is_live_weather_not_emoji() {
        let cloud = test_item("app:cloudy", "Cloudy", "weather");
        let catalog = test_catalog(vec![cloud]);
        let rows = catalog.search_root("weather");
        assert_eq!(rows[0].item.id, "live:weather");
        let weather_at = rows.iter().position(|r| r.item.id == "live:weather");
        let emoji_at = rows.iter().position(|r| r.item.id.starts_with("emoji:"));
        if let (Some(weather_at), Some(emoji_at)) = (weather_at, emoji_at) {
            assert!(weather_at < emoji_at, "emoji must never sit above weather");
        }
    }

    #[test]
    fn empty_fuzzy_pool_still_offers_web_search() {
        let catalog = test_catalog(Vec::new());
        let rows = catalog.search_root("zzzznotanappxyz");
        assert!(
            rows.iter().any(|row| row.item.id.starts_with("search:")
                && row.item.title.contains("Search the web")),
            "an unmatched query should still offer a web search row"
        );
    }

    #[test]
    fn mix_limit_honors_max_results() {
        let apps: Vec<Item> = (0..40)
            .map(|i| test_item(&format!("app:{i}"), &format!("Widget{i:02}"), "app"))
            .collect();
        let catalog = test_catalog(apps);
        catalog.settings.borrow_mut().general.max_results = 8;
        let rows = catalog.search_root("wid");
        assert!(
            rows.len() <= 8,
            "max_results=8 must not floor at 24, got {} rows: {:?}",
            rows.len(),
            rows.iter().map(|r| &r.item.id).collect::<Vec<_>>()
        );
        assert!(!rows.is_empty());
    }

    #[test]
    fn alias_is_indexed_into_keywords() {
        crate::db::with_temp(|_| {
            let slack = test_item("app:slack", "Slack", "");
            let catalog = test_catalog(vec![slack]);
            catalog.set_alias("app:slack", "slx");
            let entry = catalog.index.borrow().get("app:slack").cloned().unwrap();
            assert!(
                entry.keywords_lc.split_whitespace().any(|w| w == "slx"),
                "alias must land in keywords_lc, got {:?}",
                entry.keywords_lc
            );
            let rows = catalog.search_root("slx");
            assert_eq!(rows[0].item.id, "app:slack");
            assert!(rows[0].score >= super::TIER_ALIAS);
        });
    }

    fn scored(item: Item, score: u32) -> super::Scored {
        super::Scored::new(item, score)
    }

    #[test]
    fn live_rows_do_not_jump_above_selection() {
        let mut results = vec![
            scored(test_item("app:a", "A", ""), 10_000),
            scored(test_item("app:b", "B", ""), 9_000),
            scored(test_item("app:c", "C", ""), 8_000),
        ];
        let incoming = vec![scored(test_item("file:hot", "Hot", ""), 20_000)];
        super::insert_live_stable(&mut results, incoming, "app:b");
        assert_eq!(results[0].item.id, "app:a");
        assert_eq!(results[1].item.id, "app:b");
        assert_eq!(
            results.last().map(|r| r.item.id.as_str()),
            Some("file:hot"),
            "a higher-score live row must append when it would land above the selection"
        );
    }

    #[test]
    fn live_rows_insert_at_or_below_selection() {
        let mut results = vec![
            scored(test_item("app:a", "A", ""), 10_000),
            scored(test_item("app:b", "B", ""), 9_000),
            scored(test_item("app:c", "C", ""), 1_000),
        ];
        let incoming = vec![scored(test_item("file:mid", "Mid", ""), 5_000)];
        super::insert_live_stable(&mut results, incoming, "app:b");
        let ids: Vec<&str> = results.iter().map(|r| r.item.id.as_str()).collect();
        assert_eq!(ids, vec!["app:a", "app:b", "file:mid", "app:c"]);
    }

    #[test]
    fn live_weather_stays_at_row_zero() {
        let mut results = vec![
            scored(
                Item {
                    id: "live:weather".into(),
                    title: "Weather".into(),
                    subtitle: "Detecting…".into(),
                    keywords: String::new(),
                    kind: Kind::Weather,
                    icon: Icon::None,
                    action: Action::Copy(String::new()),
                },
                super::TIER_WEATHER,
            ),
            scored(test_item("app:a", "A", ""), 10_000),
        ];
        let incoming = vec![scored(test_item("file:hot", "Hot", ""), 200_000)];
        super::insert_live_stable(&mut results, incoming, "live:weather");
        super::pin_weather_row_zero(&mut results);
        assert_eq!(results[0].item.id, "live:weather");
    }

    #[test]
    fn empty_query_offers_fresh_clipboard_actions() {
        let catalog = test_catalog(Vec::new());
        catalog.settings.borrow_mut().general.context_aware = true;
        *catalog.context.borrow_mut() = crate::context::Context {
            class: "firefox".into(),
            title: String::new(),
            primary: Some("hello world".into()),
            captured_at: crate::usage::now_secs(),
        };
        let rows = catalog.search_root("");
        let ids: Vec<&str> = rows.iter().map(|r| r.item.id.as_str()).collect();
        assert!(ids.contains(&"ctx:paste"), "paste chip: {ids:?}");
        assert!(
            ids.iter().any(|id| id.starts_with("ctx:web:")),
            "web chip: {ids:?}"
        );
        assert!(
            ids.iter().any(|id| id.starts_with("ctx:ask:")),
            "ask chip: {ids:?}"
        );

        catalog.settings.borrow_mut().general.context_aware = false;
        let rows = catalog.search_root("");
        let ids: Vec<&str> = rows.iter().map(|r| r.item.id.as_str()).collect();
        assert!(
            !ids.iter().any(|id| id.starts_with("ctx:")),
            "context off hides clipboard chips: {ids:?}"
        );
    }

    #[test]
    fn empty_query_prefers_fresh_clipboard_over_primary() {
        let catalog = test_catalog(Vec::new());
        catalog
            .clips
            .borrow_mut()
            .entries
            .push(crate::clipboard::Entry {
                id: "c1".into(),
                text: "from clip".into(),
                copied_at: crate::usage::now_secs(),
                pinned: false,
                label: String::new(),
            });
        *catalog.context.borrow_mut() = crate::context::Context {
            class: "code".into(),
            title: String::new(),
            primary: Some("from primary".into()),
            captured_at: crate::usage::now_secs(),
        };
        let rows = catalog.search_root("");
        let paste = rows
            .iter()
            .find(|r| r.item.id == "ctx:paste")
            .expect("paste");
        assert_eq!(paste.item.subtitle, "from clip");
    }

    #[test]
    fn empty_query_seeds_editor_path_from_title() {
        let dir = std::env::temp_dir().join(format!(
            "flint-ctx-cat-{}-{}",
            std::process::id(),
            crate::usage::now_secs()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("main.rs");
        std::fs::write(&file, b"fn").unwrap();
        let catalog = test_catalog(Vec::new());
        *catalog.context.borrow_mut() = crate::context::Context {
            class: "code".into(),
            title: format!("{} — Visual Studio Code", file.display()),
            primary: None,
            captured_at: crate::usage::now_secs(),
        };
        let rows = catalog.search_root("");
        let _ = std::fs::remove_dir_all(&dir);
        let want = format!("file:{}", file.display());
        assert!(
            rows.iter().any(|r| r.item.id == want),
            "editor file missing from {ids:?}",
            ids = rows.iter().map(|r| r.item.id.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn context_class_choice_beats_global() {
        let firefox = test_item("app:firefox", "Firefox", "browser");
        let code = test_item("app:code", "Code", "editor");
        let mut catalog = test_catalog(vec![firefox, code]);
        let rec = crate::usage::Record {
            count: 2,
            last: crate::usage::now_secs() - 60,
        };
        catalog
            .choices
            .entry("ed".into())
            .or_default()
            .insert("app:firefox".into(), rec);
        catalog
            .choice_ctx
            .entry("kitty".into())
            .or_default()
            .entry("ed".into())
            .or_default()
            .insert("app:code".into(), rec);
        *catalog.context.borrow_mut() = crate::context::Context {
            class: "kitty".into(),
            title: String::new(),
            primary: None,
            captured_at: crate::usage::now_secs(),
        };
        let rows = catalog.search_root("ed");
        assert_eq!(
            rows[0].item.id,
            "app:code",
            "(query, class) must beat global (query, \"\"): {:?}",
            rows.iter().map(|r| r.item.id.as_str()).collect::<Vec<_>>()
        );

        catalog.settings.borrow_mut().general.context_aware = false;
        let rows = catalog.search_root("ed");
        assert_eq!(
            rows[0].item.id, "app:firefox",
            "context off falls back to global choice"
        );
    }

    #[test]
    fn matching_app_tag_boosts_item() {
        let a = test_item("app:alpha", "Alpha Helper", "");
        let b = test_item("app:beta", "Alpha Tools", "");
        let catalog = test_catalog(vec![a, b]);
        catalog
            .app_tags
            .borrow_mut()
            .insert("app:beta".into(), "firefox".into());
        *catalog.context.borrow_mut() = crate::context::Context {
            class: "Firefox".into(),
            title: String::new(),
            primary: None,
            captured_at: crate::usage::now_secs(),
        };
        let rows = catalog.search_root("alpha");
        assert_eq!(
            rows[0].item.id,
            "app:beta",
            "app tag matching focused class should win: {:?}",
            rows.iter().map(|r| r.item.id.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore]
    fn bench_root_search_p95() {
        let apps: Vec<Item> = (0..500)
            .map(|i| {
                test_item(
                    &format!("app:{i}"),
                    &format!("App Title {i} Extra Words"),
                    "launcher desktop command",
                )
            })
            .collect();
        let catalog = test_catalog(apps);
        let queries: Vec<String> = (0..50)
            .map(|i| match i % 10 {
                0 => "app".into(),
                1 => "ti".into(),
                2 => format!("App Title {i}"),
                3 => "zzzmissing".into(),
                4 => "extra".into(),
                5 => "command".into(),
                6 => "ap".into(),
                7 => format!("title {i}"),
                8 => "desk".into(),
                _ => "a".into(),
            })
            .collect();
        for q in &queries {
            let _ = catalog.search_root(q);
        }
        let mut times: Vec<std::time::Duration> = Vec::with_capacity(queries.len());
        for q in &queries {
            let start = std::time::Instant::now();
            let _ = catalog.search_root(q);
            times.push(start.elapsed());
        }
        times.sort();
        let p50 = times[times.len() / 2];
        let p95 = times[(times.len() * 95 / 100).min(times.len() - 1)];
        println!(
            "root search 500 items × 50 queries: p50={:?} p95={:?}",
            p50, p95
        );
        if !cfg!(debug_assertions) {
            assert!(
                p95.as_secs_f64() * 1000.0 < 2.0,
                "p95 {p95:?} exceeds 2 ms target"
            );
        }
    }

    #[test]
    fn today_file_beats_old_better_name() {
        use std::fs;
        use std::time::{Duration, SystemTime};

        let dir = std::env::temp_dir().join(format!(
            "flint-rank-files-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("video.mp4");
        let new = dir.join("IMG_1234.mp4");
        fs::write(&old, b"old").unwrap();
        fs::write(&new, b"new").unwrap();
        filetime_set(
            &old,
            SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 400),
        );

        let old_item = crate::files::file_item(old.clone(), Some("video"));
        let new_item = crate::files::file_item(new.clone(), Some("video"));
        let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
        let pattern = nucleo_matcher::pattern::Pattern::parse(
            "video",
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let usage = crate::usage::Map::new();
        let now = crate::usage::now_secs();
        let mut buf = Vec::new();
        let old_score = super::rank_file(
            &mut matcher,
            &pattern,
            &mut buf,
            "video",
            &old_item,
            &usage,
            now,
        );
        let new_score = super::rank_file(
            &mut matcher,
            &pattern,
            &mut buf,
            "video",
            &new_item,
            &usage,
            now,
        );
        let _ = fs::remove_dir_all(&dir);
        assert!(
            new_score > old_score,
            "today's clip ({new_score}) must beat a 2019-named video.mp4 ({old_score})"
        );
    }

    fn filetime_set(path: &std::path::Path, at: std::time::SystemTime) {
        let Ok(secs) = at.duration_since(std::time::UNIX_EPOCH) else {
            return;
        };
        let t = libc::timespec {
            tv_sec: secs.as_secs() as libc::time_t,
            tv_nsec: secs.subsec_nanos() as i64,
        };
        let times = [t, t];
        let cpath = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap();
        // SAFETY: `cpath` is a valid C string; `times` is two timespecs; AT_FDCWD
        // updates `path` in the test temp dir only.
        unsafe {
            libc::utimensat(libc::AT_FDCWD, cpath.as_ptr(), times.as_ptr(), 0);
        }
    }
}
