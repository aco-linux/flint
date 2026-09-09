use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[cfg(test)]
use std::cell::RefCell;
#[cfg(not(test))]
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::auth;
use crate::clipboard;
use crate::config::Settings;
use crate::db;
use crate::item::{Action, Icon, Item, Kind};
use crate::mcp;
use crate::memory;
use crate::placeholder::{self, Input, Stamp};
use crate::preview::{self, MediaKind};
use crate::skills;

const HISTORY_TURNS: usize = 16;
const TURN_CHARS: usize = 4000;
const ATTACH_CAP: usize = 8;

pub struct Reply {
    pub text: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub created: u64,
    pub updated: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: String,
    pub thread_id: String,
    pub role: String,
    pub text: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub enum Attachment {
    Text { label: String, body: String },
    File { path: PathBuf, excerpt: String },
    Image { path: PathBuf, ocr: Option<String> },
}

impl Attachment {
    fn render(&self) -> String {
        match self {
            Self::Text { label, body } => format!("### {label}\n{body}"),
            Self::File { path, excerpt } => {
                format!("### File {}\n{excerpt}", path.display())
            }
            Self::Image { path, ocr } => match ocr {
                Some(text) if !text.is_empty() => {
                    format!("### Image at {}\n{text}", path.display())
                }
                _ => format!(
                    "### Image at {}\n(no OCR text; local path only)",
                    path.display()
                ),
            },
        }
    }
}

struct Session {
    thread: Option<String>,
    attachments: Vec<Attachment>,
}

impl Session {
    const fn new() -> Self {
        Self {
            thread: None,
            attachments: Vec::new(),
        }
    }
}

#[cfg(test)]
thread_local! {
    static SESSION: RefCell<Session> = const { RefCell::new(Session::new()) };
}

#[cfg(not(test))]
static SESSION: Mutex<Session> = Mutex::new(Session::new());

fn with_session<R>(f: impl FnOnce(&mut Session) -> R) -> R {
    #[cfg(test)]
    {
        SESSION.with(|cell| f(&mut cell.borrow_mut()))
    }
    #[cfg(not(test))]
    {
        let mut state = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut state)
    }
}

pub fn current_id() -> Option<String> {
    with_session(|s| s.thread.clone())
}

pub fn current_thread() -> Option<Thread> {
    current_id().and_then(|id| db::ai_thread_get(&id))
}

pub fn new_chat() {
    with_session(|s| {
        s.thread = None;
        s.attachments.clear();
    });
}

pub fn resume(id: &str) -> Option<Thread> {
    let thread = db::ai_thread_get(id)?;
    with_session(|s| {
        s.thread = Some(thread.id.clone());
        s.attachments.clear();
    });
    Some(thread)
}

pub fn ensure_thread(prompt: &str) -> Thread {
    if let Some(id) = current_id()
        && let Some(thread) = db::ai_thread_get(&id)
    {
        return thread;
    }
    let now = now_secs();
    let thread = Thread {
        id: new_id(),
        title: title_from(prompt),
        created: now,
        updated: now,
    };
    let _ = db::ai_thread_insert(&thread);
    with_session(|s| s.thread = Some(thread.id.clone()));
    thread
}

pub fn append_turn(thread_id: &str, role: &str, text: &str) {
    let now = now_secs();
    let _ = db::ai_message_insert(&ChatMessage {
        id: new_id(),
        thread_id: thread_id.to_string(),
        role: role.to_string(),
        text: text.to_string(),
        at: now,
    });
    let _ = db::ai_thread_touch(thread_id, None, now);
}

pub fn transcript(thread_id: &str) -> String {
    let msgs = db::ai_messages(thread_id).unwrap_or_default();
    if msgs.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for msg in msgs {
        let role = if msg.role == "assistant" {
            "Flint"
        } else {
            "You"
        };
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(role);
        out.push('\n');
        out.push_str(msg.text.trim());
    }
    out
}

pub fn history(thread_id: &str) -> Vec<Turn> {
    let mut msgs = db::ai_messages(thread_id).unwrap_or_default();
    if msgs.len() > HISTORY_TURNS {
        msgs = msgs.split_off(msgs.len() - HISTORY_TURNS);
    }
    msgs.into_iter()
        .filter(|m| !m.text.trim().is_empty())
        .map(|m| Turn {
            role: if m.role == "assistant" {
                "assistant".into()
            } else {
                "user".into()
            },
            text: clip_chars(&m.text, TURN_CHARS),
        })
        .collect()
}

pub fn thread_items(query: &str) -> Vec<Item> {
    db::ai_threads_search(query)
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.to_item())
        .collect()
}

