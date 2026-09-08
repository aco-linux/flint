//! Dedicated Settings window — connections, plugins, skills, Ask AI.
//!
//! The launcher search list is a shortcut index. This window is the
//! product surface: sidebar, cards, switches, and Connect buttons.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk::Key;
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box, Button, DropDown, Entry, EventControllerKey,
    HeaderBar, Label, Orientation, Overflow, PolicyType, ScrolledWindow, Separator, Stack, Switch,
};

use crate::auth;
use crate::config::{McpServer, Settings};

pub const PAGES: &[(&str, &str)] = &[
    ("general", "General"),
    ("ask", "Ask AI"),
    ("connections", "Connections"),
    ("extensions", "Extensions"),
    ("skills", "Skills"),
    ("mcp", "MCP"),
    ("files", "Files"),
    ("voice", "Voice"),
    ("advanced", "Advanced"),
];

#[derive(Clone)]
pub enum Event {
    SignIn(String),
    ImportGrok,
    SignOut,
    RefreshModels,
    OpenStore,
    SyncVicinae,
    SyncScripts,
    OpenPath(std::path::PathBuf),
}

pub struct Host {
    window: ApplicationWindow,
    stack: Stack,
    status: Label,
    connections: Box,
    extensions: Box,
    skills: Box,
    mcp: Box,
    settings: Rc<RefCell<Settings>>,
    on_event: Rc<dyn Fn(Event)>,
}

impl Clone for Host {
    fn clone(&self) -> Self {
        Self {
            window: self.window.clone(),
            stack: self.stack.clone(),
            status: self.status.clone(),
            connections: self.connections.clone(),
            extensions: self.extensions.clone(),
            skills: self.skills.clone(),
            mcp: self.mcp.clone(),
            settings: self.settings.clone(),
            on_event: self.on_event.clone(),
        }
    }
}

pub fn open(
    app: &Application,
    settings: Rc<RefCell<Settings>>,
    on_event: Rc<dyn Fn(Event)>,
) -> Host {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Flint Settings")
        .default_width(1080)
        .default_height(740)
        .decorated(true)
        .resizable(true)
        .icon_name("flint")
        .css_classes(["flint-root", "prefs-root"])
        .build();
    window.set_hide_on_close(true);

    let titlebar = HeaderBar::new();
    titlebar.add_css_class("flint-titlebar");
    titlebar.set_show_title_buttons(true);
    let title = Label::new(Some("SETTINGS"));
    title.add_css_class("flint-title");
    titlebar.set_title_widget(Some(&title));
    window.set_titlebar(Some(&titlebar));

    let root = Box::new(Orientation::Horizontal, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);

    let rail = Box::new(Orientation::Vertical, 4);
    rail.add_css_class("prefs-rail");
    rail.set_valign(Align::Fill);

    let mark = Label::new(Some("FLINT"));
    mark.add_css_class("prefs-mark");
    mark.set_xalign(0.0);
    rail.append(&mark);
    let lede = Label::new(Some("Configure the launcher"));
    lede.add_css_class("prefs-lede");
    lede.set_xalign(0.0);
    lede.set_wrap(true);
    rail.append(&lede);

    let stack = Stack::new();
    stack.add_css_class("prefs-stack");
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let connections = Box::new(Orientation::Vertical, 12);
    let extensions = Box::new(Orientation::Vertical, 12);
    let skills = Box::new(Orientation::Vertical, 12);
    let mcp = Box::new(Orientation::Vertical, 12);

    let host = Host {
        window: window.clone(),
        stack: stack.clone(),
        status: Label::new(None),
        connections: connections.clone(),
        extensions: extensions.clone(),
        skills: skills.clone(),
        mcp: mcp.clone(),
        settings: settings.clone(),
        on_event: on_event.clone(),
    };
    host.status.add_css_class("prefs-status");
    host.status.set_xalign(0.0);
    host.status.set_wrap(true);
    host.status.set_visible(false);

    stack.add_named(&scroll(page_general(&settings)), Some("general"));
    stack.add_named(&scroll(page_ask(&settings, &host)), Some("ask"));
    stack.add_named(&scroll(connections.clone()), Some("connections"));
    stack.add_named(&scroll(extensions.clone()), Some("extensions"));
    stack.add_named(&scroll(skills.clone()), Some("skills"));
    stack.add_named(&scroll(mcp.clone()), Some("mcp"));
    stack.add_named(&scroll(page_files(&settings, &window)), Some("files"));
    stack.add_named(&scroll(page_voice(&settings)), Some("voice"));
    stack.add_named(&scroll(page_advanced(&settings, &host)), Some("advanced"));

    for (id, title) in PAGES {
        let btn = Button::with_label(title);
        btn.add_css_class("prefs-nav");
        btn.set_halign(Align::Fill);
        let stack_nav = stack.clone();
        let id_owned = (*id).to_string();
        btn.connect_clicked(move |_| {
            stack_nav.set_visible_child_name(&id_owned);
        });
        rail.append(&btn);
    }

    let rail_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(&rail)
        .css_classes(["prefs-rail-scroll"])
        .build();

    let main = Box::new(Orientation::Vertical, 0);
    main.set_hexpand(true);
    main.set_vexpand(true);
    main.append(&host.status);
    main.append(&stack);

    root.append(&rail_scroll);
    root.append(&Separator::new(Orientation::Vertical));
    root.append(&main);
    window.set_child(Some(&root));

    let win = window.clone();
    let keys = EventControllerKey::new();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == Key::Escape {
            win.set_visible(false);
            Propagation::Stop
        } else {
            Propagation::Proceed
        }
    });
    window.add_controller(keys);

    host.rebuild();
    host
}

