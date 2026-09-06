use std::path::{Path, PathBuf};

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
    Weather,
    Media,
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
            Kind::Weather => "NOW",
            Kind::Media => "MEDIA",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    LaunchDesktop {
        path: PathBuf,
    },
    FocusWindow {
        address: String,
    },
    Copy(String),
    Paste(String),
    OpenUri(String),
    OpenPath(PathBuf),
    PlayMedia {
        path: PathBuf,
    },
    Spawn {
        program: String,
        args: Vec<String>,
    },
    Shell {
        command: String,
        terminal: bool,
    },
    EnterMode(Mode),
    SaveSnippet {
        keyword: String,
    },
    CreateNote {
        title: String,
    },
    OpenNote {
        id: String,
    },
    AskAi {
        prompt: String,
    },
    ToggleVoice,
    SaveSettings,
    InstallExt {
        id: String,
    },
    SyncScriptCommands,
    SyncVicinae,
    RunScript {
        path: PathBuf,
    },
    UseModel {
        source: String,
        model: String,
        endpoint: String,
        api: String,
    },
    SignIn {
        provider: String,
    },
    SignOut,
    RefreshModels,
    /// Start an installed extension command in the Node host.
    LaunchExtension {
        dir: PathBuf,
        command: String,
    },
    /// A row rendered by a running extension. Enter runs the first action,
    /// Shift+Enter the second.
    Extension {
        actions: Vec<ExtAction>,
        detail: String,
    },
    Confetti,
    SaveQuicklink {
        name: String,
        target: String,
    },
    Layout {
        name: String,
        address: Option<String>,
    },
    SaveLayout {
        name: String,
    },
    CloseWindow {
        address: String,
    },
    KillPid {
        pid: i32,
    },
    QuitClass {
        class: String,
    },
    QuitAll,
    ConfirmQuitAll,
    Uninstall {
        manager: String,
        package: String,
    },
    Capture {
        kind: String,
    },
    SetResolution {
        spec: String,
    },
    /// `None` reads a PNG from the clipboard into a temp file first.
    Ocr {
        path: Option<PathBuf>,
    },
    Qr {
        path: Option<PathBuf>,
    },
}

/// Callback identity for an action rendered by a running extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtAction {
    pub title: String,
    pub node: u64,
}

/// What a result *shows* — not what happens when you press Enter.
/// Weather, a photo, a document snippet. Icon+title+action is not enough.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Live {
    #[default]
    None,
    Weather {
        summary: String,
        location: String,
        extra: String,
    },
    Image {
        path: PathBuf,
    },
    Snippet {
        text: String,
    },
    Media {
        path: PathBuf,
        hint: String,
    },
}

impl Live {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn from_item(item: &Item) -> Self {
        match item.kind {
            Kind::Weather => Self::Weather {
                summary: item.title.clone(),
                location: item.subtitle.clone(),
                extra: String::new(),
            },
            Kind::Calc => {
                if item.subtitle.is_empty() {
                    Self::None
                } else {
                    Self::Snippet {
                        text: item.subtitle.clone(),
                    }
                }
            }
            Kind::File | Kind::Media => match &item.action {
                Action::OpenPath(path) | Action::PlayMedia { path } => Self::from_path(path),
                _ => Self::None,
            },
            Kind::App
            | Kind::Window
            | Kind::Command
            | Kind::Web
            | Kind::Clipboard
            | Kind::Shell
            | Kind::Snippet
            | Kind::Extension
            | Kind::Note
            | Kind::Ai
            | Kind::Voice
            | Kind::Settings
            | Kind::Store
            | Kind::Script => Self::None,
        }
    }

    pub fn thumb_path(&self) -> Option<&Path> {
        match self {
            Self::Image { path } | Self::Media { path, .. } => Some(path),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Self {
        match crate::preview::classify(path) {
            crate::preview::MediaKind::Image => Self::Image {
                path: path.to_path_buf(),
            },
            crate::preview::MediaKind::Audio => Self::Media {
                path: path.to_path_buf(),
                hint: format!("▶  {}", path.display()),
            },
            crate::preview::MediaKind::Video => Self::Media {
                path: path.to_path_buf(),
                hint: format!("▶  {}", path.display()),
            },
            crate::preview::MediaKind::Text | crate::preview::MediaKind::Document => {
                Self::Snippet {
                    text: String::new(),
                }
            }
            crate::preview::MediaKind::Other => Self::None,
        }
    }

    pub fn fill_snippet(&mut self, path: &Path) {
        if let Self::Snippet { text } = self
            && text.is_empty()
        {
            *text = crate::preview::snippet(path, 220);
        }
    }
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

#[derive(Debug, Clone, Default)]
pub enum Icon {
    Name(String),
    Path(PathBuf),
    #[default]
    None,
}

impl Item {
    pub fn haystack(&self) -> String {
        format!(
            "{} {} {} {}",
            self.title,
            self.subtitle,
            self.keywords,
            self.kind.label()
        )
    }
}