impl Thread {
    pub fn to_item(&self) -> Item {
        let current = current_id().as_deref() == Some(self.id.as_str());
        Item {
            id: format!("ask:thread:{}", self.id),
            title: self.title.clone(),
            subtitle: if current {
                "Current chat · Enter resumes".into()
            } else {
                "Chat · Enter resumes".into()
            },
            keywords: format!("chat thread {}", self.title),
            kind: Kind::Ai,
            icon: Icon::Name("document-open-recent".into()),
            action: Action::ResumeThread {
                id: self.id.clone(),
            },
        }
    }
}

pub fn new_chat_item() -> Item {
    Item {
        id: "ask:new".into(),
        title: "New chat".into(),
        subtitle: "Start another Ask thread".into(),
        keywords: "new chat thread ask".into(),
        kind: Kind::Ai,
        icon: Icon::Name("document-new".into()),
        action: Action::NewChat,
    }
}

pub fn peek_attachments() -> Vec<Attachment> {
    with_session(|s| s.attachments.clone())
}

pub fn take_attachments() -> Vec<Attachment> {
    with_session(|s| std::mem::take(&mut s.attachments))
}

pub fn attach(attachment: Attachment) -> Result<String, String> {
    with_session(|s| {
        if s.attachments.len() >= ATTACH_CAP {
            return Err("Too many attachments (max 8)".into());
        }
        let label = match &attachment {
            Attachment::Text { label, .. } => format!("Attached {label}"),
            Attachment::File { path, .. } | Attachment::Image { path, .. } => {
                format!("Attached {}", path.display())
            }
        };
        s.attachments.push(attachment);
        Ok(label)
    })
}

pub fn attach_clipboard() -> Result<String, String> {
    let Some(text) = clipboard::current_text() else {
        return Err("Clipboard is empty".into());
    };
    if clipboard::looks_secret(&text) {
        return Err("Clipboard looks like a secret — not attached".into());
    }
    let body = clip_chars(text.trim(), 8 * 1024);
    if body.is_empty() {
        return Err("Clipboard is empty".into());
    }
    attach(Attachment::Text {
        label: "Clipboard".into(),
        body,
    })
}

pub fn attach_path(path: &Path) -> Result<String, String> {
    if path.is_dir() {
        return Err("Attach a file, not a folder".into());
    }
    if !path.exists() {
        return Err("File not found".into());
    }
    match preview::classify(path) {
        MediaKind::Image => {
            let ocr = crate::ocr::read_text(path);
            attach(Attachment::Image {
                path: path.to_path_buf(),
                ocr,
            })
        }
        MediaKind::Text | MediaKind::Document => {
            let excerpt = preview::read_head(path, 8 * 1024);
            if clipboard::looks_secret(&excerpt) {
                return Err("File looks like it contains a secret — not attached".into());
            }
            attach(Attachment::File {
                path: path.to_path_buf(),
                excerpt,
            })
        }
        _ => attach(Attachment::File {
            path: path.to_path_buf(),
            excerpt: format!("file at {}", path.display()),
        }),
    }
}

pub fn compose_user(prompt: &str, attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return prompt.to_string();
    }
    let mut out = prompt.to_string();
    out.push_str("\n\nAttachments:\n");
    for item in attachments {
        out.push_str(&item.render());
        out.push('\n');
    }
    out
}

const SELECTION_PROMPTS: &[(&str, &str, &str, &str)] = &[
    (
        "grammar",
        "Fix grammar",
        "Fix grammar and spelling of the selected text. Output only the corrected text.",
        "grammar spelling proofread",
    ),
    (
        "quickfix",
        "Quick Fix",
        "Fix this text. Correct grammar, spelling, and obvious mistakes. Keep the original meaning and tone. Output only the corrected text.",
        "quick fix rewrite tidy",
    ),
    (
        "translate",
        "Translate selection",
        "Translate the following text to English. If it is already English, translate to French. Output only the translation.",
        "translate translation language",
    ),
    (
        "explain",
        "Explain selection",
        "Explain the following selection clearly and concisely.",
        "explain meaning what",
    ),
];