impl Host {
    pub fn show(&self, page: Option<&str>) {
        if let Some(page) = page.filter(|p| PAGES.iter().any(|(id, _)| *id == *p)) {
            self.stack.set_visible_child_name(page);
        }
        self.rebuild();
        crate::hypr::float_launcher();
        self.window.present();
        self.window.unmaximize();
    }

    pub fn set_status(&self, text: impl Into<String>) {
        let text = text.into();
        if text.is_empty() {
            self.status.set_visible(false);
        } else {
            self.status.set_text(&text);
            self.status.set_visible(true);
        }
    }

    pub fn rebuild(&self) {
        rebuild_box(&self.connections, page_connections(&self.settings, self));
        rebuild_box(&self.extensions, page_extensions(&self.settings, self));
        rebuild_box(&self.skills, page_skills(self));
        rebuild_box(&self.mcp, page_mcp(&self.settings, self));
    }
}

fn rebuild_box(host: &Box, fresh: Box) {
    while let Some(child) = host.first_child() {
        host.remove(&child);
    }
    while let Some(child) = fresh.first_child() {
        fresh.remove(&child);
        host.append(&child);
    }
}

fn scroll(child: Box) -> ScrolledWindow {
    child.add_css_class("prefs-page");
    child.set_hexpand(true);
    ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(&child)
        .hexpand(true)
        .vexpand(true)
        .build()
}

fn page_header(title: &str, blurb: &str) -> Box {
    let col = Box::new(Orientation::Vertical, 6);
    let h = Label::new(Some(title));
    h.add_css_class("prefs-heading");
    h.set_xalign(0.0);
    let p = Label::new(Some(blurb));
    p.add_css_class("prefs-blurb");
    p.set_xalign(0.0);
    p.set_wrap(true);
    col.append(&h);
    col.append(&p);
    col
}

fn make_card() -> Box {
    let card = Box::new(Orientation::Vertical, 10);
    card.add_css_class("prefs-card");
    card.set_overflow(Overflow::Hidden);
    card
}

fn switch_row(title: &str, subtitle: &str, on: bool, on_change: impl Fn(bool) + 'static) -> Box {
    let row = Box::new(Orientation::Horizontal, 12);
    row.add_css_class("prefs-row");
    let text = Box::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    let t = Label::new(Some(title));
    t.add_css_class("prefs-row-title");
    t.set_xalign(0.0);
    let s = Label::new(Some(subtitle));
    s.add_css_class("prefs-row-sub");
    s.set_xalign(0.0);
    s.set_wrap(true);
    text.append(&t);
    text.append(&s);
    let sw = Switch::new();
    sw.set_active(on);
    sw.set_valign(Align::Center);
    sw.connect_state_set(move |_, state| {
        on_change(state);
        Propagation::Proceed
    });
    row.append(&text);
    row.append(&sw);
    row
}

fn field_row(title: &str, value: &str, secret: bool, on_save: impl Fn(String) + 'static) -> Box {
    let on_save = Rc::new(on_save);
    let col = Box::new(Orientation::Vertical, 6);
    let t = Label::new(Some(title));
    t.add_css_class("prefs-row-title");
    t.set_xalign(0.0);
    let row = Box::new(Orientation::Horizontal, 8);
    let entry = Entry::new();
    entry.set_text(value);
    entry.set_hexpand(true);
    entry.set_visibility(!secret);
    if secret {
        entry.set_placeholder_text(Some("••••••••"));
    }
    let save = Button::with_label("Save");
    save.add_css_class("prefs-btn");
    let entry_c = entry.clone();
    let on_c = on_save.clone();
    save.connect_clicked(move |_| {
        on_c(entry_c.text().to_string());
    });
    let entry_e = entry.clone();
    let on_e = on_save.clone();
    entry.connect_activate(move |_| {
        on_e(entry_e.text().to_string());
    });
    row.append(&entry);
    row.append(&save);
    col.append(&t);
    col.append(&row);
    col
}

