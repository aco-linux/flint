use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::clipboard;
use crate::config::Settings;
use crate::db;
use crate::item::{Action, Icon, Item, Kind};
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Listening,
    Transcribing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dest {
    Search,
    FocusedApp,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: i64,
    pub text: String,
    pub at: u64,
}

impl Entry {
    pub fn to_item(&self) -> Item {
        let preview: String = self.text.chars().take(72).collect();
        Item {
            id: format!("voice:hist:{}", self.id),
            title: preview,
            subtitle: "Dictation history · Enter pastes".into(),
            keywords: format!("voice history dictation {} {}", self.text, self.at),
            kind: Kind::Voice,
            icon: Icon::Name("audio-input-microphone".into()),
            action: Action::Paste(self.text.clone()),
        }
    }
}

#[derive(Clone)]
pub struct Session {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    state: State,
    child: Option<u32>,
    dest: Dest,
}

impl Session {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                state: State::Idle,
                child: None,
                dest: Dest::Search,
            })),
        }
    }

    pub fn state(&self) -> State {
        self.inner.lock().map(|g| g.state).unwrap_or(State::Idle)
    }

    pub fn dest(&self) -> Dest {
        self.inner.lock().map(|g| g.dest).unwrap_or(Dest::Search)
    }

    pub fn set_dest(&self, dest: Dest) {
        if let Ok(mut g) = self.inner.lock() {
            g.dest = dest;
        }
    }

    pub fn start(&self, settings: &Settings) -> Result<(), String> {
        self.start_for(settings, Dest::Search)
    }

    pub fn start_for(&self, _settings: &Settings, dest: Dest) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        if g.state == State::Listening {
            g.dest = dest;
            return Ok(());
        }
        let wav = wav_path();
        let wav_str = wav
            .to_str()
            .ok_or_else(|| "Voice path is not valid UTF-8".to_string())?
            .to_string();
        let _ = fs::remove_file(&wav);
        let child = Command::new("pw-record")
            .args([
                "--rate",
                "16000",
                "--channels",
                "1",
                "--format",
                "s16",
                &wav_str,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("pw-record failed: {e}"))?;
        g.child = Some(child.id());
        g.state = State::Listening;
        g.dest = dest;
        Ok(())
    }

    pub fn stop(&self, settings: &Settings) -> Result<String, String> {
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            if let Some(pid) = g.child.take() {
                stop_pid(pid);
            }
            g.state = State::Transcribing;
        }
        thread::sleep(Duration::from_millis(180));
        let result = transcribe(settings);
        if let Ok(mut g) = self.inner.lock() {
            g.state = State::Idle;
        }
        result
    }

    pub fn cancel(&self) {
        if let Ok(mut g) = self.inner.lock() {
            if let Some(pid) = g.child.take() {
                stop_pid(pid);
            }
            g.state = State::Idle;
            g.dest = Dest::Search;
        }
        let _ = fs::remove_file(wav_path());
    }
}

pub fn remember(text: &str) {
    let text = text.trim();
    if text.is_empty() || clipboard::looks_secret(text) {
        return;
    }
    let _ = db::dictation_push(text);
}

pub fn history() -> Vec<Entry> {
    db::dictation_load().unwrap_or_default()
}

pub fn last_text() -> Option<String> {
    db::dictation_last()
}

pub fn history_item(id: &str) -> Option<Item> {
    let id: i64 = id.parse().ok()?;
    db::dictation_get(id).map(|e| e.to_item())
}

pub fn history_items() -> Vec<Item> {
    history().into_iter().map(|e| e.to_item()).collect()
}

const STYLES: &[(&str, &str, &str)] = &[
    (
        "email",
        "Rewrite last dictation as email",
        "Rewrite the following dictation as a clear email with a greeting and sign-off. Output only the rewritten text.",
    ),
    (
        "formal",
        "Rewrite last dictation formally",
        "Rewrite the following dictation in a formal style. Output only the rewritten text.",
    ),
    (
        "concise",
        "Make last dictation concise",
        "Rewrite the following dictation to be concise. Output only the rewritten text.",
    ),
    (
        "bullets",
        "Rewrite last dictation as bullets",
        "Rewrite the following dictation as a bullet list. Output only the rewritten text.",
    ),
];

const STYLE_LANGS: &[(&str, &str)] = &[
    ("fr", "French"),
    ("es", "Spanish"),
    ("de", "German"),
    ("ja", "Japanese"),
];

