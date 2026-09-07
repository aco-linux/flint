use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use gtk4::gdk::{Key, ModifierType, Texture};
use gtk4::gio::prelude::AppInfoExt;
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box, CssProvider, DrawingArea, Entry,
    EventControllerKey, GestureClick, HeaderBar, Image, Label, Orientation, Overflow, Overlay,
    PolicyType, STYLE_PROVIDER_PRIORITY_APPLICATION, ScrolledWindow, TextView, WrapMode,
};

use crate::action;
use crate::ai;
use crate::alias;
use crate::auth;
use crate::calc;
use crate::catalog::{self, Catalog, LiveExtras, Scored};
use crate::clipboard;
use crate::extension;
use crate::favorites;
use crate::files;
use crate::hypr;
use crate::item::{Action, Icon, Item, Kind, Live};
use crate::mode::Mode;
use crate::models;
use crate::notes;
use crate::prefs;
use crate::quicklinks;
use crate::snippets;
use crate::store;
use crate::usage;
use crate::voice::{self, Session as VoiceSession};

const CSS: &str = include_str!("theme.css");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shortcut {
    Settings,
    Notes,
    Files,
    Actions,
    Ask,
    Save,
}

fn shortcut(key: Key, mods: ModifierType) -> Option<Shortcut> {
    let key = key.to_lower();
    let ctrl = mods.contains(ModifierType::CONTROL_MASK);
    let shift = mods.contains(ModifierType::SHIFT_MASK);
    let alt = mods.contains(ModifierType::ALT_MASK);
    if !ctrl || alt {
        return None;
    }
    match key {
        Key::k if !shift => Some(Shortcut::Actions),
        Key::comma if !shift => Some(Shortcut::Settings),
        Key::n if !shift => Some(Shortcut::Notes),
        Key::f if !shift => Some(Shortcut::Files),
        Key::s if !shift => Some(Shortcut::Save),
        Key::question => Some(Shortcut::Ask),
        Key::slash if shift => Some(Shortcut::Ask),
        _ => None,
    }
}

pub struct Shell {
    window: ApplicationWindow,
    entry: Entry,
    badge: Label,
    empty_title: Label,
    empty_sub: Label,
    results_host: Box,
    results_scroll: ScrolledWindow,
    preview: Box,
    preview_image: Image,
    preview_text: Label,
    empty: Box,
    status: Label,
    detail: ScrolledWindow,
    detail_view: TextView,
    action_panel: Box,
    action_list: Box,
    confetti: DrawingArea,
    confetti_bits: Rc<RefCell<Vec<Particle>>>,
    state: Rc<RefCell<State>>,
    live_jobs: Sender<LiveJob>,
    live_cancel: Arc<files::Cancel>,
    thumb_jobs: Sender<PathBuf>,
    thumb_cancel: Arc<files::Cancel>,
    prefs: Rc<RefCell<Option<prefs::Host>>>,
}

struct State {
    catalog: Catalog,
    results: Vec<Scored>,
    selected: usize,
    rows: Vec<Box>,
    mode: Mode,
    visible: bool,
    status: String,
    editing: Option<Editing>,
    actions_open: bool,
    actions: Vec<PanelAction>,
    action_selected: usize,
    saved_query: String,
    pending_alias: Option<String>,
    /// A running extension command. While set, the list belongs to it.
    extension: Option<extension::Session>,
    extension_gen: u64,
    pending_confirm: Option<PendingConfirm>,
    voice: VoiceSession,
    /// Bumped to cancel the 1s focus timeout when idle or replaced.
    focus_gen: u64,
    search_gen: u64,
    thumbs: HashMap<PathBuf, CachedThumb>,
    captions: HashMap<PathBuf, String>,
    thumb_miss: HashSet<PathBuf>,
}

struct CachedThumb {
    mtime: u64,
    texture: Texture,
}

#[derive(Clone)]
enum Editing {
    Note(String),
    ClipRename(String),
    ClipEdit(String),
    FormField {
        node: u64,
        prop: String,
        kind: String,
        field_id: String,
    },
}

struct PendingConfirm {
    id: u64,
    prompt: extension::ConfirmPrompt,
}

#[derive(Clone)]
struct PanelAction {
    title: String,
    keywords: String,
    kind: PanelKind,
}

#[derive(Clone)]
enum PanelKind {
    ToggleFavorite,
    SetAlias,
    Copy(String),
    CopyPath(PathBuf),
    Open,
    OpenWith {
        app: gtk4::gio::AppInfo,
        path: PathBuf,
    },
    ShowInFiles(PathBuf),
    Paste(String),
    ClipPin(String),
    ClipUnpin(String),
    ClipRename(String),
    ClipEdit(String),
    Launch,
    SnippetPaste(String),
    SnippetCopy(String),
    Run(Action),
    Confirm(Item),
}

struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    life: f64,
    size: f64,
    r: f64,
    g: f64,
    b: f64,
}

struct LiveJob {
    generation: u64,
    query: String,
    mode: Mode,
    settings: crate::config::Settings,
    usage: usage::Map,
}

pub fn build(app: &Application, catalog: Catalog) -> Shell {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Flint")
        .default_width(crate::WINDOW_WIDTH)
        .default_height(crate::WINDOW_HEIGHT)
        .decorated(true)
        .resizable(true)
        .icon_name("flint")
        .css_classes(["flint-root"])
        .build();
    window.set_hide_on_close(true);

    // A client-side header bar gives every Wayland compositor a real drag target
    // and standard window controls, even when server-side decorations are disabled.
    let titlebar = HeaderBar::new();
    titlebar.add_css_class("flint-titlebar");
    titlebar.set_show_title_buttons(true);
    let title = Label::new(Some("Flint"));
    title.add_css_class("flint-title");
    titlebar.set_title_widget(Some(&title));
    window.set_titlebar(Some(&titlebar));

    load_css();

    let overlay = Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    let scrim = Box::new(Orientation::Vertical, 0);
    scrim.add_css_class("scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    overlay.set_child(Some(&scrim));

    let panel = Box::new(Orientation::Vertical, 0);
    panel.add_css_class("panel");
    panel.set_halign(Align::Fill);
    panel.set_valign(Align::Fill);
    panel.set_hexpand(true);
    panel.set_vexpand(true);
    panel.set_margin_top(18);
    panel.set_margin_bottom(18);
    panel.set_margin_start(18);
    panel.set_margin_end(18);
    panel.set_overflow(Overflow::Hidden);

    let search_row = Box::new(Orientation::Horizontal, 0);
    search_row.add_css_class("search-row");
    search_row.set_valign(Align::Center);

    let logo = Image::from_file(crate::paths::logo_app());
    logo.set_pixel_size(32);
    logo.set_valign(Align::Center);
    logo.add_css_class("search-logo");
    search_row.append(&logo);

    let entry = Entry::builder()
        .placeholder_text(Mode::Root.placeholder())
        .hexpand(true)
        .css_classes(["search"])
        .build();
    entry.set_has_frame(false);
    search_row.append(&entry);

    let badge = Label::new(None);
    badge.add_css_class("mode-badge");
    badge.set_visible(false);
    search_row.append(&badge);
    panel.append(&search_row);

    let rule = Box::new(Orientation::Horizontal, 0);
    rule.add_css_class("search-rule");
    panel.append(&rule);

    let status = Label::new(None);
    status.add_css_class("status");
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_visible(false);
    panel.append(&status);

    let results_host = Box::new(Orientation::Vertical, 2);
    results_host.add_css_class("results");

    let (empty, empty_title, empty_sub) = empty_state();
    results_host.append(&empty);

    let scroll = ScrolledWindow::builder()
        .min_content_height(72)
        .vexpand(true)
        .hexpand(true)
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(&results_host)
        .css_classes(["results-scroll"])
        .build();

    let preview_image = Image::new();
    preview_image.set_pixel_size(240);
    preview_image.add_css_class("preview-image");
    preview_image.set_visible(false);

    let preview_text = Label::new(None);
    preview_text.set_xalign(0.0);
    preview_text.set_yalign(0.0);
    preview_text.set_wrap(true);
    preview_text.set_wrap_mode(pango::WrapMode::WordChar);
    preview_text.set_selectable(true);
    preview_text.add_css_class("preview-text");

    let preview_inner = Box::new(Orientation::Vertical, 10);
    preview_inner.append(&preview_image);
    preview_inner.append(&preview_text);

    let preview_scroll = ScrolledWindow::builder()
        .min_content_width(280)
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(&preview_inner)
        .build();

    let preview = Box::new(Orientation::Vertical, 0);
    preview.add_css_class("preview");
    preview.set_hexpand(false);
    preview.set_vexpand(true);
    preview.set_width_request(300);
    preview.append(&preview_scroll);
    preview.set_visible(false);

    let body = Box::new(Orientation::Horizontal, 0);
    body.add_css_class("body");
    body.set_hexpand(true);
    body.set_vexpand(true);
    body.append(&scroll);
    body.append(&preview);
    panel.append(&body);

    let detail_view = TextView::builder()
        .wrap_mode(WrapMode::WordChar)
        .accepts_tab(false)
        .css_classes(["note-view"])
        .build();
    detail_view.set_top_margin(12);
    detail_view.set_bottom_margin(12);
    detail_view.set_left_margin(18);
    detail_view.set_right_margin(18);
    let detail = ScrolledWindow::builder()
        .min_content_height(220)
        .max_content_height(360)
        .hscrollbar_policy(PolicyType::Never)
        .child(&detail_view)
        .css_classes(["note-scroll"])
        .build();
    detail.set_visible(false);
    panel.append(&detail);
    panel.append(&footer());

    overlay.add_overlay(&panel);

    let action_panel = Box::new(Orientation::Vertical, 2);
    action_panel.add_css_class("action-panel");
    action_panel.set_halign(Align::End);
    action_panel.set_valign(Align::End);
    action_panel.set_hexpand(false);
    action_panel.set_vexpand(false);
    action_panel.set_visible(false);
    let action_heading = Label::new(Some("ACTIONS"));
    action_heading.add_css_class("action-panel-title");
    action_heading.set_xalign(0.0);
    action_panel.append(&action_heading);
    let action_list = Box::new(Orientation::Vertical, 2);
    action_panel.append(&action_list);
    overlay.add_overlay(&action_panel);

    let confetti_bits = Rc::new(RefCell::new(Vec::<Particle>::new()));
    let confetti = DrawingArea::new();
    confetti.add_css_class("confetti");
    confetti.set_hexpand(true);
    confetti.set_vexpand(true);
    confetti.set_halign(Align::Fill);
    confetti.set_valign(Align::Fill);
    confetti.set_can_target(false);
    confetti.set_visible(false);
    confetti.set_draw_func({
        let bits = confetti_bits.clone();
        move |_, cr, _w, _h| {
            for p in bits.borrow().iter() {
                if p.life <= 0.0 {
                    continue;
                }
                cr.set_source_rgba(p.r, p.g, p.b, p.life.clamp(0.0, 1.0));
                cr.rectangle(p.x, p.y, p.size, p.size);
                let _ = cr.fill();
            }
        }
    });
    overlay.add_overlay(&confetti);

    window.set_child(Some(&overlay));

    let (live_tx, live_rx) = mpsc::channel();
    let live_cancel = files::Cancel::new();
    let (thumb_tx, thumb_rx) = mpsc::channel();
    let thumb_cancel = files::Cancel::new();

    let shell = Shell {
        window: window.clone(),
        entry: entry.clone(),
        badge: badge.clone(),
        empty_title: empty_title.clone(),
        empty_sub: empty_sub.clone(),
        results_host: results_host.clone(),
        results_scroll: scroll.clone(),
        preview: preview.clone(),
        preview_image: preview_image.clone(),
        preview_text: preview_text.clone(),
        empty: empty.clone(),
        status: status.clone(),
        detail: detail.clone(),
        detail_view: detail_view.clone(),
        action_panel: action_panel.clone(),
        action_list: action_list.clone(),
        confetti: confetti.clone(),
        confetti_bits: confetti_bits.clone(),
        state: Rc::new(RefCell::new(State {
            catalog,
            results: Vec::new(),
            selected: 0,
            rows: Vec::new(),
            mode: Mode::Root,
            visible: false,
            status: String::new(),
            editing: None,
            actions_open: false,
            actions: Vec::new(),
            action_selected: 0,
            saved_query: String::new(),
            pending_alias: None,
            extension: None,
            extension_gen: 0,
            pending_confirm: None,
            voice: VoiceSession::new(),
            focus_gen: 0,
            search_gen: 0,
            thumbs: HashMap::new(),
            captions: HashMap::new(),
            thumb_miss: HashSet::new(),
        })),
        live_jobs: live_tx,
        live_cancel: live_cancel.clone(),
        thumb_jobs: thumb_tx,
        thumb_cancel: thumb_cancel.clone(),
        prefs: Rc::new(RefCell::new(None)),
    };

    bind_shell(&shell);
    start_live_worker(live_rx, live_cancel);
    start_thumb_worker(thumb_rx, thumb_cancel);
    start_hypr_watch();
    hypr::install_float_rule();

    {
        let shell = shell.clone();
        scroll.vadjustment().connect_value_changed(move |_| {
            shell.request_visible_thumbs();
        });
    }

    {
        let shell = shell.clone();
        let panel = panel.clone();
        let click = GestureClick::new();
        click.connect_released(move |_, _, x, y| {
            let bounds = panel.compute_bounds(&shell.window);
            if let Some(rect) = bounds {
                let inside = x >= f64::from(rect.x())
                    && y >= f64::from(rect.y())
                    && x <= f64::from(rect.x() + rect.width())
                    && y <= f64::from(rect.y() + rect.height());
                if !inside && shell.state.borrow().voice.state() == voice::State::Idle {
                    shell.hide();
                }
            }
        });
        scrim.add_controller(click);
    }

    {
        let shell = shell.clone();
        entry.connect_changed(move |_| {
            if shell.state.borrow().editing.is_some() {
                return;
            }
            if shell.state.borrow().actions_open {
                shell.refresh_actions();
                return;
            }
            shell.refresh();
        });
    }

    {
        let shell = shell.clone();
        let keys = EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, mods| shell.on_key(key, mods));
        window.add_controller(keys);
    }

    window.connect_close_request({
        let shell = shell.clone();
        move |_| {
            shell.hide();
            Propagation::Stop
        }
    });

    window.set_visible(false);
    shell
}

