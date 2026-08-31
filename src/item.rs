use std::path::PathBuf;

use crate::mode::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    App,
    Window,
    Command,
    Calc,
    File,
    Web,
    Clipboard,
    Shell,
    Snippet,
    Extension,
    Note,
    Ai,
    Voice,
    Settings,
    Store,
    Script,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::App => "APP",
            Kind::Window => "WINDOW",
            Kind::Command => "CMD",
            Kind::Calc => "CALC",
            Kind::File => "FILE",
            Kind::Web => "WEB",
            Kind::Clipboard => "CLIP",
            Kind::Shell => "RUN",
            Kind::Snippet => "SNIP",
            Kind::Extension => "EXT",
            Kind::Note => "NOTE",
            Kind::Ai => "AI",
            Kind::Voice => "VOICE",
            Kind::Settings => "SET",
            Kind::Store => "STORE",
            Kind::Script => "SCRIPT",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    LaunchDesktop { path: PathBuf },
    FocusWindow { address: String },
    Copy(String),
    Paste(String),
    OpenUri(String),
    OpenPath(PathBuf),
    Spawn { program: String, args: Vec<String> },
    Shell { command: String, terminal: bool },
    EnterMode(Mode),
    SaveSnippet { keyword: String },
    CreateNote { title: String },
    OpenNote { id: String },
    AskAi { prompt: String },
    ToggleVoice,
    SaveSettings,
    InstallExt { id: String },
    SyncScriptCommands,
    SyncVicinae,
    RunScript { path: PathBuf },
    UseModel {
        source: String,
        model: String,
        endpoint: String,
        api: String,
    },
    SignIn { provider: String },
    SignOut,
    RefreshModels,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub keywords: String,
    pub kind: Kind,
    pub icon: Icon,
    pub action: Action,
}

#[derive(Debug, Clone)]
pub enum Icon {
    Name(String),
    Path(PathBuf),
    None,
}

impl Item {
    pub fn haystack(&self) -> String {
        format!(
            "{} {} {} {}",
            self.title, self.subtitle, self.keywords, self.kind.label()
        )
    }
}