pub fn command_items(which: impl Fn(&str) -> bool) -> Vec<Item> {
    let mut items = selection_items();
    items.push(Item {
        id: "cmd:attach-clipboard".into(),
        title: "Attach clipboard to Ask".into(),
        subtitle: "Text only · secrets are skipped".into(),
        keywords: "attach clipboard ask ai context".into(),
        kind: Kind::Ai,
        icon: Icon::Name("edit-paste".into()),
        action: Action::AttachClipboard,
    });
    items.push(Item {
        id: "cmd:attach-file".into(),
        title: "Attach file to Ask".into(),
        subtitle: "Ctrl+K on a file, or copy a path first".into(),
        keywords: "attach file ask ai document".into(),
        kind: Kind::Ai,
        icon: Icon::Name("document-send".into()),
        action: Action::AttachSelected,
    });
    if which("grim") {
        items.push(Item {
            id: "cmd:share-screen-ai".into(),
            title: "Share screen with AI".into(),
            subtitle: "Hide Flint, capture once, attach the local path".into(),
            keywords: "share screen screenshot ask ai grim".into(),
            kind: Kind::Ai,
            icon: Icon::Name("applets-screenshooter".into()),
            action: Action::ShareScreen,
        });
        if which("slurp") {
            items.push(Item {
                id: "cmd:share-region-ai".into(),
                title: "Share region with AI".into(),
                subtitle: "Hide Flint, select a region, attach the local path".into(),
                keywords: "share region screenshot ask ai grim slurp".into(),
                kind: Kind::Ai,
                icon: Icon::Name("applets-screenshooter".into()),
                action: Action::ShareRegion,
            });
        }
    }
    if which("openclaw") {
        items.push(openclaw_item("openclaw"));
    } else if which("open-claw") {
        items.push(openclaw_item("open-claw"));
    }
    items
}

fn openclaw_item(bin: &str) -> Item {
    Item {
        id: format!("cmd:{bin}"),
        title: "Open OpenClaw".into(),
        subtitle: "Launch the OpenClaw CLI if installed · not a Flint agent runtime".into(),
        keywords: "openclaw claw hermes agent".into(),
        kind: Kind::Command,
        icon: Icon::Name("utilities-terminal".into()),
        action: Action::Spawn {
            program: bin.into(),
            args: Vec::new(),
        },
    }
}

pub fn selection_items() -> Vec<Item> {
    SELECTION_PROMPTS
        .iter()
        .map(|(id, title, instruction, keys)| Item {
            id: format!("cmd:ask-{id}"),
            title: (*title).into(),
            subtitle: "Primary selection, then clipboard · Ask AI".into(),
            keywords: format!("{keys} selection ask ai"),
            kind: Kind::Ai,
            icon: Icon::Name("help-faq".into()),
            action: Action::AskSelection {
                template: format!("{instruction}\n\n{{selection}}"),
            },
        })
        .collect()
}

pub fn expand_selection_template(template: &str) -> Option<String> {
    let selection = clipboard::selection_or_clipboard().unwrap_or_default();
    let selection = selection.trim();
    if selection.is_empty() {
        return None;
    }
    let clipboard = clipboard::current_text().unwrap_or_default();
    let stamp = Stamp::local();
    let input = Input {
        clipboard: &clipboard,
        selection,
        argument: "",
        stamp: &stamp,
        increment: 0,
    };
    let (prompt, _) = placeholder::expand(template, &input);
    Some(prompt)
}

pub fn compose_system(settings: &Settings) -> String {
    let mut system = settings.ai.system_prompt.clone();
    if let Some(block) = memory::prompt_block() {
        system.push_str("\n\n");
        system.push_str(&block);
    }
    if let Some(block) = skills::prompt_block() {
        system.push_str("\n\n");
        system.push_str(&block);
    }
    system.push_str(
        "\n\nYou can call Flint tools to read the user's real calendar, weather, iCloud inbox, and Instant Answers. Use get_calendar_today, get_weather, get_inbox, or web_search instead of inventing a web search or telling them to open another app.",
    );
    if settings.general.allow_mcp
        && let Some(tools) = mcp::tool_primer(&settings.mcp)
    {
        system.push_str("\n\n");
        system.push_str(&tools);
    }
    system
}

pub fn ask(prompt: &str, settings: &Settings) -> Result<Reply, String> {
    chat(settings, &[], prompt)
}

pub fn chat(settings: &Settings, history: &[Turn], prompt: &str) -> Result<Reply, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Ask something first".into());
    }
    let system = compose_system(settings);
    match settings.ai.provider.as_str() {
        "openai" | "google" | "custom" | "lmstudio" | "llamacpp" | "xai" => {
            let credential = credential(settings)?;
            let label = match settings.ai.provider.as_str() {
                "google" => "Google",
                "lmstudio" => "LM Studio",
                "llamacpp" => "llama.cpp",
                "custom" => "Custom",
                "xai" => "Grok (xAI)",
                _ => "OpenAI",
            };
            chat_completions(
                settings,
                &chat_url(&settings.ai.endpoint, "/v1/chat/completions"),
                &credential,
                &settings.ai.model,
                &system,
                history,
                prompt,
                label,
            )
        }
        "anthropic" => anthropic(settings, &system, history, prompt),
        _ => ollama(settings, &system, history, prompt),
    }
}

