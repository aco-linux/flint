mod action;
mod ai;
mod alias;
mod auth;
mod calc;
mod capture;
mod catalog;
mod clipboard;
mod config;
mod content;
mod db;
mod desktop;
mod emoji;
mod extension;
mod favorites;
mod files;
mod hypr;
mod intent;
mod item;
mod layout;
mod mcp;
mod mode;
mod models;
mod notes;
mod ocr;
mod paths;
mod pkg;
mod placeholder;
mod preview;
mod quicklinks;
mod quit;
mod scripts;
mod smart;
mod snippets;
mod store;
mod translate;
mod tz;
mod ui;
mod usage;
mod voice;
mod weather;

use std::cell::RefCell;
use std::ffi::OsString;
use std::rc::Rc;

use gtk4::gio::prelude::*;
use gtk4::glib::{OptionArg, OptionFlags};
use gtk4::{Application, gio};

use catalog::Catalog;
use config::Settings;
use mode::Mode;

pub(crate) const APP_ID: &str = "dev.flint.launcher";
pub(crate) const WINDOW_WIDTH: i32 = 980;
pub(crate) const WINDOW_HEIGHT: i32 = 720;

#[derive(Debug)]
enum Cmd {
    Toggle,
    Daemon,
    Mode(Mode),
    Quit,
}

fn main() {
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    register_cli(&app);

    let clips = Rc::new(RefCell::new(clipboard::Store::load()));
    let settings = Rc::new(RefCell::new(Settings::load()));
    let shell: Rc<RefCell<Option<ui::Shell>>> = Rc::new(RefCell::new(None));
    let hold: Rc<RefCell<Option<gio::ApplicationHoldGuard>>> = Rc::new(RefCell::new(None));

    app.connect_command_line(move |app, cmdline| {
        let cmd = parse_command_line(cmdline);
        if shell.borrow().is_none() {
            paths::ensure();
            clipboard::watch(clips.clone());
            std::thread::spawn(models::warm);
            let catalog = Catalog::load(clips.clone(), settings.clone());
            *shell.borrow_mut() = Some(ui::build(app, catalog));
        }
        if hold.borrow().is_none() {
            *hold.borrow_mut() = Some(app.hold());
        }
        match cmd {
            Cmd::Quit => {
                app.quit();
            }
            other => {
                if let Some(ui) = shell.borrow().as_ref() {
                    match other {
                        Cmd::Daemon => {}
                        Cmd::Toggle => ui.toggle(),
                        Cmd::Mode(mode) => ui.open(mode),
                        Cmd::Quit => {}
                    }
                }
            }
        }
        0.into()
    });

    app.run();
}

fn register_cli(app: &Application) {
    // Register flags so GApplication/GTK cannot swallow them (notably --windows).
    const FLAGS: &[(&str, &str)] = &[
        ("windows", "Open the window switcher"),
        ("win", "Open the window switcher"),
        ("files", "Open Search Files"),
        ("file", "Open Search Files"),
        ("clipboard", "Open clipboard history"),
        ("clip", "Open clipboard history"),
        ("snippets", "Open snippets"),
        ("snip", "Open snippets"),
        ("notes", "Open notes"),
        ("note", "Open notes"),
        ("ask", "Open Ask AI"),
        ("ai", "Open Ask AI"),
        ("voice", "Open dictation"),
        ("dictate", "Open dictation"),
        ("settings", "Open settings"),
        ("prefs", "Open settings"),
        ("store", "Open the store"),
        ("daemon", "Keep Flint running in the background"),
        ("quit", "Quit the running Flint daemon"),
    ];
    for (name, desc) in FLAGS {
        app.add_main_option(
            name,
            0u8.into(),
            OptionFlags::NONE,
            OptionArg::None,
            desc,
            None,
        );
    }
}