impl Clone for Shell {
    fn clone(&self) -> Self {
        Self {
            window: self.window.clone(),
            entry: self.entry.clone(),
            badge: self.badge.clone(),
            empty_title: self.empty_title.clone(),
            empty_sub: self.empty_sub.clone(),
            results_host: self.results_host.clone(),
            results_scroll: self.results_scroll.clone(),
            preview: self.preview.clone(),
            preview_image: self.preview_image.clone(),
            preview_text: self.preview_text.clone(),
            empty: self.empty.clone(),
            status: self.status.clone(),
            detail: self.detail.clone(),
            detail_view: self.detail_view.clone(),
            action_panel: self.action_panel.clone(),
            action_list: self.action_list.clone(),
            confetti: self.confetti.clone(),
            confetti_bits: self.confetti_bits.clone(),
            state: self.state.clone(),
            live_jobs: self.live_jobs.clone(),
            live_cancel: self.live_cancel.clone(),
            thumb_jobs: self.thumb_jobs.clone(),
            thumb_cancel: self.thumb_cancel.clone(),
            prefs: self.prefs.clone(),
        }
    }
}

impl Shell {
    pub fn toggle(&self) {
        if self.state.borrow().voice.state() == voice::State::Listening
            && self.state.borrow().voice.dest() == voice::Dest::FocusedApp
        {
            self.finish_global_dictation();
            return;
        }
        if self.state.borrow().visible {
            self.hide();
        } else if self.state.borrow().extension.is_some() {
            self.restore();
        } else {
            self.open(Mode::Root);
        }
    }

    pub fn open(&self, mode: Mode) {
        self.state.borrow_mut().visible = true;
        self.show_launcher();
        self.enter_mode(mode);
        self.entry.grab_focus();
        self.refresh();
    }

    fn restore(&self) {
        self.state.borrow_mut().visible = true;
        self.show_launcher();
        self.entry.grab_focus();
        if self.state.borrow().extension.is_some() {
            self.refresh_extension();
        } else {
            self.refresh();
        }
    }

    fn show_launcher(&self) {
        hypr::float_launcher();
        self.window
            .set_default_size(crate::WINDOW_WIDTH, crate::WINDOW_HEIGHT);
        self.window.unmaximize();
        self.window.present();
    }

    pub fn hide(&self) {
        self.hide_inner(true);
    }

    fn hide_keep_voice(&self) {
        self.hide_inner(false);
    }

    fn hide_inner(&self, cancel_voice: bool) {
        self.commit_editing();
        if self.state.borrow().pending_confirm.is_some() {
            self.finish_confirm(false);
        }
        self.close_actions_inner(false);
        if cancel_voice && self.state.borrow().voice.state() != voice::State::Idle {
            self.state.borrow().voice.cancel();
        }
        self.state.borrow_mut().visible = false;
        self.window.set_visible(false);
    }

    fn refresh(&self) {
        let query = self.entry.text().to_string();
        let (generation, include_in_root) = {
            let mut st = self.state.borrow_mut();
            if st.editing.is_some() || st.actions_open {
                return;
            }
            if st.extension.is_some() {
                drop(st);
                self.refresh_extension();
                return;
            }
            st.search_gen = st.search_gen.saturating_add(1);
            let (mode, mut results) = st.catalog.search_fast(&query);
            if let Some(item_id) = st.pending_alias.clone()
                && let Some(name) = alias_typed(&query)
            {
                let title = st
                    .catalog
                    .lookup_item(&item_id)
                    .map(|item| item.title)
                    .unwrap_or_else(|| item_id.clone());
                results.insert(
                    0,
                    Scored::new(
                        Item {
                            id: "cmd:apply-alias".into(),
                            title: format!("Set alias “{name}”"),
                            subtitle: format!("Nickname for {title}"),
                            keywords: name,
                            kind: Kind::Command,
                            icon: Icon::Name("insert-text".into()),
                            action: Action::Copy(item_id),
                        },
                        200_000,
                    ),
                );
            }
            st.mode = mode;
            st.results = results;
            st.selected = 0;
            let include_in_root = st.catalog.settings.borrow().files.include_in_root;
            (st.search_gen, include_in_root)
        };
        self.sync_chrome();
        rebuild_rows(self);
        self.update_preview();
        self.request_visible_thumbs();
        self.schedule_live(generation, query, include_in_root);
    }

    fn schedule_live(&self, generation: u64, query: String, include_in_root: bool) {
        let mode = self.state.borrow().mode;
        self.live_cancel.cancel();
        if !catalog::live_needed(&query, mode, include_in_root) {
            return;
        }
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        let usage = self.state.borrow().catalog.usage.clone();
        let _ = self.live_jobs.send(LiveJob {
            generation,
            query,
            mode,
            settings,
            usage,
        });
    }

