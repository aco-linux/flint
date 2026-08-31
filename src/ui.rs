use std::cell::RefCell;
use std::rc::Rc;
use std::thread;

use gtk4::gdk::{Key, ModifierType};
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box, CssProvider, Entry, EventControllerKey,
    GestureClick, HeaderBar, Image, Label, Orientation, Overflow, Overlay, PolicyType,
    STYLE_PROVIDER_PRIORITY_APPLICATION, ScrolledWindow, TextView, WrapMode,
};

use crate::action;
use crate::ai;
use crate::auth;
use crate::catalog::{self, Catalog, LiveExtras, Scored};
use crate::clipboard;
use crate::item::{Action, Icon, Item, Kind};
use crate::mode::Mode;
use crate::models;
use crate::notes;
use crate::snippets;
use crate::store;
use crate::usage;
use crate::voice::{self, Session as VoiceSession};

const CSS: &str = include_str!("theme.css");

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
    state: Rc<RefCell<State>>,
}

struct State {
    catalog: Catalog,
    results: Vec<Scored>,
    selected: usize,
    rows: Vec<Box>,
    mode: Mode,
    visible: bool,
    status: String,
    editing_note: Option<String>,
    voice: VoiceSession,
    search_gen: u64,
}

pub fn build(app: &Application, catalog: Catalog) -> Shell {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Flint")
        .default_width(980)
        .default_height(720)
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
    window.set_child(Some(&overlay));

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
        state: Rc::new(RefCell::new(State {
            catalog,
            results: Vec::new(),
            selected: 0,
            rows: Vec::new(),
            mode: Mode::Root,
            visible: false,
            status: String::new(),
            editing_note: None,
            voice: VoiceSession::new(),
            search_gen: 0,
        })),
    };

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
            if shell.state.borrow().editing_note.is_none() {
                shell.refresh();
            }
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
            state: self.state.clone(),
        }
    }
}

impl Shell {
    pub fn toggle(&self) {
        if self.state.borrow().visible {
            self.hide();
        } else {
            self.open(Mode::Root);
        }
    }

    pub fn open(&self, mode: Mode) {
        self.state.borrow_mut().visible = true;
        self.window.present();
        self.enter_mode(mode);
        self.entry.grab_focus();
        self.refresh();
    }

    pub fn hide(&self) {
        self.commit_note();
        if self.state.borrow().voice.state() != voice::State::Idle {
            self.state.borrow().voice.cancel();
        }
        self.state.borrow_mut().visible = false;
        self.window.set_visible(false);
    }

    fn refresh(&self) {
        let query = self.entry.text().to_string();
        let (generation, include_in_root) = {
            let mut st = self.state.borrow_mut();
            if st.editing_note.is_some() {
                return;
            }
            st.search_gen = st.search_gen.saturating_add(1);
            let (mode, results) = st.catalog.search_fast(&query);
            st.mode = mode;
            st.results = results;
            st.selected = 0;
            let include_in_root = st.catalog.settings.borrow().files.include_in_root;
            (st.search_gen, include_in_root)
        };
        self.sync_chrome();
        rebuild_rows(self);
        self.update_preview();
        self.schedule_live(generation, query, include_in_root);
    }