/// Token-by-token preview. Local Ollama over HTTP streams NDJSON; other
/// providers emit one chunk from a normal `ask`. `emit` returning false cancels.
pub fn ask_stream(
    prompt: &str,
    settings: &Settings,
    mut emit: impl FnMut(&str) -> bool,
) -> Result<Reply, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Ask something first".into());
    }
    let url = chat_url(&settings.ai.endpoint, "/api/chat");
    let local_http = matches!(settings.ai.provider.as_str(), "ollama" | "")
        && parse_url(&url).is_ok_and(|u| !u.tls);
    if local_http {
        return ollama_stream(settings, prompt, emit);
    }
    let reply = ask(prompt, settings)?;
    if !reply.text.is_empty() {
        let _ = emit(&reply.text);
    }
    Ok(reply)
}

fn ollama_stream(
    settings: &Settings,
    prompt: &str,
    mut emit: impl FnMut(&str) -> bool,
) -> Result<Reply, String> {
    let url = chat_url(&settings.ai.endpoint, "/api/chat");
    let system = compose_system(settings);
    let body = json!({
        "model": settings.ai.model,
        "stream": true,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": prompt},
        ],
    });
    let mut acc = String::new();
    http_ndjson("POST", &url, &body, &[], |line| {
        if line.is_empty() {
            return true;
        }
        let Ok(raw) = serde_json::from_str::<Value>(line) else {
            return true;
        };
        let chunk = raw
            .pointer("/message/content")
            .and_then(Value::as_str)
            .or_else(|| raw.get("response").and_then(Value::as_str))
            .unwrap_or("");
        if chunk.is_empty() {
            return true;
        }
        let delta = fold_stream_delta(&mut acc, chunk);
        if delta.is_empty() {
            return true;
        }
        emit(&delta)
    })?;
    if acc.is_empty() {
        return Err("Ollama returned an empty answer".into());
    }
    Ok(Reply {
        text: acc,
        source: format!("Ollama · {}", settings.ai.model),
    })
}

fn fold_stream_delta(acc: &mut String, chunk: &str) -> String {
    if chunk.starts_with(acc.as_str()) {
        let delta = chunk[acc.len()..].to_string();
        acc.push_str(&delta);
        delta
    } else {
        acc.push_str(chunk);
        chunk.to_string()
    }
}