    fn apply_live(&self, live: LiveExtras) {
        {
            let mut st = self.state.borrow_mut();
            let selected = st
                .results
                .get(st.selected)
                .map(|row| row.item.id.clone())
                .unwrap_or_default();
            if let Some(weather) = live.weather {
                st.results.retain(|row| row.item.id != "live:weather");
                st.results.insert(0, weather);
            }
            let incoming: std::collections::HashSet<String> =
                live.files.iter().map(|row| row.item.id.clone()).collect();
            st.results.retain(|row| !incoming.contains(&row.item.id));
            st.results.extend(live.files);
            let incoming_gifs: std::collections::HashSet<String> =
                live.gifs.iter().map(|row| row.item.id.clone()).collect();
            st.results
                .retain(|row| !incoming_gifs.contains(&row.item.id));
            st.results.extend(live.gifs);
            st.results.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then_with(|| a.item.title.cmp(&b.item.title))
            });
            st.results.dedup_by(|a, b| a.item.id == b.item.id);
            if let Some(idx) = st.results.iter().position(|row| row.item.id == selected) {
                st.selected = idx;
            } else {
                st.selected = 0;
            }
        }
        self.sync_chrome();
        rebuild_rows(self);
        self.update_preview();
        self.request_visible_thumbs();
    }

    fn apply_hypr_windows(&self, windows: Vec<crate::item::Item>) {
        let query = self.entry.text().to_string();
        let visible = {
            let st = self.state.borrow();
            st.visible && st.editing.is_none() && !st.actions_open
        };
        self.state.borrow().catalog.adopt_windows(windows);
        if !visible {
            return;
        }
        {
            let mut st = self.state.borrow_mut();
            let selected = st
                .results
                .get(st.selected)
                .map(|row| row.item.id.clone())
                .unwrap_or_default();
            let scored = st.catalog.score_windows(&query);
            st.results.retain(|row| !row.item.id.starts_with("win:"));
            st.results.extend(scored);
            st.results.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then_with(|| a.item.title.cmp(&b.item.title))
            });
            st.results.dedup_by(|a, b| a.item.id == b.item.id);
            if let Some(idx) = st.results.iter().position(|row| row.item.id == selected) {
                st.selected = idx;
            }
        }
        self.sync_chrome();
        rebuild_rows(self);
        self.update_preview();
    }

    fn show_preview_image(&self, path: &std::path::Path) {
        if let Some(texture) = self.cached_texture(path) {
            self.preview_image.set_paintable(Some(&texture));
            self.preview_image.set_visible(true);
            return;
        }
        let already_missed = self.state.borrow().thumb_miss.contains(path);
        self.preview_image.set_icon_name(Some("image-x-generic"));
        self.preview_image.set_visible(true);
        if already_missed {
            return;
        }
        self.request_thumb(path.to_path_buf());
    }

    fn cached_texture(&self, path: &std::path::Path) -> Option<Texture> {
        let mtime = crate::preview::mtime_secs(path);
        let st = self.state.borrow();
        st.thumbs.get(path).and_then(|cached| {
            if cached.mtime == mtime {
                Some(cached.texture.clone())
            } else {
                None
            }
        })
    }

    fn request_thumb(&self, path: PathBuf) {
        self.thumb_cancel.cancel();
        let _ = self.thumb_jobs.send(path);
    }

    fn request_visible_thumbs(&self) {
        let adj = self.results_scroll.vadjustment();
        let view_top = adj.value();
        let view_bottom = view_top + adj.page_size();
        let (selected, bounds, paths) = {
            let st = self.state.borrow();
            let bounds: Vec<Option<(f64, f64)>> = st
                .rows
                .iter()
                .map(|row| {
                    row.compute_bounds(&self.results_host)
                        .map(|rect| (f64::from(rect.y()), f64::from(rect.height())))
                })
                .collect();
            let paths: Vec<Option<PathBuf>> = st
                .results
                .iter()
                .map(|row| row.live.thumb_path().map(Path::to_path_buf))
                .collect();
            (st.selected, bounds, paths)
        };
        for idx in thumb_rows_in_view(paths.len(), selected, view_top, view_bottom, &bounds) {
            let Some(Some(path)) = paths.get(idx) else {
                continue;
            };
            if self.cached_texture(path).is_none() {
                let _ = self.thumb_jobs.send(path.clone());
            }
        }
    }

    fn apply_thumb(&self, path: PathBuf, texture: Texture) {
        let mtime = crate::preview::mtime_secs(&path);
        let (visible, selected_is, in_results) = {
            let mut st = self.state.borrow_mut();
            st.thumb_miss.remove(&path);
            st.thumbs.insert(
                path.clone(),
                CachedThumb {
                    mtime,
                    texture: texture.clone(),
                },
            );
            let selected_is = st
                .results
                .get(st.selected)
                .and_then(|row| row.live.thumb_path())
                .is_some_and(|live| live == path);
            let in_results = st
                .results
                .iter()
                .any(|row| row.live.thumb_path().is_some_and(|live| live == path));
            (st.visible, selected_is, in_results)
        };
        if !visible {
            return;
        }
        if selected_is {
            self.preview_image.set_paintable(Some(&texture));
            self.preview_image.set_visible(true);
        }
        if in_results {
            rebuild_rows(self);
        }
    }

    fn apply_caption(&self, path: PathBuf, text: String) {
        let selected = {
            let mut st = self.state.borrow_mut();
            st.captions.insert(path.clone(), text.clone());
            st.visible
                && st
                    .results
                    .get(st.selected)
                    .and_then(|row| row.live.thumb_path())
                    .is_some_and(|live| live == path)
        };
        if selected {
            self.update_preview();
        }
    }

    fn update_preview(&self) {
        if self.state.borrow().editing.is_some() {
            self.preview.set_visible(false);
            return;
        }
        let item = {
            let st = self.state.borrow();
            st.results.get(st.selected).map(|row| row.item.clone())
        };
        let Some(item) = item else {
            self.preview.set_visible(false);
            return;
        };
        match crate::preview::for_item(&item) {
            crate::preview::Preview::None => self.preview.set_visible(false),
            crate::preview::Preview::Text(text) => {
                self.preview_image.set_visible(false);
                self.preview_text.set_text(&text);
                self.preview_text.set_visible(true);
                self.preview.set_visible(true);
            }
            crate::preview::Preview::Image(path) => {
                self.show_preview_image(&path);
                self.preview_text.set_text(&path.to_string_lossy());
                self.preview_text.set_visible(true);
                self.preview.set_visible(true);
            }
            crate::preview::Preview::Media { path, hint } => {
                self.show_preview_image(&path);
                let caption = self.state.borrow().captions.get(&path).cloned();
                let body = match caption {
                    Some(extra) if !extra.is_empty() => format!("{hint}\n\n{extra}"),
                    _ => hint,
                };
                self.preview_text.set_text(&body);
                self.preview_text.set_visible(true);
                self.preview.set_visible(true);
            }
        }
    }

    fn sync_chrome(&self) {
        let st = self.state.borrow();
        let mode = st.mode;
        self.entry.set_placeholder_text(Some(mode.placeholder()));
        self.empty_title.set_text(mode.empty_title());
        self.empty_sub.set_text(mode.empty_sub());
        if let Some(badge) = mode.badge() {
            self.badge.set_text(badge);
            self.badge.set_visible(true);
        } else {
            self.badge.set_visible(false);
        }
        if st.status.is_empty() {
            if let Some(hint) = result_hint(&st) {
                self.status.set_text(&hint);
                self.status.set_visible(true);
            } else {
                self.status.set_visible(false);
            }
        } else {
            self.status.set_text(&st.status);
            self.status.set_visible(true);
        }
        let editing = match &st.editing {
            Some(Editing::Note(_) | Editing::ClipEdit(_)) => true,
            Some(Editing::FormField { kind, .. }) if kind == "textarea" => true,
            _ => false,
        };
        self.detail.set_visible(editing);
        if editing {
            self.preview.set_visible(false);
        }
    }

    fn set_status(&self, text: impl Into<String>) {
        let text = text.into();
        self.state.borrow_mut().status = text.clone();
        if text.is_empty() {
            self.status.set_visible(false);
        } else {
            self.status.set_text(&text);
            self.status.set_visible(true);
        }
    }

    fn on_key(&self, key: Key, mods: ModifierType) -> Propagation {
        match shortcut(key, mods) {
            Some(Shortcut::Settings) => {
                self.close_actions();
                self.open_prefs(None);
                Propagation::Stop
            }
            Some(Shortcut::Notes) => {
                self.close_actions();
                self.enter_mode(Mode::Notes);
                Propagation::Stop
            }
            Some(Shortcut::Files) => {
                self.close_actions();
                self.enter_mode(Mode::Files);
                Propagation::Stop
            }
            Some(Shortcut::Actions) => {
                if self.state.borrow().editing.is_some() {
                    Propagation::Proceed
                } else {
                    self.toggle_actions();
                    Propagation::Stop
                }
            }
            Some(Shortcut::Ask) => {
                self.close_actions();
                self.enter_mode(Mode::Ask);
                Propagation::Stop
            }
            Some(Shortcut::Save) if self.state.borrow().editing.is_some() => {
                let was_form =
                    matches!(self.state.borrow().editing, Some(Editing::FormField { .. }));
                self.commit_editing();
                self.set_status("Saved");
                if was_form && self.state.borrow().extension.is_some() {
                    self.refresh_extension();
                }
                Propagation::Stop
            }
            Some(Shortcut::Save) => Propagation::Proceed,
            None => match key {
                Key::Escape => {
                    if self.state.borrow().actions_open {
                        self.close_actions();
                    } else if self.state.borrow().editing.is_some() {
                        let back = match self.state.borrow().editing {
                            Some(Editing::Note(_)) => Some(Mode::Notes),
                            Some(Editing::ClipRename(_) | Editing::ClipEdit(_)) => {
                                Some(Mode::Clipboard)
                            }
                            Some(Editing::FormField { .. }) | None => None,
                        };
                        self.commit_editing();
                        if let Some(mode) = back {
                            self.enter_mode(mode);
                        } else if self.state.borrow().extension.is_some() {
                            self.refresh_extension();
                        }
                    } else if self.state.borrow().voice.state() == voice::State::Listening {
                        self.state.borrow().voice.cancel();
                        self.set_status("Dictation cancelled");
                    } else if self.state.borrow().pending_confirm.is_some() {
                        self.finish_confirm(false);
                    } else if self.state.borrow().extension.is_some() {
                        self.extension_back();
                    } else if self.state.borrow().mode == Mode::Ask
                        && crate::ai::current_id().is_some()
                    {
                        crate::ai::new_chat();
                        self.set_status("New chat");
                        self.entry.set_text("? ");
                        self.entry.set_position(-1);
                        self.refresh();
                    } else if self.state.borrow().mode != Mode::Root {
                        self.enter_mode(Mode::Root);
                    } else {
                        self.hide();
                    }
                    Propagation::Stop
                }
                Key::Down | Key::Tab if self.state.borrow().editing.is_none() => {
                    self.move_selection(1);
                    Propagation::Stop
                }
                Key::Page_Down if self.state.borrow().editing.is_none() => {
                    self.move_selection(8);
                    Propagation::Stop
                }
                Key::Up | Key::ISO_Left_Tab if self.state.borrow().editing.is_none() => {
                    self.move_selection(-1);
                    Propagation::Stop
                }
                Key::Page_Up if self.state.borrow().editing.is_none() => {
                    self.move_selection(-8);
                    Propagation::Stop
                }
                Key::space | Key::KP_Space => {
                    if self.state.borrow().editing.is_some() || self.state.borrow().actions_open {
                        Propagation::Proceed
                    } else {
                        let item = {
                            let st = self.state.borrow();
                            st.results.get(st.selected).map(|row| row.item.clone())
                        };
                        if let Some(item) = item
                            && let Some(play) = action::spacebar_play(&item)
                        {
                            usage::bump(&item.id);
                            self.hide();
                            action::run(&play);
                            Propagation::Stop
                        } else {
                            Propagation::Proceed
                        }
                    }
                }
                Key::Return | Key::KP_Enter
                    if mods.contains(ModifierType::SHIFT_MASK)
                        && !self.state.borrow().actions_open
                        && self.state.borrow().extension.is_some() =>
                {
                    let actions = {
                        let st = self.state.borrow();
                        st.results
                            .get(st.selected)
                            .and_then(|row| match &row.item.action {
                                Action::Extension { actions, .. } => Some(actions.clone()),
                                _ => None,
                            })
                    };
                    if let Some(actions) = actions {
                        self.run_extension_action(&actions, 1);
                    }
                    Propagation::Stop
                }
                Key::Return | Key::KP_Enter => {
                    if self.state.borrow().editing.is_some() {
                        Propagation::Proceed
                    } else if self.state.borrow().actions_open {
                        self.run_selected_action();
                        Propagation::Stop
                    } else if self.state.borrow().voice.state() != voice::State::Idle {
                        self.toggle_voice();
                        Propagation::Stop
                    } else {
                        self.activate();
                        Propagation::Stop
                    }
                }
                Key::BackSpace if mods.contains(ModifierType::CONTROL_MASK) => {
                    if self.state.borrow().mode != Mode::Root {
                        self.enter_mode(Mode::Root);
                        Propagation::Stop
                    } else {
                        Propagation::Proceed
                    }
                }
                _ => Propagation::Proceed,
            },
        }
    }

    fn move_selection(&self, delta: i32) {
        if self.state.borrow().actions_open {
            self.move_action(delta);
            return;
        }
        let mut st = self.state.borrow_mut();
        if st.results.is_empty() {
            return;
        }
        let len = st.results.len() as i32;
        st.selected = ((st.selected as i32 + delta).rem_euclid(len)) as usize;
        paint_selection(&st);
        let selected = st.selected;
        let row = st.rows.get(selected).cloned();
        drop(st);
        if let Some(row) = row {
            scroll_row_into_view(&self.results_scroll, &self.results_host, &row);
        }
        self.update_preview();
        self.request_visible_thumbs();
    }

    fn activate(&self) {
        let item = {
            let st = self.state.borrow();
            let Some(scored) = st.results.get(st.selected) else {
                return;
            };
            scored.item.clone()
        };
        usage::bump(&item.id);
        if item.id == "cmd:apply-alias" {
            if let Action::Copy(id) = &item.action
                && let Some(name) = alias_typed(&self.entry.text())
            {
                self.apply_alias(id, &name);
            }
            return;
        }
        if self.handle_ui_action(&item.action) {
            return;
        }
        match item.action {
            Action::EnterMode(mode) => self.enter_mode(mode),
            Action::OpenPrefs { page } => self.open_prefs(page.as_deref()),
            Action::SaveSnippet { keyword } => {
                match clipboard::current_text() {
                    Some(text) if clipboard::looks_secret(&text) => {
                        self.set_status("Clipboard looks like a secret — not saved");
                    }
                    Some(text) => {
                        snippets::upsert(&keyword, &text);
                        self.set_status(format!("Saved snippet {keyword}"));
                    }
                    None => self.set_status("Clipboard is empty"),
                }
                self.enter_mode(Mode::Snippets);
            }
            Action::CreateNote { title } => {
                let note = notes::create(&title);
                self.open_note(&note.id);
            }
            Action::OpenNote { id } => self.open_note(&id),
            Action::AskAi { prompt } => self.run_ask(&prompt),
            Action::ToggleVoice => self.toggle_voice(),
            Action::SaveSettings => self.apply_setting(&item.id),
            Action::InstallExt { id } => self.install_ext(&id),
            Action::SyncScriptCommands => self.sync_scripts(),
            Action::SyncVicinae => self.sync_vicinae(),
            Action::UseModel {
                source,
                model,
                endpoint,
                api,
            } => {
                let msg = self
                    .state
                    .borrow()
                    .catalog
                    .settings
                    .borrow_mut()
                    .use_model(&source, &model, &endpoint, &api);
                self.set_status(msg);
                self.refresh();
            }
            Action::SignIn { provider } => {
                {
                    let st = self.state.borrow();
                    let mut settings = st.catalog.settings.borrow_mut();
                    auth::apply_provider_defaults(&mut settings, &provider);
                }
                self.sign_in(&provider);
            }
            Action::OpenUri(_) => self.hide_then(item.action),
            Action::ConnectorFetch { id } => {
                let settings = self.state.borrow().catalog.settings.borrow().clone();
                match crate::connectors::fetch(&id, &settings) {
                    Ok(items) => {
                        let scored: Vec<_> = items
                            .into_iter()
                            .enumerate()
                            .map(|(i, item)| {
                                catalog::Scored::new(item, 40_000u32.saturating_sub(i as u32))
                            })
                            .collect();
                        {
                            let mut st = self.state.borrow_mut();
                            st.results = scored;
                            st.selected = 0;
                        }
                        self.sync_chrome();
                        rebuild_rows(self);
                        self.set_status("Connected results");
                    }
                    Err(err) => self.set_status(err),
                }
            }
            Action::ImportGrok => match crate::xai::import_grok_cli() {
                Ok(msg) => {
                    {
                        let st = self.state.borrow();
                        let mut settings = st.catalog.settings.borrow_mut();
                        crate::xai::apply_defaults(&mut settings);
                    }
                    self.set_status(msg);
                    self.refresh();
                }
                Err(err) => self.set_status(err),
            },
            Action::SignOut => {
                let status = match auth::clear() {
                    Ok(()) => "Signed out and removed the stored OAuth credential".into(),
                    Err(error) => format!(
                        "Signed out locally, but the desktop keyring could not be cleared: {error}"
                    ),
                };
                self.set_status(status);
                self.refresh();
            }
            Action::RefreshModels => self.refresh_models(),
            Action::LaunchExtension { dir, command } => self.launch_extension(dir, command),
            Action::Extension { actions, .. } => self.run_extension_action(&actions, 0),
            Action::ExtensionConfirm { confirmed } => self.finish_confirm(confirmed),
            Action::ExtensionFormField {
                node,
                prop,
                kind,
                value,
                field_id,
            } => {
                if kind == "checkbox" {
                    let next = if value == "true" { "false" } else { "true" };
                    self.commit_form_field(node, &prop, &kind, &field_id, next.into());
                    self.refresh_extension();
                } else {
                    self.begin_form_edit(node, prop, kind, value, field_id);
                }
            }
            Action::SaveQuicklink { name, target } => {
                match quicklinks::create(&name, &target) {
                    Ok(link) => {
                        quicklinks::upsert(link);
                        self.set_status(format!("Saved quicklink {name}"));
                    }
                    Err(_) => {
                        self.set_status("Quicklink target must be a path or an http(s)/file URI")
                    }
                }
                self.enter_mode(Mode::Quicklink);
            }
            Action::Layout { name, address } => {
                self.hide_then(Action::Layout { name, address });
            }
            Action::SaveLayout { name } => match crate::layout::save_current(&name) {
                Some(_) => {
                    self.set_status(format!("Saved layout {name}"));
                    self.refresh();
                }
                None => self.set_status("Could not save layout"),
            },
            Action::QuitAll => {
                self.show_oneshot(crate::quit::confirm_item(), "Enter to confirm quit all");
            }
            Action::Capture { kind } => self.hide_then(Action::Capture { kind }),
            Action::Confetti => self.throw_confetti(),
            Action::Paste(text) => {
                let paste = if item.kind == Kind::Snippet {
                    let keyword = item.id.strip_prefix("snip:").unwrap_or("");
                    snippets::expand_keyword(
                        keyword,
                        &clipboard::current_text().unwrap_or_default(),
                    )
                    .unwrap_or(text)
                } else {
                    text
                };
                self.hide();
                action::run(&Action::Paste(paste));
            }
            Action::Copy(text) => {
                if item.kind == Kind::Calc {
                    calc::record(calc_expr(&item.subtitle), &text);
                }
                self.hide();
                if item.id.starts_with("gif:") {
                    action::copy_text(&text);
                    thread::spawn(move || {
                        let _ = crate::gif::copy_gif_bytes(&text);
                    });
                } else {
                    action::run(&Action::Copy(text));
                }
            }
            Action::RunScript { path } => {
                let allowed = self
                    .state
                    .borrow()
                    .catalog
                    .settings
                    .borrow()
                    .general
                    .allow_script_commands;
                if !allowed {
                    self.set_status(
                        "Unsigned script-commands are off. Enable them in Settings first.",
                    );
                    return;
                }
                self.hide();
                action::run(&Action::RunScript { path });
            }
            action => {
                self.hide();
                action::run(&action);
            }
        }
    }

    pub fn open_prefs(&self, page: Option<&str>) {
        self.hide();
        if let Some(host) = self.prefs.borrow().as_ref() {
            host.show(page);
            return;
        }
        let Some(app) = self.window.application() else {
            self.set_status("Could not open Settings");
            return;
        };
        let settings = self.state.borrow().catalog.settings.clone();
        let shell = self.clone();
        let host = prefs::open(
            &app,
            settings,
            Rc::new(move |event| shell.handle_prefs(event)),
        );
        host.show(page);
        *self.prefs.borrow_mut() = Some(host);
    }

    fn handle_prefs(&self, event: prefs::Event) {
        match event {
            prefs::Event::SignIn(provider) => {
                {
                    let st = self.state.borrow();
                    let mut settings = st.catalog.settings.borrow_mut();
                    auth::apply_provider_defaults(&mut settings, &provider);
                }
                self.sign_in(&provider);
            }
            prefs::Event::ImportGrok => match crate::xai::import_grok_cli() {
                Ok(msg) => {
                    {
                        let st = self.state.borrow();
                        let mut settings = st.catalog.settings.borrow_mut();
                        crate::xai::apply_defaults(&mut settings);
                    }
                    self.prefs_status(&msg);
                    self.refresh_prefs();
                }
                Err(err) => self.prefs_status(&err),
            },
            prefs::Event::SignOut => {
                let status = match auth::clear() {
                    Ok(()) => "Signed out".into(),
                    Err(error) => format!("Signed out locally: {error}"),
                };
                self.prefs_status(&status);
                self.refresh_prefs();
            }
            prefs::Event::RefreshModels => self.refresh_models(),
            prefs::Event::OpenStore => {
                self.open(Mode::Store);
            }
            prefs::Event::SyncVicinae => self.sync_vicinae(),
            prefs::Event::SyncScripts => self.sync_scripts(),
            prefs::Event::OpenPath(path) => {
                action::run(&Action::OpenPath(path));
            }
        }
    }

    fn prefs_status(&self, text: &str) {
        if let Some(host) = self.prefs.borrow().as_ref() {
            host.set_status(text);
        }
        self.set_status(text);
    }

    fn refresh_prefs(&self) {
        if let Some(host) = self.prefs.borrow().as_ref() {
            host.rebuild();
        }
    }

    fn enter_mode(&self, mode: Mode) {
        self.commit_editing();
        self.close_actions_inner(false);
        if mode != Mode::Extension {
            self.end_extension();
        }
        self.state.borrow_mut().editing = None;
        self.state.borrow_mut().pending_alias = None;
        self.state.borrow_mut().status.clear();
        self.entry.set_text(mode.prefix());
        self.entry.set_position(-1);
        self.entry.grab_focus();
        self.refresh();
    }

    fn open_note(&self, id: &str) {
        let Some(note) = notes::get(id) else {
            return;
        };
        self.state.borrow_mut().editing = Some(Editing::Note(note.id.clone()));
        self.state.borrow_mut().mode = Mode::Notes;
        self.entry.set_text(&note.title);
        self.entry.set_position(-1);
        self.detail_view.buffer().set_text(&note.body);
        self.set_status("Editing note · Ctrl+S saves, Esc returns");
        self.sync_chrome();
        self.detail_view.grab_focus();
    }

    fn commit_editing(&self) {
        let editing = self.state.borrow().editing.clone();
        let Some(editing) = editing else {
            return;
        };
        match editing {
            Editing::Note(id) => {
                let title = self.entry.text().to_string();
                let body = buffer_text(&self.detail_view);
                notes::save_body(&id, &title, &body);
            }
            Editing::ClipRename(id) => {
                let label = self.entry.text().to_string();
                let clips = self.clip_store();
                let mut store = clips.borrow_mut();
                if store.rename(&id, &label) {
                    store.persist();
                }
            }
            Editing::ClipEdit(id) => {
                let text = buffer_text(&self.detail_view);
                let clips = self.clip_store();
                let mut store = clips.borrow_mut();
                if store.edit(&id, &text) {
                    store.persist();
                } else {
                    drop(store);
                    self.set_status("Clipboard edit rejected (empty or looks like a secret)");
                }
            }
            Editing::FormField {
                node,
                prop,
                kind,
                field_id,
            } => {
                let value = if kind == "textarea" {
                    buffer_text(&self.detail_view)
                } else {
                    self.entry.text().to_string()
                };
                self.commit_form_field(node, &prop, &kind, &field_id, value);
            }
        }
        self.state.borrow_mut().editing = None;
        self.detail.set_visible(false);
    }

    fn clip_store(&self) -> Rc<RefCell<clipboard::Store>> {
        self.state.borrow().catalog.clips.clone()
    }

    fn apply_alias(&self, id: &str, name: &str) {
        let mut store = alias::Store::load();
        store.set(id, name);
        store.persist();
        self.state.borrow_mut().pending_alias = None;
        self.set_status(format!("Alias “{name}” set"));
        self.refresh();
    }

    fn toggle_actions(&self) {
        if self.state.borrow().actions_open {
            self.close_actions();
            return;
        }
        if self.state.borrow().editing.is_some() {
            return;
        }
        let item = {
            let st = self.state.borrow();
            st.results.get(st.selected).map(|row| row.item.clone())
        };
        let Some(item) = item else {
            self.set_status("Nothing selected");
            return;
        };
        let saved = self.entry.text().to_string();
        {
            let mut st = self.state.borrow_mut();
            st.saved_query = saved;
            st.actions_open = true;
            st.action_selected = 0;
        }
        let actions = {
            let st = self.state.borrow();
            panel_actions(&item, &st)
        };
        self.state.borrow_mut().actions = actions;
        self.entry.set_placeholder_text(Some("Filter actions…"));
        self.entry.set_text("");
        self.entry.grab_focus();
        self.paint_actions();
        self.action_panel.set_visible(true);
    }

    fn close_actions(&self) {
        self.close_actions_inner(true);
    }

    fn close_actions_inner(&self, restore: bool) {
        let saved = {
            let mut st = self.state.borrow_mut();
            if !st.actions_open {
                return;
            }
            st.actions_open = false;
            st.actions.clear();
            st.action_selected = 0;
            std::mem::take(&mut st.saved_query)
        };
        self.action_panel.set_visible(false);
        while let Some(child) = self.action_list.last_child() {
            self.action_list.remove(&child);
        }
        if restore {
            self.entry.set_text(&saved);
            self.entry.set_position(-1);
            self.entry.grab_focus();
            self.refresh();
        }
    }

    fn refresh_actions(&self) {
        let filter = self.entry.text().to_string();
        let item = {
            let st = self.state.borrow();
            st.results.get(st.selected).map(|row| row.item.clone())
        };
        let Some(item) = item else {
            return;
        };
        let mut actions = {
            let st = self.state.borrow();
            panel_actions(&item, &st)
        };
        let f = filter.trim().to_ascii_lowercase();
        if !f.is_empty() {
            actions.retain(|action| {
                action.title.to_ascii_lowercase().contains(&f)
                    || action.keywords.to_ascii_lowercase().contains(&f)
            });
        }
        {
            let mut st = self.state.borrow_mut();
            st.actions = actions;
            st.action_selected = 0;
        }
        self.paint_actions();
    }

    fn paint_actions(&self) {
        while let Some(child) = self.action_list.last_child() {
            self.action_list.remove(&child);
        }
        let (actions, selected) = {
            let st = self.state.borrow();
            (st.actions.clone(), st.action_selected)
        };
        if actions.is_empty() {
            let empty = Label::new(Some("No actions"));
            empty.add_css_class("hint");
            empty.set_xalign(0.0);
            self.action_list.append(&empty);
            self.action_panel.set_visible(true);
            return;
        }
        for (idx, action) in actions.iter().enumerate() {
            let row = Box::new(Orientation::Horizontal, 8);
            row.add_css_class("action-row");
            if idx == selected {
                row.add_css_class("selected");
            }
            let title = Label::new(Some(&action.title));
            title.add_css_class("action-row-title");
            title.set_xalign(0.0);
            title.set_hexpand(true);
            row.append(&title);
            self.action_list.append(&row);
        }
        self.action_panel.set_visible(true);
    }

    fn move_action(&self, delta: i32) {
        {
            let mut st = self.state.borrow_mut();
            if st.actions.is_empty() {
                return;
            }
            let len = st.actions.len() as i32;
            st.action_selected = ((st.action_selected as i32 + delta).rem_euclid(len)) as usize;
        }
        self.paint_actions();
    }

    fn run_selected_action(&self) {
        let action = {
            let st = self.state.borrow();
            st.actions.get(st.action_selected).cloned()
        };
        let Some(action) = action else {
            return;
        };
        let item = {
            let st = self.state.borrow();
            st.results.get(st.selected).map(|row| row.item.clone())
        };
        let Some(item) = item else {
            return;
        };
        let filter = self.entry.text().to_string();
        match action.kind {
            PanelKind::ToggleFavorite => {
                let mut store = favorites::Store::load();
                let pinned = store.toggle(&item.id);
                store.persist();
                self.close_actions();
                self.set_status(if pinned {
                    format!("Pinned {}", item.title)
                } else {
                    format!("Unpinned {}", item.title)
                });
                self.refresh();
            }
            PanelKind::SetAlias => {
                let typed = filter.trim();
                if !typed.is_empty() && !typed.eq_ignore_ascii_case("set alias") {
                    self.close_actions();
                    self.apply_alias(&item.id, typed);
                } else {
                    self.state.borrow_mut().pending_alias = Some(item.id.clone());
                    self.close_actions();
                    self.entry.set_text("alias:");
                    self.entry.set_position(-1);
                    self.set_status(format!("Type an alias for {}", item.title));
                }
            }
            PanelKind::Copy(text) => {
                action::copy_text(&text);
                self.close_actions();
                self.set_status("Copied");
            }
            PanelKind::CopyPath(path) => {
                action::copy_text(&path.display().to_string());
                self.close_actions();
                self.set_status("Copied path");
            }
            PanelKind::Open => {
                self.close_actions();
                self.hide();
                action::run(&item.action);
            }
            PanelKind::OpenWith { app, path } => {
                self.close_actions();
                self.hide();
                let file = gtk4::gio::File::for_path(path);
                let _ = app.launch(&[file], gtk4::gio::AppLaunchContext::NONE);
            }
            PanelKind::ShowInFiles(path) => {
                self.close_actions();
                self.hide();
                action::run(&Action::OpenPath(path));
            }
            PanelKind::Paste(text) => {
                self.close_actions();
                self.hide();
                action::run(&Action::Paste(text));
            }
            PanelKind::ClipPin(id) => {
                {
                    let clips = self.clip_store();
                    let mut store = clips.borrow_mut();
                    store.pin(&id);
                    store.persist();
                }
                self.close_actions();
                self.set_status("Pinned clipboard entry");
                self.refresh();
            }
            PanelKind::ClipUnpin(id) => {
                {
                    let clips = self.clip_store();
                    let mut store = clips.borrow_mut();
                    store.unpin(&id);
                    store.persist();
                }
                self.close_actions();
                self.set_status("Unpinned clipboard entry");
                self.refresh();
            }
            PanelKind::ClipRename(id) => {
                self.close_actions();
                self.begin_clip_rename(&id);
            }
            PanelKind::ClipEdit(id) => {
                self.close_actions();
                self.begin_clip_edit(&id);
            }
            PanelKind::Launch => {
                self.close_actions();
                if self.handle_ui_action(&item.action) {
                    return;
                }
                self.hide();
                action::run(&item.action);
            }
            PanelKind::SnippetPaste(keyword) => {
                let clip = clipboard::current_text().unwrap_or_default();
                let text = snippets::expand_keyword(&keyword, &clip).unwrap_or_default();
                self.close_actions();
                self.hide();
                action::run(&Action::Paste(text));
            }
            PanelKind::SnippetCopy(keyword) => {
                let clip = clipboard::current_text().unwrap_or_default();
                let text = snippets::expand_keyword(&keyword, &clip).unwrap_or_default();
                action::copy_text(&text);
                self.close_actions();
                self.set_status("Copied expanded snippet");
            }
            PanelKind::Run(action) => {
                self.close_actions();
                self.dispatch_action(action);
            }
            PanelKind::Confirm(item) => {
                self.close_actions_inner(false);
                self.show_oneshot(item, "Enter to confirm");
            }
        }
    }

    fn show_oneshot(&self, item: Item, status: &str) {
        {
            let mut st = self.state.borrow_mut();
            st.search_gen = st.search_gen.saturating_add(1);
            st.results = vec![Scored::new(item, 200_000)];
            st.selected = 0;
        }
        self.set_status(status);
        self.sync_chrome();
        rebuild_rows(self);
    }

    fn hide_then(&self, action: Action) {
        self.hide();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            action::run(&action);
            gtk4::glib::ControlFlow::Break
        });
    }

    fn dispatch_action(&self, action: Action) {
        if self.handle_ui_action(&action) {
            return;
        }
        match action {
            Action::Layout { .. } | Action::Capture { .. } | Action::OpenUri(_) => {
                self.hide_then(action)
            }
            Action::QuitAll => {
                self.show_oneshot(crate::quit::confirm_item(), "Enter to confirm quit all");
            }
            Action::SaveLayout { name } => match crate::layout::save_current(&name) {
                Some(_) => self.set_status(format!("Saved layout {name}")),
                None => self.set_status("Could not save layout"),
            },
            other => {
                self.hide();
                action::run(&other);
            }
        }
    }

    fn begin_clip_rename(&self, id: &str) {
        let Some(entry) = self.clip_store().borrow().get(id).cloned() else {
            return;
        };
        self.state.borrow_mut().editing = Some(Editing::ClipRename(entry.id.clone()));
        self.state.borrow_mut().mode = Mode::Clipboard;
        self.entry.set_text(&entry.label);
        self.entry.set_position(-1);
        self.detail.set_visible(false);
        self.set_status("Editing clipboard · Ctrl+S saves");
        self.sync_chrome();
        self.entry.grab_focus();
    }

    fn begin_clip_edit(&self, id: &str) {
        let Some(entry) = self.clip_store().borrow().get(id).cloned() else {
            return;
        };
        self.state.borrow_mut().editing = Some(Editing::ClipEdit(entry.id.clone()));
        self.state.borrow_mut().mode = Mode::Clipboard;
        self.entry.set_text(&entry.display_title());
        self.detail_view.buffer().set_text(&entry.text);
        self.set_status("Editing clipboard · Ctrl+S saves");
        self.sync_chrome();
        self.detail_view.grab_focus();
    }

    fn throw_confetti(&self) {
        let width = self.window.width().max(1) as f64;
        let height = self.window.height().max(1) as f64;
        *self.confetti_bits.borrow_mut() = spawn_particles(width, height);
        self.confetti.set_visible(true);
        self.confetti.queue_draw();
        let bits = self.confetti_bits.clone();
        let area = self.confetti.clone();
        let frames = Rc::new(RefCell::new(0u32));
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let n = {
                let mut n = frames.borrow_mut();
                *n += 1;
                *n
            };
            tick_particles(&mut bits.borrow_mut());
            area.queue_draw();
            if n >= 75 {
                area.set_visible(false);
                bits.borrow_mut().clear();
                gtk4::glib::ControlFlow::Break
            } else {
                gtk4::glib::ControlFlow::Continue
            }
        });
    }

    fn run_ask(&self, prompt: &str) {
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        let typed = prompt.to_string();
        let mut prompt = typed.clone();
        if settings.general.attach_clipboard_to_ai
            && let Some(clip) = clipboard::current_text()
        {
            if clipboard::looks_secret(&clip) {
                prompt = format!("{prompt}\n\nClipboard: [redacted — looked like a secret]");
            } else {
                prompt = format!("{prompt}\n\nClipboard:\n{clip}");
            }
        }
        let extras = ai::peek_attachments();
        if !extras.is_empty() {
            prompt = ai::compose_user(&prompt, &extras);
        }
        let chat = ai::ensure_thread(&typed);
        let history = ai::history(&chat.id);
        self.set_status(format!("Thinking with {}…", settings.ai.model));
        let (tx, rx) = std::sync::mpsc::channel();
        let send_prompt = prompt.clone();
        thread::spawn(move || {
            let _ = tx.send(if history.is_empty() {
                ai::ask(&send_prompt, &settings)
            } else {
                ai::chat(&settings, &history, &send_prompt)
            });
        });
        let shell = self.clone();
        let thread_id = chat.id;
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
            match rx.try_recv() {
                Ok(Ok(reply)) => {
                    let _ = ai::take_attachments();
                    ai::append_turn(&thread_id, "user", &prompt);
                    ai::append_turn(&thread_id, "assistant", &reply.text);
                    shell.set_status(reply.source.clone());
                    shell.show_ai_reply(reply.text, reply.source);
                    gtk4::glib::ControlFlow::Break
                }
                Ok(Err(err)) => {
                    shell.set_status(err);
                    gtk4::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    shell.set_status("Ask AI stopped unexpectedly");
                    gtk4::glib::ControlFlow::Break
                }
            }
        });
    }

    fn show_ai_reply(&self, text: String, source: String) {
        let item = Item {
            id: "ask:reply".into(),
            title: text
                .lines()
                .next()
                .unwrap_or("Answer")
                .chars()
                .take(72)
                .collect(),
            subtitle: format!("{source} · Enter copies"),
            keywords: text.clone(),
            kind: Kind::Ai,
            icon: Icon::Name("help-faq".into()),
            action: Action::Copy(text.clone()),
        };
        {
            let mut st = self.state.borrow_mut();
            st.results = vec![Scored::new(item, 100_000)];
            st.selected = 0;
            st.mode = Mode::Ask;
        }
        self.detail_view.buffer().set_text(&text);
        self.detail.set_visible(true);
        self.sync_chrome();
        rebuild_rows(self);
    }

    fn handle_ui_action(&self, action: &Action) -> bool {
        match action {
            Action::DictateFocused => {
                self.start_focused_dictation();
                true
            }
            Action::NoteFromSelection => {
                self.note_from_selection();
                true
            }
            Action::StartFocus { seconds, label } => {
                self.begin_focus(*seconds, label);
                true
            }
            Action::StopFocus => {
                self.stop_focus();
                true
            }
            Action::TypeText(text) => {
                self.paste_with_wtype(text);
                true
            }
            Action::AskSelection { template } => {
                match ai::expand_selection_template(template) {
                    Some(prompt) => self.run_ask(&prompt),
                    None => self.set_status("No selected text (primary selection is empty)"),
                }
                true
            }
            Action::ResumeThread { id } => {
                match ai::resume(id) {
                    Some(thread) => {
                        self.enter_mode(Mode::Ask);
                        self.set_status(format!("Resumed “{}”. Type a follow-up.", thread.title));
                    }
                    None => self.set_status("That chat is gone"),
                }
                true
            }
            Action::NewChat => {
                ai::new_chat();
                self.enter_mode(Mode::Ask);
                self.set_status("New chat");
                true
            }
            Action::Remember { text } => {
                match crate::memory::remember(text) {
                    Some(_) => self.set_status("Remembered"),
                    None => self.set_status("Nothing to remember"),
                }
                self.refresh();
                true
            }
            Action::ForgetMemory { query } => {
                let n = crate::memory::forget(query);
                if n == 0 {
                    self.set_status("Nothing matched");
                } else {
                    self.set_status(format!("Forgot {n}"));
                }
                self.refresh();
                true
            }
            Action::ShowMemory => {
                let items = crate::memory::list_items();
                if items.is_empty() {
                    self.set_status("No memories yet. Type remember …");
                } else {
                    {
                        let mut st = self.state.borrow_mut();
                        st.results = items
                            .into_iter()
                            .map(|item| Scored::new(item, 50_000))
                            .collect();
                        st.selected = 0;
                    }
                    self.set_status("Memory · Enter copies · Forget … to delete");
                    self.sync_chrome();
                    rebuild_rows(self);
                }
                true
            }
            Action::AttachClipboard => {
                self.after_attach(ai::attach_clipboard());
                true
            }
            Action::AttachSelected => {
                let selected = {
                    let st = self.state.borrow();
                    st.results.get(st.selected).map(|row| row.item.clone())
                };
                if let Some(item) = selected.as_ref()
                    && let Some(path) = item_path(item)
                    && item.id != "cmd:attach-file"
                {
                    self.after_attach(ai::attach_path(&path));
                } else if let Some(path) = path_from_clipboard() {
                    self.after_attach(ai::attach_path(&path));
                } else {
                    self.set_status("Select a file (Ctrl+K → Attach) or copy a path");
                }
                true
            }
            Action::AttachPath { path } => {
                self.after_attach(ai::attach_path(path));
                true
            }
            Action::ShareRegion => {
                self.share_capture(true);
                true
            }
            Action::ShareScreen => {
                self.share_capture(false);
                true
            }
            _ => false,
        }
    }

    fn after_attach(&self, result: Result<String, String>) {
        match result {
            Ok(msg) => {
                if self.state.borrow().mode != Mode::Ask {
                    self.enter_mode(Mode::Ask);
                }
                self.set_status(msg);
            }
            Err(err) => self.set_status(err),
        }
    }

    fn share_capture(&self, region: bool) {
        self.hide();
        let shell = self.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            let captured = crate::capture::shot_to_path(region);
            shell.state.borrow_mut().visible = true;
            shell.show_launcher();
            match captured {
                Some(path) => {
                    let ocr = crate::ocr::read_text(&path);
                    let result = ai::attach(ai::Attachment::Image { path, ocr });
                    shell.enter_mode(Mode::Ask);
                    shell.after_attach(result);
                }
                None => {
                    shell.restore_with_status("Capture cancelled".into());
                }
            }
            gtk4::glib::ControlFlow::Break
        });
    }

    fn start_focused_dictation(&self) {
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        match self.state.borrow().voice.state() {
            voice::State::Listening
                if self.state.borrow().voice.dest() == voice::Dest::FocusedApp =>
            {
                self.finish_global_dictation();
            }
            voice::State::Listening => {
                self.state.borrow().voice.set_dest(voice::Dest::FocusedApp);
                self.set_status("Listening… Alt+Space pastes into the focused app");
                self.hide_keep_voice();
            }
            voice::State::Transcribing => self.set_status("Still transcribing…"),
            voice::State::Idle => {
                let started = self
                    .state
                    .borrow()
                    .voice
                    .start_for(&settings, voice::Dest::FocusedApp);
                match started {
                    Ok(()) => {
                        self.set_status("Listening… Alt+Space pastes into the focused app");
                        self.hide_keep_voice();
                    }
                    Err(err) => self.set_status(err),
                }
            }
        }
    }

    fn finish_global_dictation(&self) {
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        let voice = self.state.borrow().voice.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(voice.stop(&settings));
        });
        let shell = self.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
            match rx.try_recv() {
                Ok(Ok(text)) => {
                    shell.deliver_focused_transcript(text);
                    gtk4::glib::ControlFlow::Break
                }
                Ok(Err(err)) => {
                    shell.restore_with_status(err);
                    gtk4::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    shell.restore_with_status("Dictation stopped unexpectedly".into());
                    gtk4::glib::ControlFlow::Break
                }
            }
        });
    }

    fn deliver_focused_transcript(&self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.restore_with_status("No speech detected".into());
            return;
        }
        crate::voice::remember(&text);
        if action::wtype_available() {
            let _ = action::type_text(&text);
            self.set_status("Dictation pasted");
        } else {
            action::copy_text(&text);
            self.restore_with_status("install wtype to paste into the focused app".into());
        }
    }

    fn restore_with_status(&self, status: String) {
        self.state.borrow_mut().visible = true;
        self.show_launcher();
        self.entry.grab_focus();
        self.set_status(status);
        self.refresh();
    }

    fn paste_with_wtype(&self, text: &str) {
        if action::wtype_available() {
            let text = text.to_string();
            self.hide();
            gtk4::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                let _ = action::type_text(&text);
                gtk4::glib::ControlFlow::Break
            });
        } else {
            action::copy_text(text);
            self.set_status("install wtype to paste into the focused app");
        }
    }

    fn note_from_selection(&self) {
        match clipboard::selection_or_clipboard() {
            Some(text) if !text.trim().is_empty() => {
                let note = notes::from_text(&text);
                self.open_note(&note.id);
            }
            _ => self.set_status("No selected text (primary selection is empty)"),
        }
    }

    fn begin_focus(&self, seconds: u32, label: &str) {
        crate::focus::start(seconds, label);
        self.arm_focus_ticks();
        let text = crate::focus::status_line().unwrap_or_else(|| "Focus started".into());
        self.set_status(text);
        self.refresh();
    }

    fn stop_focus(&self) {
        {
            let mut st = self.state.borrow_mut();
            st.focus_gen = st.focus_gen.saturating_add(1);
        }
        crate::focus::stop();
        self.set_status("Focus stopped");
        self.refresh();
    }

    fn arm_focus_ticks(&self) {
        let tick_gen = {
            let mut st = self.state.borrow_mut();
            st.focus_gen = st.focus_gen.saturating_add(1);
            st.focus_gen
        };
        let shell = self.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            if shell.state.borrow().focus_gen != tick_gen {
                return gtk4::glib::ControlFlow::Break;
            }
            match crate::focus::tick() {
                crate::focus::Tick::Running { text } => {
                    if shell.state.borrow().visible {
                        shell.set_status(text);
                    }
                    gtk4::glib::ControlFlow::Continue
                }
                crate::focus::Tick::Done { text } => {
                    if shell.state.borrow().visible {
                        shell.set_status(text);
                        shell.refresh();
                    }
                    gtk4::glib::ControlFlow::Break
                }
            }
        });
    }

    fn toggle_voice(&self) {
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        let dest = self.state.borrow().voice.dest();
        let state = self.state.borrow().voice.state();
        match state {
            voice::State::Listening if dest == voice::Dest::FocusedApp => {
                self.finish_global_dictation();
            }
            voice::State::Listening => {
                self.set_status("Transcribing…");
                let voice = self.state.borrow().voice.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                thread::spawn(move || {
                    let _ = tx.send(voice.stop(&settings));
                });
                let shell = self.clone();
                gtk4::glib::timeout_add_local(
                    std::time::Duration::from_millis(40),
                    move || match rx.try_recv() {
                        Ok(Ok(text)) => {
                            shell.apply_transcript(text);
                            gtk4::glib::ControlFlow::Break
                        }
                        Ok(Err(err)) => {
                            shell.set_status(err);
                            gtk4::glib::ControlFlow::Break
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            gtk4::glib::ControlFlow::Continue
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            shell.set_status("Dictation stopped unexpectedly");
                            gtk4::glib::ControlFlow::Break
                        }
                    },
                );
            }
            voice::State::Transcribing => self.set_status("Still transcribing…"),
            voice::State::Idle => {
                // Bind first: a `match self.state.borrow()...` keeps that Ref
                // alive through the arms, and set_status() borrow_mut() panics.
                let started = self.state.borrow().voice.start(&settings);
                match started {
                    Ok(()) => {
                        self.set_status("Listening… Enter fills the search box");
                        self.window.present();
                        self.entry.grab_focus();
                    }
                    Err(err) => self.set_status(err),
                }
            }
        }
    }

    fn apply_transcript(&self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.set_status("No speech detected");
            return;
        }
        crate::voice::remember(&text);
        let current = self.entry.text().to_string();
        let filled = if current.trim_start().starts_with('?')
            || self.state.borrow().mode == Mode::Ask
            || self.state.borrow().mode == Mode::Voice
        {
            format!("? {text}")
        } else {
            text.clone()
        };
        self.entry.set_text(&filled);
        self.entry.set_position(-1);
        self.entry.grab_focus();
        self.set_status("Dictation captured · edit, then Enter · Ctrl+K to paste with wtype");
        self.refresh();
    }

    fn sign_in(&self, provider: &str) {
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        match auth::start_login(provider, &settings) {
            Ok((job, pending)) => {
                action::copy_text(&job.url);
                let opened = auth::open_browser(&job.url);
                let status = match &opened {
                    Ok(()) => job.message.clone(),
                    Err(err) => format!("{err} · URL copied to the clipboard"),
                };
                self.show_signin_progress(&job, &status);
                let (tx, rx) = std::sync::mpsc::channel();
                thread::spawn(move || {
                    let _ = tx.send(auth::finish_login(pending));
                });
                let shell = self.clone();
                gtk4::glib::timeout_add_local(
                    std::time::Duration::from_millis(80),
                    move || match rx.try_recv() {
                        Ok(Ok(msg)) => {
                            shell.prefs_status(&msg);
                            shell.refresh();
                            shell.refresh_prefs();
                            gtk4::glib::ControlFlow::Break
                        }
                        Ok(Err(err)) => {
                            shell.set_status(err);
                            gtk4::glib::ControlFlow::Break
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            gtk4::glib::ControlFlow::Continue
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            shell.set_status("Sign-in stopped");
                            gtk4::glib::ControlFlow::Break
                        }
                    },
                );
            }
            Err(err) => {
                self.show_oneshot(
                    Item {
                        id: "auth:error".into(),
                        title: err.clone(),
                        subtitle: "Sign-in did not start — nothing was sent to a browser".into(),
                        keywords: "oauth signin".into(),
                        kind: Kind::Settings,
                        icon: Icon::Name("dialog-error".into()),
                        action: Action::Copy(err.clone()),
                    },
                    &err,
                );
            }
        }
    }

    fn show_signin_progress(&self, job: &auth::BrowserJob, status: &str) {
        let mut items = Vec::new();
        if let Some(code) = &job.user_code {
            items.push(Item {
                id: "auth:code".into(),
                title: format!("Confirm this code: {code}"),
                subtitle: "xAI shows the same code in the browser".into(),
                keywords: "oauth grok xai".into(),
                kind: Kind::Ai,
                icon: Icon::Name("dialog-password".into()),
                action: Action::Copy(code.clone()),
            });
        }
        items.push(Item {
            id: "auth:open".into(),
            title: "Open the sign-in page again".into(),
            subtitle: job.url.clone(),
            keywords: "oauth browser".into(),
            kind: Kind::Web,
            icon: Icon::Name("web-browser".into()),
            action: Action::OpenUri(job.url.clone()),
        });
        {
            let mut st = self.state.borrow_mut();
            st.search_gen = st.search_gen.saturating_add(1);
            st.results = items
                .into_iter()
                .map(|item| catalog::Scored::new(item, 200_000))
                .collect();
            st.selected = 0;
        }
        self.set_status(status);
        self.sync_chrome();
        rebuild_rows(self);
    }

    fn refresh_models(&self) {
        self.set_status("Scanning Ollama, LM Studio, llama.cpp…");
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            models::warm();
            let n = models::discover().len();
            let _ = tx.send(n);
        });
        let shell = self.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(60), move || {
            match rx.try_recv() {
                Ok(n) => {
                    shell.set_status(if n == 0 {
                        "No local models responded. Start Ollama or LM Studio, then scan again."
                            .into()
                    } else {
                        format!("{n} local models ready")
                    });
                    shell.refresh();
                    gtk4::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    shell.set_status("Model scan stopped");
                    gtk4::glib::ControlFlow::Break
                }
            }
        });
    }

    fn apply_setting(&self, id: &str) {
        let typed = self
            .entry
            .text()
            .trim_start()
            .trim_start_matches("set")
            .trim_start_matches("settings")
            .trim()
            .to_string();
        let msg = self
            .state
            .borrow()
            .catalog
            .settings
            .borrow_mut()
            .apply(id, &typed);
        self.set_status(msg);
        self.refresh();
    }

    fn install_ext(&self, id: &str) {
        let msg = {
            let st = self.state.borrow();
            let mut settings = st.catalog.settings.borrow_mut();
            match store::install(id, &mut settings) {
                Ok(msg) => msg,
                Err(err) => err,
            }
        };
        self.set_status(msg);
        self.state.borrow().catalog.reload_installed();
        self.refresh();
    }

    fn sync_vicinae(&self) {
        self.set_status("Refreshing Vicinae catalog…");
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(store::sync_vicinae());
        });
        let shell = self.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
            match rx.try_recv() {
                Ok(Ok(msg)) => {
                    shell.set_status(msg);
                    shell.refresh();
                    gtk4::glib::ControlFlow::Break
                }
                Ok(Err(err)) => {
                    shell.set_status(err);
                    gtk4::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    shell.set_status("Vicinae sync stopped");
                    gtk4::glib::ControlFlow::Break
                }
            }
        });
    }

    fn sync_scripts(&self) {
        self.set_status("Syncing Raycast Script Commands…");
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(store::sync_script_commands(&settings));
        });
        let shell = self.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
            match rx.try_recv() {
                Ok(Ok(msg)) => {
                    shell.set_status(msg);
                    shell.refresh();
                    gtk4::glib::ControlFlow::Break
                }
                Ok(Err(err)) => {
                    shell.set_status(err);
                    gtk4::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    shell.set_status("Store sync stopped");
                    gtk4::glib::ControlFlow::Break
                }
            }
        });
    }
}