fn page_general(settings: &Rc<RefCell<Settings>>) -> Box {
    let page = Box::new(Orientation::Vertical, 16);
    page.append(&page_header(
        "General",
        "How Flint behaves when it is sitting on your desktop.",
    ));
    let card = make_card();
    let s = settings.clone();
    card.append(&switch_row(
        "Launch at login",
        "Start the daemon when you sign in to the session.",
        settings.borrow().general.autostart,
        {
            let s = s.clone();
            move |on| {
                s.borrow_mut().general.autostart = on;
                s.borrow().save();
            }
        },
    ));
    card.append(&switch_row(
        "Attach clipboard to Ask AI",
        "Include the current clipboard as context when you ask.",
        settings.borrow().general.attach_clipboard_to_ai,
        {
            let s = s.clone();
            move |on| {
                s.borrow_mut().general.attach_clipboard_to_ai = on;
                s.borrow().save();
            }
        },
    ));
    card.append(&switch_row(
        "Context-aware search",
        "Use the focused Hyprland window (class and title) when Flint opens. Empty query also offers Paste / Search / Ask on clipboard copied in the last 10 seconds. Off stores nothing about the focused app.",
        settings.borrow().general.context_aware,
        {
            let s = s.clone();
            move |on| {
                s.borrow_mut().general.context_aware = on;
                s.borrow().save();
            }
        },
    ));
    let names = [
        "DuckDuckGo HTML",
        "Instant Answer JSON",
        "Off (no web fetch)",
    ];
    let ids = ["ddg-html", "instant", "off"];
    let current = settings.borrow().web.provider.clone();
    let selected = ids.iter().position(|id| *id == current).unwrap_or(0) as u32;
    let drop = DropDown::from_strings(&names);
    drop.set_selected(selected);
    drop.connect_selected_notify({
        let s = s.clone();
        move |dd| {
            let idx = dd.selected() as usize;
            if let Some(id) = ids.get(idx) {
                let _ = s.borrow_mut().apply("set:web", id);
            }
        }
    });
    let lab = Label::new(Some("Web search"));
    lab.add_css_class("prefs-row-title");
    lab.set_xalign(0.0);
    let hint = Label::new(Some(
        "In-app results from DuckDuckGo. searxng and brave are documented fallbacks and currently use DuckDuckGo HTML. Off never leaves this computer.",
    ));
    hint.add_css_class("prefs-row-sub");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    card.append(&lab);
    card.append(&hint);
    card.append(&drop);
    page.append(&card);
    page
}

fn page_ask(settings: &Rc<RefCell<Settings>>, host: &Host) -> Box {
    let page = Box::new(Orientation::Vertical, 16);
    page.append(&page_header(
        "Ask AI",
        "Local models by default. Grok, ChatGPT, and Claude when you connect an account or key.",
    ));
    let card = make_card();
    let names = [
        "Ollama (local)",
        "Grok (xAI)",
        "ChatGPT (OpenAI)",
        "Claude (Anthropic)",
        "Google Gemini",
        "Custom",
    ];
    let ids = ["ollama", "xai", "openai", "anthropic", "google", "custom"];
    let current = settings.borrow().ai.provider.clone();
    let selected = ids.iter().position(|id| *id == current).unwrap_or(0) as u32;
    let drop = DropDown::from_strings(&names);
    drop.set_selected(selected);
    let s = settings.clone();
    drop.connect_selected_notify(move |dd| {
        let idx = dd.selected() as usize;
        if let Some(id) = ids.get(idx) {
            s.borrow_mut().set_provider(id);
        }
    });
    let lab = Label::new(Some("Provider"));
    lab.add_css_class("prefs-row-title");
    lab.set_xalign(0.0);
    card.append(&lab);
    card.append(&drop);

    let model = settings.borrow().ai.model.clone();
    let endpoint = settings.borrow().ai.endpoint.clone();
    card.append(&field_row("Model", &model, false, {
        let s = settings.clone();
        move |v| {
            let _ = s.borrow_mut().apply("set:model", &v);
        }
    }));
    card.append(&field_row("Endpoint", &endpoint, false, {
        let s = settings.clone();
        move |v| {
            let _ = s.borrow_mut().apply("set:endpoint", &v);
        }
    }));
    card.append(&field_row("API key", "", true, {
        let s = settings.clone();
        let host = host.clone();
        move |v| {
            let provider = s.borrow().ai.provider.clone();
            match crate::auth::save_api_key(&provider, v.trim()) {
                Ok(storage) => host.set_status(format!("API key saved in {storage}")),
                Err(err) => host.set_status(err),
            }
        }
    }));
    let scan = Button::with_label("Scan local models");
    scan.add_css_class("prefs-btn-primary");
    let on = host.on_event.clone();
    scan.connect_clicked(move |_| on(Event::RefreshModels));
    card.append(&scan);
    page.append(&card);
    page
}

struct Conn {
    id: &'static str,
    title: &'static str,
    blurb: &'static str,
    action: ConnAction,
}