fn title_from(prompt: &str) -> String {
    let line = prompt
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("Chat")
        .trim();
    let title: String = line.chars().take(64).collect();
    if title.is_empty() {
        "Chat".into()
    } else {
        title
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{ns:x}-{n:x}")
}

fn clip_chars(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

fn chat_messages(history: &[Turn], prompt: &str) -> Vec<Value> {
    let mut msgs = Vec::new();
    let start = history.len().saturating_sub(HISTORY_TURNS);
    for turn in &history[start..] {
        let role = if turn.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        msgs.push(json!({"role": role, "content": clip_chars(&turn.text, TURN_CHARS)}));
    }
    msgs.push(json!({"role": "user", "content": prompt}));
    msgs
}

#[cfg(test)]
pub(crate) fn reset() {
    with_session(|s| *s = Session::new());
}

struct Credential {
    value: String,
    oauth: bool,
}

fn credential(settings: &Settings) -> Result<Credential, String> {
    if let Some(key) = auth::api_key(&settings.ai.provider) {
        return Ok(Credential {
            value: key,
            oauth: false,
        });
    }
    Ok(Credential {
        value: auth::bearer(settings)?.unwrap_or_default(),
        oauth: true,
    })
}

fn ollama(
    settings: &Settings,
    system: &str,
    history: &[Turn],
    prompt: &str,
) -> Result<Reply, String> {
    let url = chat_url(&settings.ai.endpoint, "/api/chat");
    let mut messages = vec![json!({"role": "system", "content": system})];
    messages.extend(chat_messages(history, prompt));
    let mut body = json!({
        "model": settings.ai.model,
        "stream": false,
        "messages": messages,
        "tools": crate::tools::definitions(),
    });
    let mut text = String::new();
    for _ in 0..3 {
        let raw = http_json("POST", &url, &body, &[])?;
        let message = raw.get("message").cloned().unwrap_or(Value::Null);
        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array)
            && !calls.is_empty()
        {
            let msgs = body
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .ok_or("bad messages")?;
            msgs.push(message.clone());
            for call in calls {
                let name = call
                    .pointer("/function/name")
                    .or_else(|| call.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let args = call
                    .pointer("/function/arguments")
                    .cloned()
                    .or_else(|| call.get("arguments").cloned())
                    .unwrap_or(json!({}));
                let args = match args {
                    Value::String(s) => serde_json::from_str(&s).unwrap_or(json!({})),
                    other => other,
                };
                let result = crate::tools::run(name, &args, settings);
                msgs.push(json!({
                    "role": "tool",
                    "content": result,
                }));
            }
            continue;
        }
        text = message
            .get("content")
            .and_then(Value::as_str)
            .or_else(|| raw.get("response").and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_string();
        break;
    }
    if text.is_empty() {
        return Err("Ollama returned an empty answer".into());
    }
    Ok(Reply {
        text,
        source: format!("Ollama · {}", settings.ai.model),
    })
}

#[allow(clippy::too_many_arguments)]
fn chat_completions(
    settings: &Settings,
    url: &str,
    credential: &Credential,
    model: &str,
    system: &str,
    history: &[Turn],
    prompt: &str,
    label: &str,
) -> Result<Reply, String> {
    let local = url.contains("127.0.0.1") || url.contains("localhost");
    if credential.value.is_empty() && !local {
        return Err(format!(
            "Connect a supported API OAuth account in Settings, or add a {label} API key"
        ));
    }
    if settings.ai.provider == "google"
        && credential.oauth
        && !credential.value.is_empty()
        && settings.ai.oauth_project_id.is_empty()
    {
        return Err("Google OAuth requires the Google Cloud quota project ID in Settings".into());
    }
    let mut messages = vec![json!({"role": "system", "content": system})];
    messages.extend(chat_messages(history, prompt));
    let mut body = json!({
        "model": model,
        "messages": messages,
        "max_tokens": 2048,
        "tools": crate::tools::definitions(),
    });
    let auth = if credential.value.is_empty() {
        None
    } else {
        Some(format!("Bearer {}", credential.value))
    };
    let mut headers = Vec::new();
    if let Some(value) = auth.as_deref() {
        headers.push(("Authorization", value));
    }
    if settings.ai.provider == "google" && credential.oauth {
        headers.push(("x-goog-user-project", &settings.ai.oauth_project_id));
    }
    let mut text = String::new();
    for _ in 0..3 {
        let raw = http_json("POST", url, &body, &headers)?;
        let message = raw
            .pointer("/choices/0/message")
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array)
            && !calls.is_empty()
        {
            let msgs = body
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .ok_or("bad messages")?;
            msgs.push(message.clone());
            for call in calls {
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let args_raw = call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}");
                let args: Value = serde_json::from_str(args_raw).unwrap_or(json!({}));
                let result = crate::tools::run(name, &args, settings);
                let id = call.get("id").and_then(Value::as_str).unwrap_or("");
                msgs.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": result,
                }));
            }
            continue;
        }
        text = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        break;
    }
    if text.is_empty() {
        return Err(format!("{label} returned an empty answer"));
    }
    Ok(Reply {
        text,
        source: format!("{label} · {}", settings.ai.model),
    })
}