thread_local! {
    static SHELL: RefCell<Option<Shell>> = const { RefCell::new(None) };
}

enum UiMsg {
    Live {
        generation: u64,
        extras: std::boxed::Box<LiveExtras>,
    },
    Thumb(crate::preview::DecodedImage),
    Caption {
        path: PathBuf,
        text: String,
    },
    ThumbMiss(PathBuf),
    Windows(Vec<Item>),
}

fn bind_shell(shell: &Shell) {
    SHELL.with(|slot| *slot.borrow_mut() = Some(shell.clone()));
}

fn push_ui(inbox: &Arc<Mutex<Vec<UiMsg>>>, msg: UiMsg) {
    inbox.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
    let inbox = inbox.clone();
    let _ = gtk4::glib::idle_add(move || {
        let batch = {
            let mut guard = inbox.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *guard)
        };
        SHELL.with(|slot| {
            if let Some(shell) = slot.borrow().as_ref() {
                for msg in batch {
                    match msg {
                        UiMsg::Live { generation, extras } => {
                            if shell.state.borrow().search_gen == generation {
                                shell.apply_live(*extras);
                            }
                        }
                        UiMsg::Thumb(decoded) => {
                            let path = decoded.path.clone();
                            if let Some(caption) = decoded.caption.clone() {
                                shell.apply_caption(path.clone(), caption);
                            }
                            let texture = Texture::for_pixbuf(&decoded.to_pixbuf());
                            shell.apply_thumb(path, texture);
                        }
                        UiMsg::Caption { path, text } => shell.apply_caption(path, text),
                        UiMsg::ThumbMiss(path) => {
                            shell.state.borrow_mut().thumb_miss.insert(path);
                        }
                        UiMsg::Windows(windows) => shell.apply_hypr_windows(windows),
                    }
                }
            }
        });
        gtk4::glib::ControlFlow::Break
    });
}