#[derive(Clone, Copy)]
enum ConnAction {
    Xai,
    Oauth(&'static str),
    ApiKey(&'static str),
    Caldav(&'static str),
    Obsidian,
}

fn connections() -> &'static [Conn] {
    &[
        Conn {
            id: "xai",
            title: "Grok (xAI)",
            blurb: "Your SuperGrok subscription. Opens the browser and confirms a device code.",
            action: ConnAction::Xai,
        },
        Conn {
            id: "openai",
            title: "ChatGPT / OpenAI",
            blurb: "API key from platform.openai.com. ChatGPT Plus is not an API login.",
            action: ConnAction::ApiKey("openai"),
        },
        Conn {
            id: "anthropic",
            title: "Claude",
            blurb: "API key from console.anthropic.com. Claude Pro is not third-party OAuth.",
            action: ConnAction::ApiKey("anthropic"),
        },
        Conn {
            id: "google",
            title: "Google Gemini",
            blurb: "Desktop OAuth client ID in Advanced, then Connect.",
            action: ConnAction::Oauth("google"),
        },
        Conn {
            id: "google-calendar",
            title: "Google Calendar",
            blurb: "PKCE OAuth. Uses the Google client ID from Advanced.",
            action: ConnAction::Oauth("google-calendar"),
        },
        Conn {
            id: "outlook",
            title: "Outlook / Microsoft 365",
            blurb: "Azure app client ID in Advanced, then Connect.",
            action: ConnAction::Oauth("outlook"),
        },
        Conn {
            id: "notion",
            title: "Notion",
            blurb: "OAuth client ID and secret. Connect opens the Notion consent page.",
            action: ConnAction::Oauth("notion"),
        },
        Conn {
            id: "todoist",
            title: "Todoist",
            blurb: "OAuth client ID and secret from the Todoist App Management console.",
            action: ConnAction::Oauth("todoist"),
        },
        Conn {
            id: "obsidian",
            title: "Obsidian",
            blurb: "Local vault folder. Flint searches it with the rest of your files.",
            action: ConnAction::Obsidian,
        },
        Conn {
            id: "apple-calendar",
            title: "Apple Calendar",
            blurb: "App-specific password + Apple ID. iCloud has no Linux OAuth for CalDAV.",
            action: ConnAction::Caldav("apple-calendar"),
        },
        Conn {
            id: "proton-calendar",
            title: "Proton Calendar",
            blurb: "CalDAV URL and app password if Proton enabled CalDAV on the account.",
            action: ConnAction::Caldav("proton-calendar"),
        },
    ]
}

fn conn_status(id: &str, settings: &Settings) -> (bool, String) {
    if id == "obsidian" {
        let vault = settings.connectors.obsidian_vault.trim();
        if vault.is_empty() {
            return (false, "No vault selected".into());
        }
        return (true, vault.into());
    }
    if let Some(tokens) = auth::load_for(id) {
        let label = if tokens.account.is_empty() {
            format!("Connected · {}", tokens.provider)
        } else {
            format!("Connected as {}", tokens.account)
        };
        return (true, label);
    }
    if auth::has_api_key(id) {
        return (true, "Credential saved in the keyring".into());
    }
    (false, "Not connected".into())
}

fn page_connections(settings: &Rc<RefCell<Settings>>, host: &Host) -> Box {
    let page = Box::new(Orientation::Vertical, 16);
    page.append(&page_header(
        "Connections",
        "Sign in the way each product actually allows — OAuth where it exists, a key or vault where it does not.",
    ));
    let snap = settings.borrow().clone();
    for conn in connections() {
        page.append(&connection_card(conn, &snap, settings, host));
    }
    page
}

fn connection_card(
    conn: &Conn,
    snap: &Settings,
    settings: &Rc<RefCell<Settings>>,
    host: &Host,
) -> Box {
    let (on, status) = conn_status(conn.id, snap);
    let card = make_card();
    let head = Box::new(Orientation::Horizontal, 12);
    let text = Box::new(Orientation::Vertical, 3);
    text.set_hexpand(true);
    let title = Label::new(Some(conn.title));
    title.add_css_class("prefs-card-title");
    title.set_xalign(0.0);
    let blurb = Label::new(Some(conn.blurb));
    blurb.add_css_class("prefs-row-sub");
    blurb.set_xalign(0.0);
    blurb.set_wrap(true);
    let st = Label::new(Some(&status));
    st.add_css_class(if on {
        "prefs-pill-on"
    } else {
        "prefs-pill-off"
    });
    st.set_xalign(0.0);
    text.append(&title);
    text.append(&blurb);
    text.append(&st);
    head.append(&text);

    let actions = Box::new(Orientation::Horizontal, 6);
    actions.set_valign(Align::Start);
    match conn.action {
        ConnAction::Xai => {
            let connect = Button::with_label(if on { "Reconnect" } else { "Connect" });
            connect.add_css_class("prefs-btn-primary");
            let on_ev = host.on_event.clone();
            connect.connect_clicked(move |_| on_ev(Event::SignIn("xai".into())));
            actions.append(&connect);
            let import = Button::with_label("Use Grok CLI");
            import.add_css_class("prefs-btn");
            let on_ev = host.on_event.clone();
            import.connect_clicked(move |_| on_ev(Event::ImportGrok));
            actions.append(&import);
        }
        ConnAction::Oauth(provider) => {
            let connect = Button::with_label(if on { "Reconnect" } else { "Connect" });
            connect.add_css_class("prefs-btn-primary");
            let on_ev = host.on_event.clone();
            let provider = provider.to_string();
            connect.connect_clicked(move |_| on_ev(Event::SignIn(provider.clone())));
            actions.append(&connect);
        }
        ConnAction::ApiKey(provider) => {
            let entry = Entry::new();
            entry.set_placeholder_text(Some("API key"));
            entry.set_visibility(false);
            entry.set_width_chars(18);
            let save = Button::with_label("Save key");
            save.add_css_class("prefs-btn-primary");
            let provider = provider.to_string();
            let settings = settings.clone();
            let host_c = host.clone();
            let entry_c = entry.clone();
            let provider_c = provider.clone();
            save.connect_clicked(move |_| {
                let value = entry_c.text().to_string();
                match auth::save_api_key(&provider_c, value.trim()) {
                    Ok(storage) => {
                        settings.borrow_mut().set_provider(&provider_c);
                        host_c.set_status(format!("Saved in {storage}"));
                        host_c.rebuild();
                    }
                    Err(err) => host_c.set_status(err),
                }
            });
            actions.append(&entry);
            actions.append(&save);
        }
        ConnAction::Caldav(provider) => {
            let connect = Button::with_label("Connect");
            connect.add_css_class("prefs-btn-primary");
            let on_ev = host.on_event.clone();
            let provider = provider.to_string();
            connect.connect_clicked(move |_| on_ev(Event::SignIn(provider.clone())));
            actions.append(&connect);
        }
        ConnAction::Obsidian => {
            let pick = Button::with_label(if on { "Change vault" } else { "Choose vault" });
            pick.add_css_class("prefs-btn-primary");
            let settings = settings.clone();
            let host_c = host.clone();
            let window = host.window.clone();
            pick.connect_clicked(move |_| {
                pick_folder(&window, {
                    let settings = settings.clone();
                    let host_c = host_c.clone();
                    move |path| {
                        let _ = settings
                            .borrow_mut()
                            .apply("set:obsidian-vault", &path.to_string_lossy());
                        host_c.set_status("Obsidian vault connected");
                        host_c.rebuild();
                    }
                });
            });
            actions.append(&pick);
        }
    }
    head.append(&actions);
    card.append(&head);
    card
}

fn pick_folder(window: &ApplicationWindow, on_pick: impl Fn(std::path::PathBuf) + 'static) {
    let dialog = gtk4::FileChooserNative::new(
        Some("Choose folder"),
        Some(window),
        gtk4::FileChooserAction::SelectFolder,
        Some("Select"),
        Some("Cancel"),
    );
    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Accept
            && let Some(file) = gtk4::prelude::FileChooserExt::file(dialog)
            && let Some(path) = file.path()
        {
            on_pick(path);
        }
        dialog.destroy();
    });
    dialog.show();
    std::mem::forget(dialog);
}