    fn schedule_live(&self, generation: u64, query: String, include_in_root: bool) {
        let mode = self.state.borrow().mode;
        if !catalog::live_needed(&query, mode, include_in_root) {
            return;
        }
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        let usage = self.state.borrow().catalog.usage.clone();
        let shell = self.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
            if shell.state.borrow().search_gen != generation {
                return gtk4::glib::ControlFlow::Break;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let q = query.clone();
            let s = settings.clone();
            let u = usage.clone();
            thread::spawn(move || {
                let _ = tx.send(catalog::live_extras(&q, mode, &s, &u));
            });
            let shell = shell.clone();
            gtk4::glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                match rx.try_recv() {
                    Ok(live) => {
                        if shell.state.borrow().search_gen == generation {
                            shell.apply_live(live);
                        }
                        gtk4::glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        gtk4::glib::ControlFlow::Break
                    }
                }
            });
            gtk4::glib::ControlFlow::Break
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
    }

    fn update_preview(&self) {
        if self.state.borrow().editing_note.is_some() {
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
                self.preview_image.set_from_file(Some(&path));
                self.preview_image.set_visible(true);
                self.preview_text.set_text(&path.to_string_lossy());
                self.preview_text.set_visible(true);
                self.preview.set_visible(true);
            }
            crate::preview::Preview::Media { hint } => {
                self.preview_image.set_visible(false);
                self.preview_text.set_text(&hint);
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
        let editing = st.editing_note.is_some();
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
        let ctrl = mods.contains(ModifierType::CONTROL_MASK);
        if ctrl && matches!(key, Key::comma) {
            self.enter_mode(Mode::Settings);
            Propagation::Stop
        } else if ctrl && matches!(key, Key::n) {
            self.enter_mode(Mode::Notes);
            Propagation::Stop
        } else if ctrl && matches!(key, Key::f) {
            self.enter_mode(Mode::Files);
            Propagation::Stop
        } else if ctrl && matches!(key, Key::k | Key::question) {
            self.enter_mode(Mode::Ask);
            Propagation::Stop
        } else if ctrl && matches!(key, Key::s) && self.state.borrow().editing_note.is_some() {
            self.commit_note();
            self.set_status("Note saved");
            Propagation::Stop
        } else {
            match key {
                Key::Escape => {
                    if self.state.borrow().editing_note.is_some() {
                        self.commit_note();
                        self.enter_mode(Mode::Notes);
                    } else if self.state.borrow().voice.state() == voice::State::Listening {
                        self.state.borrow().voice.cancel();
                        self.set_status("Dictation cancelled");
                    } else if self.state.borrow().mode != Mode::Root {
                        self.enter_mode(Mode::Root);
                    } else {
                        self.hide();
                    }
                    Propagation::Stop
                }
                Key::Down | Key::Tab if self.state.borrow().editing_note.is_none() => {
                    self.move_selection(1);
                    Propagation::Stop
                }
                Key::Page_Down if self.state.borrow().editing_note.is_none() => {
                    self.move_selection(8);
                    Propagation::Stop
                }
                Key::Up | Key::ISO_Left_Tab if self.state.borrow().editing_note.is_none() => {
                    self.move_selection(-1);
                    Propagation::Stop
                }
                Key::Page_Up if self.state.borrow().editing_note.is_none() => {
                    self.move_selection(-8);
                    Propagation::Stop
                }
                Key::Return | Key::KP_Enter => {
                    if self.state.borrow().editing_note.is_some() {
                        Propagation::Proceed
                    } else if self.state.borrow().voice.state() != voice::State::Idle {
                        self.toggle_voice();
                        Propagation::Stop
                    } else {
                        self.activate();
                        Propagation::Stop
                    }
                }
                Key::BackSpace if ctrl => {
                    if self.state.borrow().mode != Mode::Root {
                        self.enter_mode(Mode::Root);
                        Propagation::Stop
                    } else {
                        Propagation::Proceed
                    }
                }
                _ => Propagation::Proceed,
            }
        }
    }

    fn move_selection(&self, delta: i32) {
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
        match item.action {
            Action::EnterMode(mode) => self.enter_mode(mode),
            Action::SaveSnippet { keyword } => {
                if let Some(text) = clipboard::current_text() {
                    snippets::upsert(&keyword, &text);
                    self.set_status(format!("Saved snippet {keyword}"));
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

    fn enter_mode(&self, mode: Mode) {
        self.commit_note();
        self.state.borrow_mut().editing_note = None;
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
        self.state.borrow_mut().editing_note = Some(note.id.clone());
        self.state.borrow_mut().mode = Mode::Notes;
        self.entry.set_text(&note.title);
        self.entry.set_position(-1);
        self.detail_view.buffer().set_text(&note.body);
        self.set_status("Editing note · Ctrl+S saves, Esc returns");
        self.sync_chrome();
        self.detail_view.grab_focus();
    }

    fn commit_note(&self) {
        let id = self.state.borrow().editing_note.clone();
        let Some(id) = id else {
            return;
        };
        let title = self.entry.text().to_string();
        let buffer = self.detail_view.buffer();
        let start = buffer.start_iter();
        let end = buffer.end_iter();
        let body = buffer.text(&start, &end, false).to_string();
        notes::save_body(&id, &title, &body);
        self.state.borrow_mut().editing_note = None;
        self.detail.set_visible(false);
    }

    fn run_ask(&self, prompt: &str) {
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        let mut prompt = prompt.to_string();
        if settings.general.attach_clipboard_to_ai
            && let Some(clip) = clipboard::current_text()
        {
            if clipboard::looks_secret(&clip) {
                prompt = format!("{prompt}\n\nClipboard: [redacted — looked like a secret]");
            } else {
                prompt = format!("{prompt}\n\nClipboard:\n{clip}");
            }
        }
        self.set_status(format!("Thinking with {}…", settings.ai.model));
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(ai::ask(&prompt, &settings));
        });
        let shell = self.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
            match rx.try_recv() {
                Ok(Ok(reply)) => {
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
            st.results = vec![Scored {
                item,
                score: 100_000,
            }];
            st.selected = 0;
            st.mode = Mode::Ask;
        }
        self.detail_view.buffer().set_text(&text);
        self.detail.set_visible(true);
        self.sync_chrome();
        rebuild_rows(self);
    }

    fn toggle_voice(&self) {
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        let state = self.state.borrow().voice.state();
        match state {
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
            voice::State::Idle => match self.state.borrow().voice.start(&settings) {
                Ok(()) => {
                    self.set_status("Listening… Enter fills the search box");
                    self.window.present();
                    self.entry.grab_focus();
                }
                Err(err) => self.set_status(err),
            },
        }
    }

    fn apply_transcript(&self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.set_status("No speech detected");
            return;
        }
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
        self.set_status("Dictation captured · edit, then Enter");
        self.refresh();
    }

    fn sign_in(&self, provider: &str) {
        let settings = self.state.borrow().catalog.settings.borrow().clone();
        let provider = provider.to_string();
        self.set_status(format!("Opening {provider} sign-in in your browser…"));
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(auth::login(&provider, &settings));
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
                    shell.set_status("Sign-in stopped");
                    gtk4::glib::ControlFlow::Break
                }
            }
        });
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