fn start_live_worker(rx: mpsc::Receiver<LiveJob>, cancel: Arc<files::Cancel>) {
    let inbox: Arc<Mutex<Vec<UiMsg>>> = Arc::new(Mutex::new(Vec::new()));
    thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            while let Ok(next) = rx.try_recv() {
                job = next;
            }
            cancel.reset();
            let extras = files::with_cancel(cancel.clone(), || {
                catalog::live_extras(&job.query, job.mode, &job.settings, &job.usage)
            });
            if cancel.is_cancelled() {
                continue;
            }
            push_ui(
                &inbox,
                UiMsg::Live {
                    generation: job.generation,
                    extras: std::boxed::Box::new(extras),
                },
            );
        }
    });
}

/// Selected row plus rows overlapping the viewport. If rows are not laid out yet,
/// fall back to a small window around the selection instead of every result.
fn thumb_rows_in_view(
    count: usize,
    selected: usize,
    view_top: f64,
    view_bottom: f64,
    row_bounds: &[Option<(f64, f64)>],
) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let selected = selected.min(count - 1);
    let laid_out = row_bounds.iter().any(Option::is_some);
    if !laid_out {
        const FALLBACK: usize = 16;
        let start = selected.saturating_sub(FALLBACK);
        let end = (selected + FALLBACK + 1).min(count);
        return (start..end).collect();
    }
    let mut out = Vec::new();
    for (idx, bounds) in row_bounds.iter().take(count).enumerate() {
        if idx == selected {
            out.push(idx);
            continue;
        }
        if let Some((y, height)) = *bounds {
            let bottom = y + height;
            if bottom >= view_top && y <= view_bottom {
                out.push(idx);
            }
        }
    }
    out
}