fn page_extensions(settings: &Rc<RefCell<Settings>>, host: &Host) -> Box {
    let page = Box::new(Orientation::Vertical, 16);
    page.append(&page_header(
        "Extensions",
        "Vicinae / Raycast-style plugins run as Node with your user privileges. Off until you opt in.",
    ));
    let card = make_card();
    let s = settings.clone();
    card.append(&switch_row(
        "Run installed extensions",
        "Unsigned. They can do anything your user can.",
        settings.borrow().general.allow_extensions,
        {
            let s = s.clone();
            move |on| {
                s.borrow_mut().general.allow_extensions = on;
                s.borrow().save();
            }
        },
    ));
    card.append(&switch_row(
        "Run unsigned script-commands",
        "sh / python3 / node from the Store folder.",
        settings.borrow().general.allow_script_commands,
        {
            let s = s.clone();
            move |on| {
                s.borrow_mut().general.allow_script_commands = on;
                s.borrow().save();
            }
        },
    ));
    let row = Box::new(Orientation::Horizontal, 8);
    let store = Button::with_label("Open Store");
    store.add_css_class("prefs-btn-primary");
    let on = host.on_event.clone();
    store.connect_clicked(move |_| on(Event::OpenStore));
    let sync = Button::with_label("Refresh Vicinae catalog");
    sync.add_css_class("prefs-btn");
    let on = host.on_event.clone();
    sync.connect_clicked(move |_| on(Event::SyncVicinae));
    let scripts = Button::with_label("Sync script-commands");
    scripts.add_css_class("prefs-btn");
    let on = host.on_event.clone();
    scripts.connect_clicked(move |_| on(Event::SyncScripts));
    row.append(&store);
    row.append(&sync);
    row.append(&scripts);
    card.append(&row);
    page.append(&card);

    let installed = crate::extension::installed();
    let list = make_card();
    if installed.is_empty() {
        let empty = Label::new(Some(
            "No plugins installed yet. Open the Store to add some.",
        ));
        empty.add_css_class("prefs-row-sub");
        empty.set_xalign(0.0);
        list.append(&empty);
    } else {
        for ext in installed {
            let title = if ext.manifest.title.is_empty() {
                ext.manifest.name.clone()
            } else {
                ext.manifest.title.clone()
            };
            let n = ext.manifest.commands.len();
            list.append(&plain_row(
                &title,
                &format!(
                    "{n} command{} · {}",
                    if n == 1 { "" } else { "s" },
                    ext.dir.display()
                ),
            ));
        }
    }
    page.append(&list);
    page
}