fn anthropic(
    settings: &Settings,
    system: &str,
    history: &[Turn],
    prompt: &str,
) -> Result<Reply, String> {
    let key = auth::api_key("anthropic")
        .ok_or("Anthropic consumer subscriptions do not include API access. Add an Anthropic API key in Settings.")?;
    let url = chat_url(&settings.ai.endpoint, "/v1/messages");
    let endpoint = if settings.ai.endpoint.contains("anthropic") {
        url
    } else {
        "https://api.anthropic.com/v1/messages".into()
    };
    let mut messages = chat_messages(history, prompt);
    let mut text = String::new();
    for _ in 0..3 {
        let body = json!({
            "model": settings.ai.model,
            "max_tokens": 2048,
            "system": system,
            "messages": messages,
            "tools": crate::tools::anthropic_definitions(),
        });
        let raw = http_json(
            "POST",
            &endpoint,
            &body,
            &[("x-api-key", &key), ("anthropic-version", "2023-06-01")],
        )?;
        let stop = raw.get("stop_reason").and_then(Value::as_str).unwrap_or("");
        let content = raw.get("content").cloned().unwrap_or(json!([]));
        if stop == "tool_use" {
            messages.push(json!({"role": "assistant", "content": content}));
            let mut results = Vec::new();
            if let Some(blocks) = content.as_array() {
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                        continue;
                    }
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(json!({}));
                    let id = block.get("id").and_then(Value::as_str).unwrap_or("");
                    let result = crate::tools::run(name, &input, settings);
                    results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": result,
                    }));
                }
            }
            messages.push(json!({"role": "user", "content": results}));
            continue;
        }
        text = raw
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty()
            && let Some(blocks) = content.as_array()
        {
            text = blocks
                .iter()
                .filter_map(|b| {
                    (b.get("type").and_then(Value::as_str) == Some("text"))
                        .then(|| b.get("text").and_then(Value::as_str))
                        .flatten()
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        break;
    }
    if text.is_empty() {
        return Err("Anthropic returned an empty answer".into());
    }
    Ok(Reply {
        text,
        source: format!("Anthropic · {}", settings.ai.model),
    })
}

fn chat_url(endpoint: &str, path: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if base.ends_with(path)
        || base.contains("/chat/completions")
        || base.contains("/messages")
        || base.ends_with("/api/chat")
    {
        return base.to_string();
    }
    // OpenAI-compatible servers often already include `/v1` (xAI's default is
    // `https://api.x.ai/v1`). Don't emit `/v1/v1/chat/completions`.
    if base.ends_with("/v1")
        && let Some(rest) = path.strip_prefix("/v1/")
    {
        return format!("{base}/{rest}");
    }
    format!("{base}{path}")
}

fn http_json(
    method: &str,
    url: &str,
    body: &Value,
    extra_headers: &[(&str, &str)],
) -> Result<Value, String> {
    let parsed = parse_url(url)?;
    let payload = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let raw = if parsed.tls {
        https_request(method, &parsed, &payload, extra_headers)?
    } else {
        http_request(method, &parsed, &payload, extra_headers)?
    };
    let json_start = raw.find('{').ok_or_else(|| {
        let preview = raw.chars().take(180).collect::<String>();
        format!("Non-JSON response: {preview}")
    })?;
    serde_json::from_str(&raw[json_start..]).map_err(|e| format!("Bad JSON: {e}"))
}

fn http_ndjson(
    method: &str,
    url: &str,
    body: &Value,
    extra_headers: &[(&str, &str)],
    mut on_line: impl FnMut(&str) -> bool,
) -> Result<(), String> {
    let parsed = parse_url(url)?;
    if parsed.tls {
        return Err("NDJSON stream needs a local HTTP endpoint".into());
    }
    if extra_headers.iter().any(|(name, _)| {
        name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("x-api-key")
    }) {
        return Err("Refusing to send credentials over HTTP".into());
    }
    let payload = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let addr = format!("{}:{}", parsed.host, parsed.port);
    let stream = TcpStream::connect_timeout(
        &addr
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("Could not resolve AI endpoint")?,
        Duration::from_secs(8),
    )
    .map_err(|e| format!("Connect failed: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(60))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(15))).ok();
    let mut stream = stream;
    write_request(&mut stream, method, &parsed, &payload, extra_headers)?;
    let mut reader = BufReader::new(stream);
    let mut header = String::new();
    loop {
        header.clear();
        let n = reader.read_line(&mut header).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
    }
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if !on_line(line.trim_end_matches(['\r', '\n'])) {
            break;
        }
    }
    Ok(())
}

struct Url {
    host: String,
    port: u16,
    path: String,
    tls: bool,
}

fn parse_url(url: &str) -> Result<Url, String> {
    let tls = url.starts_with("https://");
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| "AI endpoint must be http(s)".to_string())?;
    if rest.contains('@') {
        return Err("AI endpoint must not include credentials".into());
    }
    let (hostport, path) = rest.split_once('/').unwrap_or((rest, "/"));
    let (host, port) = if let Some((h, p)) = hostport.split_once(':') {
        (
            h.to_string(),
            p.parse().unwrap_or(if tls { 443 } else { 80 }),
        )
    } else {
        (hostport.to_string(), if tls { 443 } else { 80 })
    };
    if host.is_empty()
        || host
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-')))
    {
        return Err("AI endpoint host is invalid".into());
    }
    Ok(Url {
        host,
        port,
        path: format!("/{path}"),
        tls,
    })
}

fn http_request(
    method: &str,
    url: &Url,
    body: &[u8],
    extra: &[(&str, &str)],
) -> Result<String, String> {
    if extra.iter().any(|(name, _)| {
        name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("x-api-key")
    }) {
        return Err("Refusing to send credentials over HTTP".into());
    }
    let addr = format!("{}:{}", url.host, url.port);
    let mut stream = TcpStream::connect_timeout(
        &addr
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("Could not resolve AI endpoint")?,
        Duration::from_secs(8),
    )
    .map_err(|e| format!("Connect failed: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(60))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(15))).ok();
    write_request(&mut stream, method, url, body, extra)?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn https_request(
    method: &str,
    url: &Url,
    body: &[u8],
    extra: &[(&str, &str)],
) -> Result<String, String> {
    crate::paths::ensure();
    let cfg_path = crate::paths::runtime_dir().join(format!("curl-{}.cfg", std::process::id()));
    let mut cfg = String::from("header = \"Content-Type: application/json\"\n");
    for (name, value) in extra {
        if name
            .chars()
            .any(|c| c.is_control() || c == '"' || c == '\\' || c == ':')
            || value.chars().any(|c| c.is_control())
        {
            return Err("Invalid HTTP header".into());
        }
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        cfg.push_str(&format!("header = \"{name}: {escaped}\"\n"));
    }
    crate::paths::write_private(&cfg_path, &cfg).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new("curl");
    cmd.args([
        "-sS",
        "--fail",
        "--max-time",
        "60",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "-K",
        cfg_path.to_str().unwrap_or(""),
        "-X",
        method,
        "--data-binary",
        "@-",
        &format!(
            "{}://{}:{}{}",
            if url.tls { "https" } else { "http" },
            url.host,
            url.port,
            url.path
        ),
    ]);
    let result = (|| {
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|_| "curl is required for HTTPS AI providers".to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(body).map_err(|e| e.to_string())?;
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("HTTPS request failed: {err}"));
        }
        String::from_utf8(out.stdout).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_file(&cfg_path);
    result
}

fn write_request<W: Write>(
    stream: &mut W,
    method: &str,
    url: &Url,
    body: &[u8],
    extra: &[(&str, &str)],
) -> Result<(), String> {
    let mut req = format!(
        "{method} {path} HTTP/1.0\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n",
        path = url.path,
        host = url.host,
        len = body.len()
    );
    for (name, value) in extra {
        if name.chars().any(|c| c.is_control() || c == ':') || value.chars().any(|c| c.is_control())
        {
            return Err("Invalid HTTP header".into());
        }
        req.push_str(name);
        req.push_str(": ");
        req.push_str(value);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        Attachment, Turn, attach, attach_clipboard, chat_messages, chat_url, command_items,
        compose_system, compose_user, ensure_thread, fold_stream_delta, history, new_chat,
        parse_url, peek_attachments, reset, resume, selection_items, thread_items,
    };
    use crate::config::Settings;
    use crate::db;
    use crate::item::Action;

    #[test]
    fn chat_url_does_not_double_v1() {
        assert_eq!(
            chat_url("https://api.x.ai/v1", "/v1/chat/completions"),
            "https://api.x.ai/v1/chat/completions"
        );
        assert_eq!(
            chat_url("https://api.x.ai/v1/", "/v1/chat/completions"),
            "https://api.x.ai/v1/chat/completions"
        );
        assert_eq!(
            chat_url("https://api.openai.com", "/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_url(
                "https://generativelanguage.googleapis.com/v1beta/openai",
                "/v1/chat/completions"
            ),
            "https://generativelanguage.googleapis.com/v1beta/openai/v1/chat/completions"
        );
        assert_eq!(
            chat_url("https://api.anthropic.com", "/v1/messages"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            chat_url("https://api.anthropic.com/v1", "/v1/messages"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            chat_url("http://127.0.0.1:11434", "/api/chat"),
            "http://127.0.0.1:11434/api/chat"
        );
        assert_eq!(
            chat_url(
                "https://api.x.ai/v1/chat/completions",
                "/v1/chat/completions"
            ),
            "https://api.x.ai/v1/chat/completions"
        );
    }

    #[test]
    fn parses_ollama_url() {
        let u = parse_url("http://127.0.0.1:11434/api/chat").unwrap();
        assert_eq!(u.host, "127.0.0.1");
        assert_eq!(u.port, 11434);
        assert!(!u.tls);
        assert_eq!(u.path, "/api/chat");
    }

    #[test]
    fn rejects_credentialed_urls() {
        assert!(parse_url("http://user:pass@127.0.0.1:11434/api/chat").is_err());
        assert!(parse_url("ftp://127.0.0.1/x").is_err());
    }

    #[test]
    fn history_payload_keeps_order() {
        let history = vec![
            Turn {
                role: "user".into(),
                text: "hi".into(),
            },
            Turn {
                role: "assistant".into(),
                text: "hello".into(),
            },
        ];
        let msgs = chat_messages(&history, "follow up");
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[2]["content"], "follow up");
    }

    #[test]
    fn threads_create_follow_up_and_search() {
        db::with_temp(|dir| {
            db::open_path(&dir.join("flint.db")).expect("open");
            reset();
            let first = ensure_thread("Weather in Tokyo");
            super::append_turn(&first.id, "user", "Weather in Tokyo");
            super::append_turn(&first.id, "assistant", "Sunny.");
            let same = ensure_thread("follow up");
            assert_eq!(same.id, first.id);
            super::append_turn(&same.id, "user", "And tomorrow?");
            super::append_turn(&same.id, "assistant", "Rain.");
            let turns = history(&first.id);
            assert_eq!(turns.len(), 4);
            let listed = thread_items("");
            assert_eq!(listed.len(), 1);
            assert!(listed[0].title.contains("Weather"));
            let found = thread_items("tomorrow");
            assert_eq!(found.len(), 1);
            new_chat();
            let second = ensure_thread("New topic");
            assert_ne!(second.id, first.id);
            assert_eq!(thread_items("").len(), 2);
            resume(&first.id).expect("resume");
            assert_eq!(super::current_id().as_deref(), Some(first.id.as_str()));
        });
    }

    #[test]
    fn attachments_skip_secrets_and_render_local_paths() {
        db::with_temp(|dir| {
            db::open_path(&dir.join("flint.db")).expect("open");
            reset();
            let secret = ["sk-", "abcdefghijklmnopqrstuvwxyz", "1234"].concat();
            // looks_secret is used by attach_clipboard; simulate the skip path.
            assert!(crate::clipboard::looks_secret(secret));
            attach(Attachment::Text {
                label: "Clipboard".into(),
                body: "hello from clip".into(),
            })
            .expect("text");
            let path = dir.join("notes.md");
            std::fs::write(&path, "ship it").expect("write");
            super::attach_path(&path).expect("file");
            let image = dir.join("shot.png");
            std::fs::write(&image, [0x89, b'P', b'N', b'G']).expect("png");
            super::attach_path(&image).expect("image");
            let composed = compose_user("what is this", &peek_attachments());
            assert!(composed.contains("hello from clip"));
            assert!(composed.contains("ship it"));
            assert!(composed.contains(&format!("Image at {}", image.display())));
            assert!(!composed.contains("http://"));
            let _ = attach_clipboard();
        });
    }

    #[test]
    fn selection_templates_use_placeholder() {
        let items = selection_items();
        assert_eq!(items.len(), 4);
        assert!(items.iter().any(|i| i.title == "Fix grammar"));
        assert!(items.iter().any(|i| i.title == "Quick Fix"));
        assert!(items.iter().any(|i| i.title == "Translate selection"));
        assert!(items.iter().any(|i| i.title == "Explain selection"));
        for item in &items {
            match &item.action {
                Action::AskSelection { template } => {
                    assert!(template.contains("{selection}"));
                }
                other => panic!("expected AskSelection, got {other:?}"),
            }
        }
        let stamp = crate::placeholder::Stamp {
            date: "2026-09-06".into(),
            time: "14:05".into(),
            day: "Sunday".into(),
        };
        let (expanded, _) = crate::placeholder::expand(
            "Fix grammar.\n\n{selection}",
            &crate::placeholder::Input {
                clipboard: "",
                selection: "teh cat",
                argument: "",
                stamp: &stamp,
                increment: 0,
            },
        );
        assert_eq!(expanded, "Fix grammar.\n\nteh cat");
    }

    #[test]
    fn command_items_omit_missing_binaries() {
        let none = command_items(|_| false);
        assert!(none.iter().any(|i| i.id == "cmd:ask-grammar"));
        assert!(none.iter().any(|i| i.id == "cmd:attach-clipboard"));
        assert!(!none.iter().any(|i| i.id == "cmd:share-region-ai"));
        assert!(!none.iter().any(|i| i.id.contains("openclaw")));
        let with = command_items(|bin| matches!(bin, "grim" | "slurp" | "openclaw"));
        assert!(with.iter().any(|i| i.id == "cmd:share-screen-ai"));
        assert!(with.iter().any(|i| i.id == "cmd:share-region-ai"));
        assert!(with.iter().any(|i| matches!(
            &i.action,
            Action::Spawn { program, .. } if program == "openclaw"
        )));
        let grim_only = command_items(|bin| bin == "grim");
        assert!(grim_only.iter().any(|i| i.id == "cmd:share-screen-ai"));
        assert!(!grim_only.iter().any(|i| i.id == "cmd:share-region-ai"));
    }

    #[test]
    fn system_prompt_stays_local_without_mcp_runner() {
        let settings = Settings::default();
        assert!(!settings.general.allow_mcp);
        let system = compose_system(&settings);
        assert!(system.contains("Flint"));
        assert!(!system.contains("Connected MCP servers"));
    }

    #[test]
    fn stream_delta_handles_accumulated_and_incremental() {
        let mut acc = String::new();
        assert_eq!(fold_stream_delta(&mut acc, "Hel"), "Hel");
        assert_eq!(fold_stream_delta(&mut acc, "Hello"), "lo");
        assert_eq!(acc, "Hello");
        assert_eq!(fold_stream_delta(&mut acc, "!"), "!");
        assert_eq!(acc, "Hello!");
    }
}