fn unique_thumb_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for path in paths.into_iter().rev() {
        if seen.insert(path.clone()) {
            unique.push(path);
        }
    }
    unique.reverse();
    unique
}

/// Walk a drained batch until a newer request cancels it. Do not reset per path.
fn for_each_live_thumb(
    paths: Vec<PathBuf>,
    cancel: &files::Cancel,
    mut f: impl FnMut(&Path) -> bool,
) {
    for path in unique_thumb_paths(paths) {
        if cancel.is_cancelled() {
            break;
        }
        if !f(&path) {
            break;
        }
    }
}

fn start_thumb_worker(rx: mpsc::Receiver<PathBuf>, cancel: Arc<files::Cancel>) {
    let inbox: Arc<Mutex<Vec<UiMsg>>> = Arc::new(Mutex::new(Vec::new()));
    thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut paths = vec![first];
            while let Ok(next) = rx.try_recv() {
                paths.push(next);
            }
            cancel.reset();
            for_each_live_thumb(paths, &cancel, |path| {
                let produced =
                    files::with_cancel(cancel.clone(), || crate::preview::thumbnail(path));
                if cancel.is_cancelled() {
                    return false;
                }
                let Some(produced) = produced else {
                    return true;
                };
                if let Some(image) = produced.image {
                    push_ui(&inbox, UiMsg::Thumb(image));
                } else {
                    push_ui(&inbox, UiMsg::ThumbMiss(path.to_path_buf()));
                }
                let summary = produced.info.summary();
                if !summary.is_empty() {
                    push_ui(
                        &inbox,
                        UiMsg::Caption {
                            path: path.to_path_buf(),
                            text: summary,
                        },
                    );
                }
                true
            });
        }
    });
}

fn start_hypr_watch() {
    let inbox: Arc<Mutex<Vec<UiMsg>>> = Arc::new(Mutex::new(Vec::new()));
    hypr::watch(move |windows| push_ui(&inbox, UiMsg::Windows(windows)));
}

fn rebuild_rows(shell: &Shell) {
    let host = &shell.results_host;
    while let Some(child) = host.last_child() {
        host.remove(&child);
    }

    {
        let mut st = shell.state.borrow_mut();
        st.rows.clear();
        if st.editing.is_some() {
            return;
        }
        if st.results.is_empty() {
            host.append(&shell.empty);
            return;
        }
        let selected = st.selected;
        let rows: Vec<(Item, Live)> = st
            .results
            .iter()
            .map(|s| (s.item.clone(), s.live.clone()))
            .collect();
        drop(st);
        let thumbs: Vec<Option<Texture>> = rows
            .iter()
            .map(|(_, live)| {
                live.thumb_path()
                    .and_then(|path| shell.cached_texture(path))
            })
            .collect();
        let mut st = shell.state.borrow_mut();
        for (idx, ((item, live), thumb)) in rows.into_iter().zip(thumbs).enumerate() {
            let row = result_row(&item, &live, idx == selected, thumb.as_ref());
            host.append(&row);
            st.rows.push(row);
        }
    }

    let n = shell.state.borrow().rows.len();
    for idx in 0..n {
        let row = shell.state.borrow().rows[idx].clone();
        let click = GestureClick::new();
        let shell = shell.clone();
        click.connect_released(move |_, _, _, _| {
            {
                let mut st = shell.state.borrow_mut();
                st.selected = idx;
                paint_selection(&st);
            }
            shell.activate();
        });
        row.add_controller(click);
    }

    let selected = shell.state.borrow().selected;
    let row = shell.state.borrow().rows.get(selected).cloned();
    if let Some(row) = row {
        scroll_row_into_view(&shell.results_scroll, &shell.results_host, &row);
    }
}

impl Shell {
    fn launch_extension(&self, dir: PathBuf, command: String) {
        let allowed = self
            .state
            .borrow()
            .catalog
            .settings
            .borrow()
            .general
            .allow_extensions;
        if !allowed {
            self.set_status(
                "Extensions are off. Enable “Run installed extensions” in Settings first.",
            );
            return;
        }
        self.end_extension();
        let launch = extension::Launch {
            dir,
            command,
            arguments: HashMap::new(),
        };
        let session = extension::Session::start(launch);
        let generation = {
            let mut st = self.state.borrow_mut();
            st.extension_gen = st.extension_gen.wrapping_add(1);
            st.editing = None;
            st.status.clear();
            st.mode = Mode::Extension;
            st.results.clear();
            st.selected = 0;
            st.extension = Some(session);
            st.extension_gen
        };
        self.entry.set_text("");
        self.entry.grab_focus();
        self.set_status("Starting extension…");
        self.sync_chrome();
        rebuild_rows(self);
        self.update_preview();

        let shell = self.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
            shell.pump_extension(generation)
        });
    }

    fn pump_extension(&self, generation: u64) -> gtk4::glib::ControlFlow {
        {
            let expired = {
                let st = self.state.borrow();
                if st.extension_gen != generation {
                    return gtk4::glib::ControlFlow::Break;
                }
                st.extension.as_ref().is_some_and(|s| s.idle_expired())
            };
            if expired {
                self.end_extension();
                self.refresh();
                self.set_status("Extension host idle — stopped");
                return gtk4::glib::ControlFlow::Break;
            }
        }
        loop {
            let msg = {
                let st = self.state.borrow();
                if st.extension_gen != generation {
                    return gtk4::glib::ControlFlow::Break;
                }
                let Some(session) = &st.extension else {
                    return gtk4::glib::ControlFlow::Break;
                };
                session.try_recv()
            };
            let Some(msg) = msg else {
                return gtk4::glib::ControlFlow::Continue;
            };
            match msg {
                extension::Msg::Status(text) => self.set_status(text),
                extension::Msg::Ready => {
                    let no_view = self
                        .state
                        .borrow()
                        .extension
                        .as_ref()
                        .is_some_and(|s| s.no_view);
                    if !no_view {
                        self.set_status("");
                    }
                }
                extension::Msg::Render(view) => {
                    let new_list = {
                        let mut st = self.state.borrow_mut();
                        let Some(session) = st.extension.as_mut() else {
                            return gtk4::glib::ControlFlow::Break;
                        };
                        let previous = session
                            .view
                            .as_ref()
                            .and_then(|v| v.search_callback.clone());
                        let changed = previous != view.search_callback;
                        session.depth = view.depth;
                        session.view = Some(view);
                        if changed {
                            session.last_search = None;
                        }
                        changed
                    };
                    if new_list && !self.entry.text().is_empty() {
                        self.entry.set_text("");
                    } else {
                        self.refresh_extension();
                    }
                }
                extension::Msg::Toast {
                    title,
                    message,
                    style,
                } => {
                    let prefix = match style.as_str() {
                        "Error" => "✕ ",
                        "Dynamic" => "… ",
                        _ => "✓ ",
                    };
                    let text = if message.is_empty() {
                        format!("{prefix}{title}")
                    } else {
                        format!("{prefix}{title} — {message}")
                    };
                    self.set_status(text);
                }
                extension::Msg::Hud(title) => {
                    self.set_status(title.clone());
                    if !self.state.borrow().visible {
                        let _ = std::process::Command::new("notify-send")
                            .args(["Flint", &title])
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                    }
                }
                extension::Msg::SetSearchText(text) => {
                    self.entry.set_text(&text);
                    self.entry.set_position(-1);
                }
                extension::Msg::CloseWindow => {
                    self.hide();
                }
                extension::Msg::PopToRoot => {
                    self.enter_mode(Mode::Root);
                    return gtk4::glib::ControlFlow::Break;
                }
                extension::Msg::Request { id, method, params } => {
                    if method == "ui.confirmAlert" {
                        self.begin_confirm_alert(id, &params);
                    } else {
                        let result = self.serve_extension_request(&method, &params);
                        if let Some(session) = &self.state.borrow().extension {
                            session.respond(id, result);
                        }
                    }
                }
                extension::Msg::Done => {
                    let shell = self.clone();
                    gtk4::glib::timeout_add_local(
                        std::time::Duration::from_millis(900),
                        move || {
                            if shell.state.borrow().extension_gen == generation {
                                shell.hide();
                                shell.end_extension();
                            }
                            gtk4::glib::ControlFlow::Break
                        },
                    );
                }
                extension::Msg::Error(err) => {
                    let has_view = self
                        .state
                        .borrow()
                        .extension
                        .as_ref()
                        .is_some_and(|s| s.view.is_some());
                    self.set_status(format!("Extension error: {err}"));
                    if !has_view {
                        self.state.borrow_mut().mode = Mode::Extension;
                        self.sync_chrome();
                    }
                }
                extension::Msg::Exited => {
                    let (had_view, status) = {
                        let st = self.state.borrow();
                        (
                            st.extension.as_ref().is_some_and(|s| s.view.is_some()),
                            st.status.clone(),
                        )
                    };
                    if !had_view && status.is_empty() {
                        self.set_status("Extension exited");
                    }
                    if !had_view {
                        self.state.borrow_mut().extension = None;
                        self.state.borrow_mut().mode = Mode::Root;
                        let keep = self.state.borrow().status.clone();
                        self.refresh();
                        self.set_status(keep);
                    }
                    return gtk4::glib::ControlFlow::Break;
                }
            }
        }
    }

    fn refresh_extension(&self) {
        let query = self.entry.text().to_string();
        let previous_id = {
            let st = self.state.borrow();
            st.results.get(st.selected).map(|row| row.item.id.clone())
        };
        let view = {
            let mut st = self.state.borrow_mut();
            let Some(session) = st.extension.as_mut() else {
                return;
            };
            session.search(&query);
            session.view.clone()
        };
        let mut rows: Vec<Scored> = Vec::new();
        let confirm = self
            .state
            .borrow()
            .pending_confirm
            .as_ref()
            .map(|p| p.prompt.clone());
        if let Some(prompt) = confirm {
            for (index, item) in extension::confirm_items(&prompt).into_iter().enumerate() {
                rows.push(Scored::new(item, 100_000u32.saturating_sub(index as u32)));
            }
        } else if let Some(view) = &view {
            let needle = query.trim().to_lowercase();
            for (index, row) in view.rows.iter().enumerate() {
                if view.local_filter && !needle.is_empty() {
                    let hay = format!(
                        "{} {} {} {}",
                        row.title, row.subtitle, row.keywords, row.section
                    )
                    .to_lowercase();
                    if !needle.split_whitespace().all(|w| hay.contains(w)) {
                        continue;
                    }
                }
                let subtitle = if row.section.is_empty() || row.subtitle.contains(&row.section) {
                    row.subtitle.clone()
                } else if row.subtitle.is_empty() {
                    row.section.clone()
                } else {
                    format!("{}  ·  {}", row.section, row.subtitle)
                };
                let action = if !row.field_kind.is_empty() {
                    let (node, prop) = row.field_on_change.clone().unwrap_or((0, String::new()));
                    Action::ExtensionFormField {
                        node,
                        prop,
                        kind: row.field_kind.clone(),
                        value: row.field_value.clone(),
                        field_id: row.field_id.clone(),
                    }
                } else {
                    Action::Extension {
                        actions: row.actions.clone(),
                        detail: row.detail.clone(),
                    }
                };
                let item = Item {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    subtitle,
                    keywords: row.keywords.clone(),
                    kind: Kind::Extension,
                    icon: row.icon.clone(),
                    action,
                };
                rows.push(Scored::new(item, 100_000u32.saturating_sub(index as u32)));
            }
        }
        {
            let mut st = self.state.borrow_mut();
            st.mode = Mode::Extension;
            st.selected = previous_id
                .and_then(|id| rows.iter().position(|r| r.item.id == id))
                .unwrap_or(0);
            st.results = rows;
        }
        if let Some(notice) = view.as_ref().and_then(|v| v.notice.clone()) {
            self.set_status(notice);
        }
        self.sync_chrome();
        if let Some(view) = &view {
            if !view.placeholder.is_empty() {
                self.entry.set_placeholder_text(Some(&view.placeholder));
            }
            if !view.empty_title.is_empty() {
                self.empty_title.set_text(&view.empty_title);
                self.empty_sub.set_text(&view.empty_description);
            }
        }
        rebuild_rows(self);
        let row = {
            let st = self.state.borrow();
            st.rows.get(st.selected).cloned()
        };
        if let Some(row) = row {
            scroll_row_into_view(&self.results_scroll, &self.results_host, &row);
        }
        self.update_preview();
        self.request_visible_thumbs();
    }

    fn run_extension_action(&self, actions: &[crate::item::ExtAction], index: usize) {
        let Some(action) = actions.get(index) else {
            if index == 0 {
                self.set_status("This item has no action");
            }
            return;
        };
        let args = if action.prop == "onSubmit" {
            let values = self
                .state
                .borrow()
                .extension
                .as_ref()
                .and_then(|s| s.view.as_ref())
                .map(extension::form_values)
                .unwrap_or(serde_json::json!({}));
            vec![serde_json::json!({"values": values})]
        } else {
            Vec::new()
        };
        if let Some(session) = &self.state.borrow().extension {
            session.invoke(action.node, &action.prop, args);
        }
    }

    fn extension_back(&self) {
        let depth = self
            .state
            .borrow()
            .extension
            .as_ref()
            .map(|s| s.depth)
            .unwrap_or(1);
        if depth > 1 {
            if let Some(session) = &self.state.borrow().extension {
                session.pop();
            }
        } else {
            self.enter_mode(Mode::Root);
        }
    }

    fn end_extension(&self) {
        if self.state.borrow().pending_confirm.is_some() {
            self.finish_confirm(false);
        }
        let session = self.state.borrow_mut().extension.take();
        if session.is_some() {
            let mut st = self.state.borrow_mut();
            st.extension_gen = st.extension_gen.wrapping_add(1);
            if st.mode == Mode::Extension {
                st.mode = Mode::Root;
            }
        }
        drop(session);
    }

    fn begin_confirm_alert(&self, id: u64, params: &serde_json::Value) {
        if let Some(prev) = self.state.borrow_mut().pending_confirm.take()
            && let Some(session) = &self.state.borrow().extension
        {
            session.respond(prev.id, Ok(serde_json::json!({"confirmed": false})));
        }
        let prompt = extension::confirm_prompt(params);
        let status = if prompt.message.is_empty() {
            prompt.title.clone()
        } else {
            format!("{} — {}", prompt.title, prompt.message)
        };
        self.state.borrow_mut().pending_confirm = Some(PendingConfirm { id, prompt });
        self.entry.set_text("");
        self.set_status(status);
        self.refresh_extension();
    }

    fn finish_confirm(&self, confirmed: bool) {
        let pending = self.state.borrow_mut().pending_confirm.take();
        let Some(pending) = pending else {
            return;
        };
        if let Some(session) = &self.state.borrow().extension {
            session.respond(pending.id, Ok(serde_json::json!({"confirmed": confirmed})));
        }
        if self.state.borrow().extension.is_some() {
            self.set_status("");
            self.refresh_extension();
        }
    }

    fn begin_form_edit(
        &self,
        node: u64,
        prop: String,
        kind: String,
        value: String,
        field_id: String,
    ) {
        self.state.borrow_mut().editing = Some(Editing::FormField {
            node,
            prop,
            kind: kind.clone(),
            field_id,
        });
        self.state.borrow_mut().mode = Mode::Extension;
        if kind == "textarea" {
            self.entry.set_text("");
            self.detail_view.buffer().set_text(&value);
            self.set_status("Editing field · Ctrl+S saves, Esc returns");
            self.sync_chrome();
            self.detail_view.grab_focus();
        } else {
            self.entry.set_text(&value);
            self.entry.set_position(-1);
            self.detail.set_visible(false);
            self.set_status("Editing field · Ctrl+S saves, Esc returns");
            self.sync_chrome();
            self.entry.grab_focus();
        }
    }

    fn commit_form_field(&self, node: u64, prop: &str, kind: &str, field_id: &str, value: String) {
        if let Some(session) = self.state.borrow_mut().extension.as_mut()
            && let Some(view) = session.view.as_mut()
        {
            for row in &mut view.rows {
                if row.field_id == field_id && !field_id.is_empty() {
                    row.field_value = value.clone();
                    row.subtitle = if kind == "checkbox" {
                        if value == "true" {
                            "On · Enter toggles".into()
                        } else {
                            "Off · Enter toggles".into()
                        }
                    } else if kind == "password" && !value.is_empty() {
                        "••••".into()
                    } else if value.is_empty() {
                        "Enter to edit".into()
                    } else {
                        value.clone()
                    };
                }
            }
        }
        if !prop.is_empty() {
            let arg = if kind == "checkbox" {
                serde_json::json!(value == "true")
            } else {
                serde_json::json!(value)
            };
            if let Some(session) = &self.state.borrow().extension {
                session.invoke(node, prop, vec![arg]);
            }
        }
    }

    fn serve_extension_request(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use serde_json::{Value, json};
        let text_of = |content: &Value| -> String {
            content
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    content.get("urls").and_then(Value::as_array).map(|u| {
                        u.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                })
                .unwrap_or_default()
        };
        match method {
            "clipboard.copy" => {
                let text = text_of(params.get("content").unwrap_or(&Value::Null));
                action::copy_text(&text);
                if !text.is_empty() {
                    self.set_status("Copied");
                }
                Ok(json!({}))
            }
            "clipboard.paste" => {
                let text = text_of(params.get("content").unwrap_or(&Value::Null));
                self.hide();
                action::run(&Action::Paste(text));
                Ok(json!({}))
            }
            "clipboard.read" => Ok(json!({"text": clipboard::current_text().unwrap_or_default()})),
            "ui.getSelectedText" => Ok(json!({
                "text": clipboard::selection_or_clipboard().unwrap_or_default()
            })),
            "app.open" => {
                let target = params
                    .get("target")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if target.is_empty() {
                    return Err("nothing to open".into());
                }
                let path = std::path::Path::new(&target);
                if path.is_absolute() && path.exists() {
                    action::run(&Action::OpenPath(path.to_path_buf()));
                } else {
                    action::run(&Action::OpenUri(target));
                }
                Ok(json!({}))
            }
            "app.runInTerminal" => {
                let parts: Vec<String> = params
                    .get("cmdline")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                if parts.is_empty() {
                    return Err("empty command".into());
                }
                let command = parts
                    .iter()
                    .map(|p| shell_quote(p))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.hide();
                action::run(&Action::Shell {
                    command,
                    terminal: true,
                });
                Ok(json!({}))
            }
            other => Err(format!("{other} is not supported by Flint")),
        }
    }
}