fn page_skills(host: &Host) -> Box {
    let page = Box::new(Orientation::Vertical, 16);
    page.append(&page_header(
        "Skills",
        "Markdown files in ~/.config/flint/skills that Ask AI reads as standing instructions.",
    ));
    let card = make_card();
    let row = Box::new(Orientation::Horizontal, 8);
    let open = Button::with_label("Open skills folder");
    open.add_css_class("prefs-btn-primary");
    let on = host.on_event.clone();
    open.connect_clicked(move |_| {
        let dir = crate::skills::dir();
        let _ = std::fs::create_dir_all(&dir);
        on(Event::OpenPath(dir));
    });
    let new = Button::with_label("New skill");
    new.add_css_class("prefs-btn");
    let on = host.on_event.clone();
    let host_c = host.clone();
    new.connect_clicked(move |_| {
        let dir = crate::skills::dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("new-skill.md");
        if !path.exists() {
            let _ = std::fs::write(
                &path,
                "# New skill\n\nDescribe when this applies and what Flint should do.\n",
            );
        }
        on(Event::OpenPath(path));
        host_c.rebuild();
    });
    row.append(&open);
    row.append(&new);
    card.append(&row);
    page.append(&card);

    let list = make_card();
    let skills = crate::skills::list();
    if skills.is_empty() {
        let empty = Label::new(Some("No skills yet. New skill creates a markdown file."));
        empty.add_css_class("prefs-row-sub");
        empty.set_xalign(0.0);
        list.append(&empty);
    } else {
        for skill in skills {
            let host_c = host.clone();
            let path = skill.path.clone();
            let row = Box::new(Orientation::Horizontal, 8);
            row.add_css_class("prefs-row");
            let text = Box::new(Orientation::Vertical, 2);
            text.set_hexpand(true);
            let t = Label::new(Some(&skill.name));
            t.add_css_class("prefs-row-title");
            t.set_xalign(0.0);
            let s = Label::new(Some(&skill.preview));
            s.add_css_class("prefs-row-sub");
            s.set_xalign(0.0);
            text.append(&t);
            text.append(&s);
            let open = Button::with_label("Edit");
            open.add_css_class("prefs-btn");
            let on = host_c.on_event.clone();
            open.connect_clicked(move |_| on(Event::OpenPath(path.clone())));
            row.append(&text);
            row.append(&open);
            list.append(&row);
        }
    }
    page.append(&list);
    page
}