fn rebuild_rows(shell: &Shell) {
    let host = &shell.results_host;
    while let Some(child) = host.last_child() {
        host.remove(&child);
    }

    {
        let mut st = shell.state.borrow_mut();
        st.rows.clear();
        if st.editing_note.is_some() {
            return;
        }
        if st.results.is_empty() {
            host.append(&shell.empty);
            return;
        }
        let selected = st.selected;
        let items: Vec<Item> = st.results.iter().map(|s| s.item.clone()).collect();
        for (idx, item) in items.into_iter().enumerate() {
            let row = result_row(&item, idx == selected);
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

fn result_hint(state: &State) -> Option<String> {
    let n = state.results.len();
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

fn result_row(item: &Item, selected: bool) -> Box {
    let row = Box::new(Orientation::Horizontal, 8);
    row.add_css_class("row");
    if selected {
        row.add_css_class("selected");
    }
    row.set_hexpand(true);

    let accent = Box::new(Orientation::Vertical, 0);
    accent.add_css_class("accent");
    accent.set_valign(Align::Center);
    row.append(&accent);

    let icon = match &item.icon {
        Icon::Name(name) => Image::from_icon_name(name),
        Icon::Path(path) => Image::from_file(path),
        Icon::None => Image::from_icon_name(kind_icon(item.kind)),
    };
    icon.set_pixel_size(28);
    icon.set_valign(Align::Center);
    icon.add_css_class("icon-wrap");
    row.append(&icon);

    let text = Box::new(Orientation::Vertical, 1);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let title = Label::new(Some(&item.title));
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.add_css_class(if item.kind == Kind::Calc {
        "calc-title"
    } else {
        "title"
    });
    text.append(&title);

    if !item.subtitle.is_empty() {
        let sub = Label::new(Some(&item.subtitle));
        sub.set_xalign(0.0);
        sub.set_ellipsize(pango::EllipsizeMode::End);
        sub.add_css_class("subtitle");
        text.append(&sub);
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
    for hint in ["file", "?ask", "note", "win", "clip", "store", "set"] {
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
    bar.append(&hint_pair("⌘,", "settings"));
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