fn shell_quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | '@' | '%' | '+')
        })
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn extension_hint(state: &State) -> Option<String> {
    let session = state.extension.as_ref()?;
    let mut parts: Vec<String> = Vec::new();
    let title = session
        .view
        .as_ref()
        .filter(|v| !v.title.is_empty())
        .map(|v| v.title.clone())
        .unwrap_or_else(|| session.title.clone());
    if !title.is_empty() {
        parts.push(title);
    }
    if session.view.as_ref().is_some_and(|v| v.is_loading) {
        parts.push("loading…".into());
    }
    if let Some(row) = state.results.get(state.selected)
        && let Action::Extension { actions, .. } = &row.item.action
    {
        if let Some(primary) = actions.first() {
            parts.push(format!("Enter · {}", primary.title));
        }
        if let Some(secondary) = actions.get(1) {
            parts.push(format!("⇧Enter · {}", secondary.title));
        }
        if actions.len() > 2 {
            parts.push(format!("+{} more", actions.len() - 2));
        }
    }
    if session.depth > 1 {
        parts.push("Esc · back".into());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("   "))
    }
}

fn result_hint(state: &State) -> Option<String> {
    let n = state.results.len();
    if state.mode == Mode::Extension {
        return extension_hint(state);
    }
    if n == 0 {
        return None;
    }
    let files = state
        .results
        .iter()
        .filter(|row| matches!(row.item.kind, Kind::File | Kind::Media))
        .count();
    if state.mode == Mode::Files || files >= 8 {
        if files == n {
            Some(format!("{n} files · ↑↓ to browse"))
        } else {
            Some(format!("{n} results · {files} files · ↑↓ to browse"))
        }
    } else if n >= 16 {
        Some(format!("{n} results · ↑↓ to browse"))
    } else {
        None
    }
}

fn scroll_row_into_view(scroll: &ScrolledWindow, host: &Box, row: &Box) {
    let adj = scroll.vadjustment();
    let Some(bounds) = row.compute_bounds(host) else {
        return;
    };
    let y = f64::from(bounds.y());
    let bottom = y + f64::from(bounds.height());
    let value = adj.value();
    let page = adj.page_size();
    if y < value {
        adj.set_value(y);
    } else if bottom > value + page {
        adj.set_value((bottom - page).max(0.0));
    }
}

fn paint_selection(state: &State) {
    for (idx, row) in state.rows.iter().enumerate() {
        if idx == state.selected {
            row.add_css_class("selected");
        } else {
            row.remove_css_class("selected");
        }
    }
}

fn result_row(item: &Item, live: &Live, selected: bool, thumb: Option<&Texture>) -> Box {
    let row = Box::new(Orientation::Horizontal, 8);
    row.add_css_class("row");
    if selected {
        row.add_css_class("selected");
    }
    if !live.is_none() {
        row.add_css_class("live-row");
    }
    row.set_hexpand(true);

    let accent = Box::new(Orientation::Vertical, 0);
    accent.add_css_class("accent");
    accent.set_valign(Align::Center);
    row.append(&accent);

    let icon = if live.thumb_path().is_some() {
        let image = if let Some(texture) = thumb {
            Image::from_paintable(Some(texture))
        } else if matches!(live, Live::Media { .. }) {
            Image::from_icon_name("audio-x-generic")
        } else {
            Image::from_icon_name("image-x-generic")
        };
        image.add_css_class("live-thumb");
        image
    } else {
        match &item.icon {
            Icon::Name(name) => Image::from_icon_name(name),
            Icon::Path(path) => Image::from_file(path),
            Icon::None => Image::from_icon_name(kind_icon(item.kind)),
        }
    };
    icon.set_pixel_size(if live.thumb_path().is_some() { 48 } else { 28 });
    icon.set_valign(Align::Center);
    icon.add_css_class("icon-wrap");
    row.append(&icon);

    let text = Box::new(Orientation::Vertical, 1);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let title_text = match live {
        Live::Weather { summary, .. } if !summary.is_empty() => summary.as_str(),
        _ => item.title.as_str(),
    };
    let title = Label::new(Some(title_text));
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.add_css_class(
        if item.kind == Kind::Calc || matches!(live, Live::Weather { .. }) {
            "calc-title"
        } else {
            "title"
        },
    );
    text.append(&title);

    let subtitle_text = match live {
        Live::Weather {
            location, extra, ..
        } => {
            if extra.is_empty() {
                location.clone()
            } else if location.is_empty() {
                extra.clone()
            } else {
                format!("{location} · {extra}")
            }
        }
        _ => item.subtitle.clone(),
    };
    if !subtitle_text.is_empty() {
        let sub = Label::new(Some(&subtitle_text));
        sub.set_xalign(0.0);
        sub.set_ellipsize(pango::EllipsizeMode::End);
        sub.add_css_class("subtitle");
        text.append(&sub);
    }

    match live {
        Live::Snippet { text: body } if !body.is_empty() => {
            let snippet = Label::new(Some(body));
            snippet.set_xalign(0.0);
            snippet.set_wrap(true);
            snippet.set_lines(3);
            snippet.set_ellipsize(pango::EllipsizeMode::End);
            snippet.add_css_class("live-snippet");
            text.append(&snippet);
        }
        Live::Media { hint, .. } => {
            let hint_l = Label::new(Some(hint));
            hint_l.set_xalign(0.0);
            hint_l.set_ellipsize(pango::EllipsizeMode::End);
            hint_l.add_css_class("live-snippet");
            text.append(&hint_l);
        }
        Live::None | Live::Weather { .. } | Live::Image { .. } | Live::Snippet { .. } => {}
    }
    row.append(&text);

    let pill = Label::new(Some(item.kind.label()));
    pill.add_css_class("pill");
    pill.set_valign(Align::Center);
    row.append(&pill);

    row
}

fn empty_state() -> (Box, Label, Label) {
    let wrap = Box::new(Orientation::Vertical, 10);
    wrap.add_css_class("empty");

    let title = Label::new(Some(Mode::Root.empty_title()));
    title.set_xalign(0.0);
    title.add_css_class("empty-title");
    wrap.append(&title);

    let sub = Label::new(Some(Mode::Root.empty_sub()));
    sub.set_xalign(0.0);
    sub.set_wrap(true);
    sub.add_css_class("empty-sub");
    wrap.append(&sub);

    let chips = Box::new(Orientation::Horizontal, 8);
    chips.set_margin_top(8);
    for hint in [
        "file", "?ask", "note", "win", "clip", "link", "calc", "store", "set",
    ] {
        let chip = Label::new(Some(hint));
        chip.add_css_class("chip");
        chips.append(&chip);
    }
    wrap.append(&chips);
    (wrap, title, sub)
}

fn footer() -> Box {
    let bar = Box::new(Orientation::Horizontal, 12);
    bar.add_css_class("footer");

    let mark = Image::from_file(crate::paths::logo_app());
    mark.set_pixel_size(18);
    mark.set_valign(Align::Center);
    mark.add_css_class("wordmark-logo");
    bar.append(&mark);

    let word = Label::new(Some("FLINT"));
    word.add_css_class("wordmark");
    bar.append(&word);

    let spacer = Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    bar.append(&spacer);

    bar.append(&hint_pair("↑↓", "move"));
    bar.append(&hint_pair("pg", "jump"));
    bar.append(&hint_pair("↵", "open"));
    bar.append(&hint_pair("ctrl+k", "actions"));
    bar.append(&hint_pair("esc", "back"));
    bar
}

fn hint_pair(key: &str, label: &str) -> Box {
    let wrap = Box::new(Orientation::Horizontal, 6);
    wrap.set_valign(Align::Center);
    let kbd = Label::new(Some(key));
    kbd.add_css_class("kbd");
    wrap.append(&kbd);
    let text = Label::new(Some(label));
    text.add_css_class("hint");
    wrap.append(&text);
    wrap
}

fn kind_icon(kind: Kind) -> &'static str {
    match kind {
        Kind::App => "application-x-executable",
        Kind::Window => "preferences-system-windows",
        Kind::Command => "system-run",
        Kind::Calc => "accessories-calculator",
        Kind::File => "text-x-generic",
        Kind::Web => "web-browser",
        Kind::Clipboard => "edit-paste",
        Kind::Shell => "utilities-terminal",
        Kind::Snippet => "insert-text",
        Kind::Extension => "application-x-addon",
        Kind::Note => "accessories-text-editor",
        Kind::Ai => "help-faq",
        Kind::Voice => "audio-input-microphone",
        Kind::Settings => "preferences-system",
        Kind::Store => "folder-download",
        Kind::Script => "utilities-terminal",
        Kind::Weather => "weather-few-clouds",
        Kind::Media => "audio-x-generic",
    }
}