fn parse_command_line(cmdline: &gio::ApplicationCommandLine) -> Cmd {
    let dict = cmdline.options_dict();
    if dict.contains("quit") {
        return Cmd::Quit;
    }
    if dict.contains("daemon") {
        return Cmd::Daemon;
    }
    if dict.contains("windows") || dict.contains("win") {
        return Cmd::Mode(Mode::Windows);
    }
    if dict.contains("files") || dict.contains("file") {
        return Cmd::Mode(Mode::Files);
    }
    if dict.contains("clipboard") || dict.contains("clip") {
        return Cmd::Mode(Mode::Clipboard);
    }
    if dict.contains("snippets") || dict.contains("snip") {
        return Cmd::Mode(Mode::Snippets);
    }
    if dict.contains("notes") || dict.contains("note") {
        return Cmd::Mode(Mode::Notes);
    }
    if dict.contains("ask") || dict.contains("ai") {
        return Cmd::Mode(Mode::Ask);
    }
    if dict.contains("voice") || dict.contains("dictate") {
        return Cmd::Mode(Mode::Voice);
    }
    if dict.contains("settings") || dict.contains("prefs") {
        return Cmd::Mode(Mode::Settings);
    }
    if dict.contains("store") {
        return Cmd::Mode(Mode::Store);
    }
    parse_args(&cmdline.arguments())
}

fn parse_args(args: &[OsString]) -> Cmd {
    // Do not skip argv[0] blindly: D-Bus command lines sometimes omit it,
    // and skipping would drop `--windows` into the void.
    let flags: Vec<String> = args
        .iter()
        .filter_map(|a| a.to_str().map(str::to_string))
        .filter(|a| a.starts_with('-'))
        .collect();
    if flags.iter().any(|a| a == "--quit") {
        return Cmd::Quit;
    }
    if flags.iter().any(|a| a == "--daemon" || a == "-d") {
        return Cmd::Daemon;
    }
    if flags.iter().any(|a| a == "--windows" || a == "--win") {
        return Cmd::Mode(Mode::Windows);
    }
    if flags.iter().any(|a| a == "--files" || a == "--file") {
        return Cmd::Mode(Mode::Files);
    }
    if flags.iter().any(|a| a == "--clipboard" || a == "--clip") {
        return Cmd::Mode(Mode::Clipboard);
    }
    if flags.iter().any(|a| a == "--snippets" || a == "--snip") {
        return Cmd::Mode(Mode::Snippets);
    }
    if flags.iter().any(|a| a == "--notes" || a == "--note") {
        return Cmd::Mode(Mode::Notes);
    }
    if flags.iter().any(|a| a == "--ask" || a == "--ai") {
        return Cmd::Mode(Mode::Ask);
    }
    if flags.iter().any(|a| a == "--voice" || a == "--dictate") {
        return Cmd::Mode(Mode::Voice);
    }
    if flags.iter().any(|a| a == "--settings" || a == "--prefs") {
        return Cmd::Mode(Mode::Settings);
    }
    if flags.iter().any(|a| a == "--store") {
        return Cmd::Mode(Mode::Store);
    }
    Cmd::Toggle
}

#[cfg(test)]
mod tests {
    use super::{Cmd, parse_args};
    use crate::mode::Mode;
    use std::ffi::OsString;

    fn cmd(args: &[&str]) -> Cmd {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        parse_args(&args)
    }

    fn mode(args: &[&str]) -> Mode {
        match cmd(args) {
            Cmd::Mode(mode) => mode,
            other => panic!("expected mode, got flag-parse {other:?}"),
        }
    }

    #[test]
    fn windows_flag_survives_missing_argv0() {
        assert_eq!(mode(&["--windows"]), Mode::Windows);
        assert_eq!(mode(&["flint", "--windows"]), Mode::Windows);
        assert_eq!(mode(&["--win"]), Mode::Windows);
        assert_eq!(mode(&["--files"]), Mode::Files);
    }

    #[test]
    fn other_mode_flags_still_parse() {
        assert_eq!(mode(&["flint", "--clipboard"]), Mode::Clipboard);
        assert_eq!(mode(&["--ask"]), Mode::Ask);
        assert!(matches!(cmd(&["flint"]), Cmd::Toggle));
        assert!(matches!(cmd(&["flint", "--daemon"]), Cmd::Daemon));
    }
}