fn page_mcp(settings: &Rc<RefCell<Settings>>, host: &Host) -> Box {
    let page = Box::new(Orientation::Vertical, 16);
    page.append(&page_header(
        "MCP",
        "Model Context Protocol servers Flint may spawn to list tools for Ask AI. npx only, off until you opt in.",
    ));
    let card = make_card();
    let s = settings.clone();
    card.append(&switch_row(
        "Allow MCP tool listing",
        "Flint will spawn the servers below when you Ask.",
        settings.borrow().general.allow_mcp,
        move |on| {
            s.borrow_mut().general.allow_mcp = on;
            s.borrow().save();
        },
    ));
    page.append(&card);

    let add = make_card();
    add.append(&plain_row(
        "Add a server",
        "Name, command npx, and arguments. Example: -y @modelcontextprotocol/server-memory",
    ));
    let name = Entry::new();
    name.set_placeholder_text(Some("Name"));
    let command = Entry::new();
    command.set_text("npx");
    let args = Entry::new();
    args.set_placeholder_text(Some("Args, space-separated"));
    let go = Button::with_label("Add");
    go.add_css_class("prefs-btn-primary");
    let settings_c = settings.clone();
    let host_c = host.clone();
    let name_c = name.clone();
    let command_c = command.clone();
    let args_c = args.clone();
    go.connect_clicked(move |_| {
        let argv: Vec<String> = args_c
            .text()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        match settings_c
            .borrow_mut()
            .add_mcp(&name_c.text(), &command_c.text(), argv)
        {
            Ok(()) => {
                host_c.set_status("MCP server added");
                host_c.rebuild();
            }
            Err(err) => host_c.set_status(err),
        }
    });
    add.append(&name);
    add.append(&command);
    add.append(&args);
    add.append(&go);
    page.append(&add);

    let list = make_card();
    let servers: Vec<McpServer> = settings.borrow().mcp.clone();
    if servers.is_empty() {
        let empty = Label::new(Some("No MCP servers configured."));
        empty.add_css_class("prefs-row-sub");
        empty.set_xalign(0.0);
        list.append(&empty);
    } else {
        for server in servers {
            let row = Box::new(Orientation::Horizontal, 8);
            row.add_css_class("prefs-row");
            let text = Box::new(Orientation::Vertical, 2);
            text.set_hexpand(true);
            let t = Label::new(Some(&server.name));
            t.add_css_class("prefs-row-title");
            t.set_xalign(0.0);
            let sub = format!("{} {}", server.command, server.args.join(" "));
            let s = Label::new(Some(&sub));
            s.add_css_class("prefs-row-sub");
            s.set_xalign(0.0);
            text.append(&t);
            text.append(&s);
            let sw = Switch::new();
            sw.set_active(server.enabled);
            sw.set_valign(Align::Center);
            let settings_sw = settings.clone();
            let name_sw = server.name.clone();
            sw.connect_state_set(move |_, on| {
                settings_sw.borrow_mut().set_mcp_enabled(&name_sw, on);
                Propagation::Proceed
            });
            let remove = Button::with_label("Remove");
            remove.add_css_class("prefs-btn");
            let settings_rm = settings.clone();
            let host_c = host.clone();
            let name = server.name.clone();
            remove.connect_clicked(move |_| {
                settings_rm.borrow_mut().remove_mcp(&name);
                host_c.rebuild();
            });
            row.append(&text);
            row.append(&sw);
            row.append(&remove);
            list.append(&row);
        }
    }
    page.append(&list);
    page
}

fn page_files(settings: &Rc<RefCell<Settings>>, window: &ApplicationWindow) -> Box {
    let page = Box::new(Orientation::Vertical, 16);
    page.append(&page_header(
        "Files",
        "Where Flint looks when you type a filename. Home is always included.",
    ));
    let card = make_card();
    let s = settings.clone();
    card.append(&switch_row(
        "Files in root search",
        "Show file hits next to apps in the empty-prefix launcher.",
        settings.borrow().files.include_in_root,
        {
            let s = s.clone();
            move |on| {
                s.borrow_mut().files.include_in_root = on;
                s.borrow().save();
            }
        },
    ));
    card.append(&switch_row(
        "System-wide search",
        "Use locate/plocate so type queries can see files outside $HOME.",
        settings.borrow().files.system_wide,
        {
            let s = s.clone();
            move |on| {
                s.borrow_mut().files.system_wide = on;
                s.borrow().save();
            }
        },
    ));
    card.append(&switch_row(
        "Include hidden files",
        "Dotfiles in Search Files.",
        settings.borrow().files.include_hidden,
        {
            let s = s.clone();
            move |on| {
                s.borrow_mut().files.include_hidden = on;
                s.borrow().save();
            }
        },
    ));
    page.append(&card);

    let roots = make_card();
    roots.append(&plain_row(
        "Extra folders",
        "External drives, project roots, Obsidian vaults.",
    ));
    for root in settings.borrow().files.search_roots.clone() {
        roots.append(&plain_row(&root, "Extra search root"));
    }
    let add = Button::with_label("Add folder");
    add.add_css_class("prefs-btn-primary");
    let settings = settings.clone();
    let window = window.clone();
    add.connect_clicked(move |_| {
        pick_folder(&window, {
            let settings = settings.clone();
            move |path| {
                let p = path.to_string_lossy().into_owned();
                let mut s = settings.borrow_mut();
                if !s.files.search_roots.iter().any(|r| r == &p) {
                    s.files.search_roots.push(p);
                    s.save();
                }
            }
        });
    });
    roots.append(&add);
    page.append(&roots);
    page
}

fn page_voice(settings: &Rc<RefCell<Settings>>) -> Box {
    let page = Box::new(Orientation::Vertical, 16);
    page.append(&page_header(
        "Voice",
        "In-bar dictation uses pw-record and voxtype on this machine. Nothing is uploaded.",
    ));
    let card = make_card();
    let lang = settings.borrow().voice.language.clone();
    let model = settings.borrow().voice.model.clone();
    card.append(&field_row("Language", &lang, false, {
        let s = settings.clone();
        move |v| {
            let _ = s.borrow_mut().apply("set:voice-lang", &v);
        }
    }));
    card.append(&field_row("Model", &model, false, {
        let s = settings.clone();
        move |v| {
            let _ = s.borrow_mut().apply("set:voice-model", &v);
        }
    }));
    page.append(&card);
    page
}