pub fn postprocess_items(last: &str) -> Vec<Item> {
    let preview: String = last.chars().take(48).collect();
    let mut items = Vec::new();
    for (id, title, instruction) in STYLES {
        items.push(Item {
            id: format!("voice:style:{id}"),
            title: (*title).into(),
            subtitle: format!("{preview}  ·  Ask AI"),
            keywords: format!("voice dictation style {id} {last}"),
            kind: Kind::Ai,
            icon: Icon::Name("help-faq".into()),
            action: Action::AskAi {
                prompt: format!("{instruction}\n\n{last}"),
            },
        });
    }
    for (code, name) in STYLE_LANGS {
        items.push(Item {
            id: format!("voice:lang:{code}"),
            title: format!("Translate last dictation to {name}"),
            subtitle: format!("{preview}  ·  Ask AI"),
            keywords: format!("voice dictation translate {name} {code} {last}"),
            kind: Kind::Ai,
            icon: Icon::Name("preferences-desktop-locale".into()),
            action: Action::AskAi {
                prompt: format!(
                    "Translate the following dictation to {name}. Output only the translation, nothing else.\n\n{last}"
                ),
            },
        });
    }
    items
}

pub fn command_items() -> Vec<Item> {
    vec![
        Item {
            id: "cmd:dictate-app".into(),
            title: "Dictate to focused app".into(),
            subtitle: "Hide Flint, speak, then Alt+Space to type with wtype".into(),
            keywords: "voice dictate dictation wtype focused app anywhere".into(),
            kind: Kind::Voice,
            icon: Icon::Name("audio-input-microphone".into()),
            action: Action::DictateFocused,
        },
        Item {
            id: "cmd:voice-history".into(),
            title: "Voice history".into(),
            subtitle: "Search past dictations · Enter pastes".into(),
            keywords: "voice history dictation remember transcript".into(),
            kind: Kind::Voice,
            icon: Icon::Name("document-open-recent".into()),
            action: Action::EnterMode(crate::mode::Mode::Voice),
        },
    ]
}

fn transcribe(settings: &Settings) -> Result<String, String> {
    let wav = wav_path();
    if !wav.exists() {
        return Err("No recording captured".into());
    }
    let meta = fs::metadata(&wav).map_err(|e| e.to_string())?;
    if meta.len() < 512 {
        let _ = fs::remove_file(&wav);
        return Err("That was too short — try again".into());
    }
    if !which("voxtype") {
        let _ = fs::remove_file(&wav);
        return Err("voxtype is required to transcribe".into());
    }
    let mut cmd = Command::new("voxtype");
    cmd.args(["transcribe", wav.to_str().unwrap_or("")]);
    if !settings.voice.language.is_empty() {
        cmd.args(["--language", &settings.voice.language]);
    }
    if !settings.voice.model.is_empty() {
        cmd.args(["--model", &settings.voice.model]);
    }
    let output = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("transcribe failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let _ = fs::remove_file(&wav);
    if !stdout.is_empty() {
        return Ok(stdout);
    }
    if !stderr.is_empty() {
        return Err(stderr.lines().next().unwrap_or("No speech detected").into());
    }
    Err("No speech detected".into())
}

fn stop_pid(pid: u32) {
    let _ = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status();
    thread::sleep(Duration::from_millis(80));
    let _ = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status();
}

fn wav_path() -> std::path::PathBuf {
    paths::runtime_dir().join("voice.wav")
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{STYLES, postprocess_items, remember};
    use crate::db;
    use crate::item::Action;

    #[test]
    fn style_prompts_are_templates_on_the_transcript() {
        let items = postprocess_items("ship the launcher tomorrow");
        assert!(items.len() >= STYLES.len());
        let formal = items
            .iter()
            .find(|item| item.id == "voice:style:formal")
            .expect("formal");
        match &formal.action {
            Action::AskAi { prompt } => {
                assert!(prompt.contains("ship the launcher tomorrow"));
                assert!(prompt.contains("formal"));
                assert!(prompt.contains("Output only"));
            }
            other => panic!("expected AskAi, got {other:?}"),
        }
        let fr = items
            .iter()
            .find(|item| item.id == "voice:lang:fr")
            .expect("french");
        match &fr.action {
            Action::AskAi { prompt } => {
                assert!(prompt.contains("French"));
                assert!(prompt.contains("ship the launcher tomorrow"));
            }
            other => panic!("expected AskAi, got {other:?}"),
        }
    }

    #[test]
    fn remember_writes_sqlite_history() {
        db::with_temp(|dir| {
            db::open_path(&dir.join("flint.db")).expect("open");
            remember("hello from flint");
            remember("second take");
            let rows = super::history();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].text, "second take");
            assert_eq!(super::last_text().as_deref(), Some("second take"));
            let item = super::history_item(&rows[0].id.to_string()).expect("item");
            assert!(matches!(item.action, Action::Paste(text) if text == "second take"));
        });
    }
}