fn panel_actions(item: &Item, st: &State) -> Vec<PanelAction> {
    let mut out = Vec::new();
    let favs = favorites::Store::load();
    let pinned = favs.is_pinned(&item.id);
    out.push(PanelAction {
        title: if pinned {
            "Unpin favorite".into()
        } else {
            "Pin favorite".into()
        },
        keywords: "pin favorite star".into(),
        kind: PanelKind::ToggleFavorite,
    });
    out.push(PanelAction {
        title: "Set alias".into(),
        keywords: "alias nickname".into(),
        kind: PanelKind::SetAlias,
    });
    out.push(PanelAction {
        title: "Copy title".into(),
        keywords: "copy name".into(),
        kind: PanelKind::Copy(item.title.clone()),
    });

    match item.kind {
        Kind::File | Kind::Media => {
            if let Some(path) = item_path(item) {
                file_actions(&mut out, &path, true);
            }
        }
        Kind::App => {
            if let Action::LaunchDesktop { path } = &item.action {
                out.push(PanelAction {
                    title: "Launch".into(),
                    keywords: "open start".into(),
                    kind: PanelKind::Launch,
                });
                if let Some(class) = crate::pkg::class_hint(path) {
                    out.push(PanelAction {
                        title: "Quit".into(),
                        keywords: "quit close exit".into(),
                        kind: PanelKind::Run(Action::QuitClass { class }),
                    });
                }
                if let Some(mapped) = crate::pkg::mapped_package(path) {
                    out.push(PanelAction {
                        title: "Uninstall".into(),
                        keywords: "uninstall remove package".into(),
                        kind: PanelKind::Confirm(crate::pkg::confirm_item(&item.title, &mapped)),
                    });
                }
                out.push(PanelAction {
                    title: "Copy .desktop path".into(),
                    keywords: "copy path desktop".into(),
                    kind: PanelKind::CopyPath(path.clone()),
                });
                if let Some(parent) = path.parent() {
                    out.push(PanelAction {
                        title: "Show in files".into(),
                        keywords: "folder reveal".into(),
                        kind: PanelKind::ShowInFiles(parent.to_path_buf()),
                    });
                }
            }
        }
        Kind::Clipboard => {
            let id = clipboard::strip_prefix(&item.id).to_string();
            let pinned_clip = st.catalog.clips.borrow().get(&id).is_some_and(|e| e.pinned);
            let text = match &item.action {
                Action::Paste(text) => text.clone(),
                _ => String::new(),
            };
            out.push(PanelAction {
                title: "Paste".into(),
                keywords: "paste".into(),
                kind: PanelKind::Paste(text.clone()),
            });
            out.push(PanelAction {
                title: "Paste as plain text".into(),
                keywords: "paste plain".into(),
                kind: PanelKind::Paste(text.clone()),
            });
            if pinned_clip {
                out.push(PanelAction {
                    title: "Unpin clipboard entry".into(),
                    keywords: "unpin clip".into(),
                    kind: PanelKind::ClipUnpin(id.clone()),
                });
            } else {
                out.push(PanelAction {
                    title: "Pin clipboard entry".into(),
                    keywords: "pin clip".into(),
                    kind: PanelKind::ClipPin(id.clone()),
                });
            }
            out.push(PanelAction {
                title: "Rename".into(),
                keywords: "rename label".into(),
                kind: PanelKind::ClipRename(id.clone()),
            });
            out.push(PanelAction {
                title: "Edit".into(),
                keywords: "edit body".into(),
                kind: PanelKind::ClipEdit(id),
            });
            out.push(PanelAction {
                title: "Copy".into(),
                keywords: "copy".into(),
                kind: PanelKind::Copy(text),
            });
            out.push(PanelAction {
                title: "Attach to Ask AI".into(),
                keywords: "attach ask ai clipboard".into(),
                kind: PanelKind::Run(Action::AttachClipboard),
            });
        }
        Kind::Snippet => {
            let keyword = item.id.strip_prefix("snip:").unwrap_or("").to_string();
            out.push(PanelAction {
                title: "Paste expanded".into(),
                keywords: "paste snippet".into(),
                kind: PanelKind::SnippetPaste(keyword.clone()),
            });
            out.push(PanelAction {
                title: "Copy expanded".into(),
                keywords: "copy snippet".into(),
                kind: PanelKind::SnippetCopy(keyword),
            });
        }
        Kind::Calc => {
            if let Action::Copy(text) = &item.action {
                out.push(PanelAction {
                    title: "Copy result".into(),
                    keywords: "copy calc".into(),
                    kind: PanelKind::Copy(text.clone()),
                });
            }
        }
        Kind::Web => {
            out.push(PanelAction {
                title: "Open".into(),
                keywords: "open url".into(),
                kind: PanelKind::Open,
            });
        }
        Kind::Voice => {
            let text = match &item.action {
                Action::Paste(text) | Action::TypeText(text) | Action::Copy(text) => {
                    Some(text.clone())
                }
                _ => None,
            };
            if let Some(text) = text {
                out.push(PanelAction {
                    title: "Paste with wtype".into(),
                    keywords: "wtype type paste focused".into(),
                    kind: PanelKind::Run(Action::TypeText(text.clone())),
                });
                out.push(PanelAction {
                    title: "Paste".into(),
                    keywords: "paste".into(),
                    kind: PanelKind::Paste(text.clone()),
                });
                out.push(PanelAction {
                    title: "Copy".into(),
                    keywords: "copy".into(),
                    kind: PanelKind::Copy(text),
                });
            }
        }
        Kind::Command | Kind::Shell => {
            out.push(PanelAction {
                title: "Run".into(),
                keywords: "launch open".into(),
                kind: PanelKind::Launch,
            });
        }
        Kind::Window => {
            out.push(PanelAction {
                title: "Focus".into(),
                keywords: "open focus".into(),
                kind: PanelKind::Open,
            });
            if let Action::FocusWindow { address } = &item.action {
                for (title, name, keys) in [
                    ("Left half", "left-half", "tile left half"),
                    ("Right half", "right-half", "tile right half"),
                    ("Maximize", "maximize", "max fullscreen"),
                    ("Center", "center", "center window"),
                    (
                        "Almost maximize",
                        "almost-maximize",
                        "almost maximize inset",
                    ),
                ] {
                    out.push(PanelAction {
                        title: title.into(),
                        keywords: keys.into(),
                        kind: PanelKind::Run(Action::Layout {
                            name: name.into(),
                            address: Some(address.clone()),
                        }),
                    });
                }
                out.push(PanelAction {
                    title: "Close".into(),
                    keywords: "close quit window".into(),
                    kind: PanelKind::Run(Action::CloseWindow {
                        address: address.clone(),
                    }),
                });
                if let Some(pid) = crate::hypr::clients()
                    .iter()
                    .find(|c| c.address == *address)
                    .map(|c| c.pid)
                    .filter(|pid| crate::quit::can_kill(*pid, std::process::id() as i32))
                {
                    out.push(PanelAction {
                        title: "Force quit".into(),
                        keywords: "kill force quit sigkill".into(),
                        kind: PanelKind::Run(Action::KillPid { pid }),
                    });
                }
            }
        }
        _ => {
            if let Some(path) = item_path(item) {
                file_actions(&mut out, &path, false);
            }
        }
    }
    out
}

fn file_actions(out: &mut Vec<PanelAction>, path: &Path, include_open: bool) {
    out.push(PanelAction {
        title: "Attach to Ask AI".into(),
        keywords: "attach ask ai file".into(),
        kind: PanelKind::Run(Action::AttachPath {
            path: path.to_path_buf(),
        }),
    });
    for (title, action) in crate::ocr::actions_for_path(path) {
        out.push(PanelAction {
            title,
            keywords: "ocr qr barcode tesseract".into(),
            kind: PanelKind::Run(action),
        });
    }
    out.push(PanelAction {
        title: "Copy path".into(),
        keywords: "copy path".into(),
        kind: PanelKind::CopyPath(path.to_path_buf()),
    });
    if include_open {
        out.push(PanelAction {
            title: "Open".into(),
            keywords: "open".into(),
            kind: PanelKind::Open,
        });
    }
    for app in recommended_apps(path) {
        let name = app.name().to_string();
        out.push(PanelAction {
            title: format!("Open with {name}"),
            keywords: format!("open with {name}"),
            kind: PanelKind::OpenWith {
                app,
                path: path.to_path_buf(),
            },
        });
    }
    let reveal = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf())
    };
    out.push(PanelAction {
        title: "Show in files".into(),
        keywords: "folder reveal files".into(),
        kind: PanelKind::ShowInFiles(reveal),
    });
}

fn recommended_apps(path: &Path) -> Vec<gtk4::gio::AppInfo> {
    let (ctype, _) = gtk4::gio::content_type_guess(Some(path), None);
    gtk4::gio::AppInfo::recommended_for_type(ctype.as_str())
        .into_iter()
        .take(4)
        .collect()
}

fn item_path(item: &Item) -> Option<PathBuf> {
    match &item.action {
        Action::OpenPath(path)
        | Action::PlayMedia { path }
        | Action::LaunchDesktop { path }
        | Action::RunScript { path }
        | Action::AttachPath { path } => Some(path.clone()),
        _ => None,
    }
}

fn path_from_clipboard() -> Option<PathBuf> {
    let text = clipboard::current_text()?;
    let text = text.trim();
    if text.is_empty() || text.contains('\n') {
        return None;
    }
    let path = if let Some(rest) = text.strip_prefix("~/") {
        dirs::home_dir()?.join(rest)
    } else if text == "~" {
        dirs::home_dir()?
    } else {
        PathBuf::from(text)
    };
    if path.is_file() { Some(path) } else { None }
}

fn alias_typed(query: &str) -> Option<String> {
    let q = query.trim();
    let rest = q
        .strip_prefix("alias:")
        .or_else(|| {
            let lower = q.to_ascii_lowercase();
            if lower.starts_with("alias ") {
                Some(&q[6..])
            } else {
                None
            }
        })?
        .trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn calc_expr(subtitle: &str) -> &str {
    subtitle
        .split("  ·  ")
        .next()
        .unwrap_or(subtitle)
        .split("  →  ")
        .next()
        .unwrap_or(subtitle)
        .trim()
}

fn buffer_text(view: &TextView) -> String {
    let buffer = view.buffer();
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    buffer.text(&start, &end, false).to_string()
}

fn spawn_particles(width: f64, height: f64) -> Vec<Particle> {
    let mut seed = width.to_bits() ^ height.to_bits() ^ 0xC0FFEE;
    let mut out = Vec::with_capacity(80);
    let colors = [
        (1.0, 0.353, 0.122),
        (0.965, 0.945, 0.918),
        (1.0, 0.824, 0.690),
        (1.0, 0.706, 0.541),
    ];
    for i in 0..80 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let rnd = |s: &mut u64| {
            *s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            (*s >> 33) as f64 / (u32::MAX as f64)
        };
        let color = colors[i % colors.len()];
        out.push(Particle {
            x: width * 0.5 + (rnd(&mut seed) - 0.5) * width * 0.4,
            y: height * 0.28 + rnd(&mut seed) * 24.0,
            vx: (rnd(&mut seed) - 0.5) * 14.0,
            vy: rnd(&mut seed) * -7.0 - 2.0,
            life: 1.0,
            size: 3.0 + rnd(&mut seed) * 5.0,
            r: color.0,
            g: color.1,
            b: color.2,
        });
    }
    out
}

fn tick_particles(bits: &mut [Particle]) {
    for p in bits.iter_mut() {
        p.vy += 0.28;
        p.x += p.vx;
        p.y += p.vy;
        p.life -= 0.014;
    }
}

fn load_css() {
    let provider = CssProvider::new();
    provider.load_from_data(CSS);
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{Shortcut, for_each_live_thumb, shortcut, thumb_rows_in_view, unique_thumb_paths};
    use crate::files::Cancel;
    use gtk4::gdk::{Key, ModifierType};
    use std::path::{Path, PathBuf};

    #[test]
    fn ctrl_k_is_actions_even_with_caps() {
        let ctrl = ModifierType::CONTROL_MASK;
        assert_eq!(shortcut(Key::k, ctrl), Some(Shortcut::Actions));
        assert_eq!(shortcut(Key::K, ctrl), Some(Shortcut::Actions));
        assert_eq!(shortcut(Key::k, ctrl | ModifierType::SHIFT_MASK), None);
        assert_eq!(shortcut(Key::question, ctrl), Some(Shortcut::Ask));
        assert_eq!(
            shortcut(Key::slash, ctrl | ModifierType::SHIFT_MASK),
            Some(Shortcut::Ask)
        );
        assert_eq!(
            shortcut(Key::comma, ctrl | ModifierType::SHIFT_MASK),
            None,
            "AZERTY Ctrl+Shift+comma must not steal Settings"
        );
        assert_eq!(shortcut(Key::comma, ctrl), Some(Shortcut::Settings));
    }

    #[test]
    fn visible_thumbs_are_viewport_plus_selected_not_the_full_list() {
        let count = 250;
        let height = 40.0;
        let bounds: Vec<Option<(f64, f64)>> = (0..count)
            .map(|i| Some((i as f64 * height, height)))
            .collect();
        let rows = thumb_rows_in_view(count, 0, 0.0, 200.0, &bounds);
        assert!(
            rows.len() < 20,
            "must not enqueue all {count} file hits, got {rows:?}"
        );
        assert_eq!(rows.first().copied(), Some(0));
        assert!(!rows.contains(&80));

        let with_selected = thumb_rows_in_view(count, 80, 0.0, 200.0, &bounds);
        assert!(with_selected.contains(&80), "selected row is always queued");
        assert!(with_selected.len() < 20);
    }

    #[test]
    fn unlaid_out_rows_use_a_window_around_selection() {
        let bounds = vec![None; 250];
        let rows = thumb_rows_in_view(250, 100, 0.0, 0.0, &bounds);
        assert_eq!(rows.first().copied(), Some(84));
        assert_eq!(rows.last().copied(), Some(116));
        assert!(!rows.contains(&0));
        assert!(!rows.contains(&249));
    }

    #[test]
    fn unique_thumb_paths_keep_last_occurrence_order() {
        let paths = unique_thumb_paths(vec![
            PathBuf::from("a"),
            PathBuf::from("b"),
            PathBuf::from("a"),
            PathBuf::from("c"),
        ]);
        assert_eq!(
            paths,
            vec![PathBuf::from("b"), PathBuf::from("a"), PathBuf::from("c")]
        );
    }

    #[test]
    fn cancelled_thumb_batch_skips_remaining_paths() {
        let cancel = Cancel::new();
        let mut seen: Vec<PathBuf> = Vec::new();
        for_each_live_thumb(
            vec![
                PathBuf::from("one"),
                PathBuf::from("two"),
                PathBuf::from("three"),
            ],
            &cancel,
            |path| {
                seen.push(path.to_path_buf());
                if path == Path::new("one") {
                    cancel.cancel();
                    return false;
                }
                true
            },
        );
        assert_eq!(seen, vec![PathBuf::from("one")]);
    }

    #[test]
    fn already_cancelled_batch_processes_nothing() {
        let cancel = Cancel::new();
        cancel.cancel();
        let mut n = 0;
        for_each_live_thumb(
            vec![PathBuf::from("one"), PathBuf::from("two")],
            &cancel,
            |_| {
                n += 1;
                true
            },
        );
        assert_eq!(n, 0);
    }
}