fn page_advanced(settings: &Rc<RefCell<Settings>>, host: &Host) -> Box {
    let page = Box::new(Orientation::Vertical, 16);
    page.append(&page_header(
        "Advanced",
        "OAuth client IDs, CalDAV, and the config file. Secrets go in the keyring.",
    ));
    let card = make_card();
    let client = settings.borrow().ai.client_id.clone();
    card.append(&field_row(
        "Google / custom OAuth client ID",
        &client,
        false,
        {
            let s = settings.clone();
            move |v| {
                let _ = s.borrow_mut().apply("set:client-id", &v);
            }
        },
    ));
    let outlook = settings.borrow().connectors.outlook_client_id.clone();
    card.append(&field_row("Outlook client ID", &outlook, false, {
        let s = settings.clone();
        move |v| {
            let _ = s.borrow_mut().apply("set:outlook-client", &v);
        }
    }));
    let notion = settings.borrow().connectors.notion_client_id.clone();
    card.append(&field_row("Notion client ID", &notion, false, {
        let s = settings.clone();
        move |v| {
            let _ = s.borrow_mut().apply("set:notion-client", &v);
        }
    }));
    card.append(&field_row("Notion client secret", "", true, {
        let host = host.clone();
        move |v| match auth::save_api_key("notion-secret", v.trim()) {
            Ok(storage) => host.set_status(format!("Saved in {storage}")),
            Err(err) => host.set_status(err),
        }
    }));
    let todoist = settings.borrow().connectors.todoist_client_id.clone();
    card.append(&field_row("Todoist client ID", &todoist, false, {
        let s = settings.clone();
        move |v| {
            let _ = s.borrow_mut().apply("set:todoist-client", &v);
        }
    }));
    card.append(&field_row("Todoist client secret", "", true, {
        let host = host.clone();
        move |v| match auth::save_api_key("todoist-secret", v.trim()) {
            Ok(storage) => host.set_status(format!("Saved in {storage}")),
            Err(err) => host.set_status(err),
        }
    }));
    let apple = settings.borrow().connectors.apple_id.clone();
    card.append(&field_row("Apple ID", &apple, false, {
        let s = settings.clone();
        move |v| {
            let _ = s.borrow_mut().apply("set:apple-id", &v);
        }
    }));
    card.append(&field_row("Apple app-specific password", "", true, {
        let host = host.clone();
        move |v| match auth::save_api_key("apple-calendar", v.trim()) {
            Ok(storage) => host.set_status(format!("Saved in {storage}")),
            Err(err) => host.set_status(err),
        }
    }));
    let caldav = settings.borrow().connectors.caldav_url.clone();
    card.append(&field_row("CalDAV URL", &caldav, false, {
        let s = settings.clone();
        move |v| {
            let _ = s.borrow_mut().apply("set:caldav-url", &v);
        }
    }));
    page.append(&card);

    let danger = make_card();
    let open = Button::with_label("Open config.json");
    open.add_css_class("prefs-btn");
    let on = host.on_event.clone();
    open.connect_clicked(move |_| on(Event::OpenPath(crate::config::path())));
    let out = Button::with_label("Sign out of all accounts");
    out.add_css_class("prefs-btn");
    let on = host.on_event.clone();
    out.connect_clicked(move |_| on(Event::SignOut));
    danger.append(&open);
    danger.append(&out);
    page.append(&danger);
    page
}

fn plain_row(title: &str, subtitle: &str) -> Box {
    let text = Box::new(Orientation::Vertical, 2);
    text.add_css_class("prefs-row");
    let t = Label::new(Some(title));
    t.add_css_class("prefs-row-title");
    t.set_xalign(0.0);
    let s = Label::new(Some(subtitle));
    s.add_css_class("prefs-row-sub");
    s.set_xalign(0.0);
    s.set_wrap(true);
    text.append(&t);
    text.append(&s);
    text
}

#[cfg(test)]
mod tests {
    use super::{PAGES, connections};
    use crate::item::Action;

    #[test]
    fn settings_window_has_the_product_pages() {
        let ids: Vec<_> = PAGES.iter().map(|(id, _)| *id).collect();
        for need in [
            "general",
            "ask",
            "connections",
            "extensions",
            "skills",
            "mcp",
            "files",
        ] {
            assert!(ids.contains(&need), "missing page {need}");
        }
    }

    #[test]
    fn connections_cover_the_accounts_people_ask_for() {
        let ids: Vec<_> = connections().iter().map(|c| c.id).collect();
        for need in [
            "xai",
            "openai",
            "anthropic",
            "google-calendar",
            "notion",
            "todoist",
            "obsidian",
            "apple-calendar",
        ] {
            assert!(ids.contains(&need), "missing connection {need}");
        }
    }

    #[test]
    fn open_prefs_action_exists() {
        let action = Action::OpenPrefs {
            page: Some("connections".into()),
        };
        match action {
            Action::OpenPrefs { page } => assert_eq!(page.as_deref(), Some("connections")),
            _ => panic!("expected OpenPrefs"),
        }
    }
}
