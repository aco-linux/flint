//! Extension runtime: run Vicinae / Raycast-style extensions.
//!
//! The JavaScript half lives in `share/runtime/flint-host.js` and is copied
//! into `~/.local/share/flint/runtime/` next to pinned `react`,
//! `react-reconciler`, `@vicinae/api`, and `esbuild`. Each command launch is
//! one Node process. The host renders the SDK's host-element tree to JSON;
//! this module turns that tree into Flint [`Item`]s and relays user actions.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::item::{Action, ExtAction, Icon, Item, Kind};

const HOST_JS: &str = include_str!("../share/runtime/flint-host.js");
/// Kill a view-command Node process after this much silence. One-shot check
/// on the existing pump — not a timer when `allow_extensions` is off.
pub const HOST_IDLE_SECS: u64 = 5 * 60;
const RUNTIME_PACKAGES: &[&str] = &[
    "react@19.1.1",
    "react-reconciler@0.32.0",
    "@vicinae/api@0.28.0",
    "esbuild@0.25.9",
];
const RUNTIME_STAMP: &str =
    "react@19.1.1 react-reconciler@0.32.0 @vicinae/api@0.28.0 esbuild@0.25.9";

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Manifest {
    pub name: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    pub author: String,
    pub commands: Vec<CommandDef>,
    pub preferences: Vec<PrefDef>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CommandDef {
    pub name: String,
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub mode: String,
    pub filename: String,
    pub arguments: Vec<ArgDef>,
    pub preferences: Vec<PrefDef>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ArgDef {
    pub name: String,
    pub placeholder: String,
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PrefDef {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub default: Value,
}

#[derive(Debug, Clone)]
pub struct Installed {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

pub fn extensions_dir() -> PathBuf {
    crate::paths::data_dir().join("store/vicinae")
}

pub fn runtime_dir() -> PathBuf {
    crate::paths::data_dir().join("runtime")
}

fn support_dir(extension: &str) -> PathBuf {
    crate::paths::data_dir().join("extensions").join(extension)
}

pub fn read_manifest(dir: &Path) -> Option<Manifest> {
    let raw = fs::read_to_string(dir.join("package.json")).ok()?;
    let manifest: Manifest = serde_json::from_str(&raw).ok()?;
    if manifest.name.is_empty() || manifest.commands.is_empty() {
        return None;
    }
    Some(manifest)
}

pub fn installed() -> Vec<Installed> {
    let Ok(entries) = fs::read_dir(extensions_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<Installed> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|dir| read_manifest(&dir).map(|manifest| Installed { dir, manifest }))
        .collect();
    out.sort_by(|a, b| a.manifest.title.cmp(&b.manifest.title));
    out
}

/// One root-search item per installed extension command.
pub fn command_items() -> Vec<Item> {
    let mut items = Vec::new();
    for ext in installed() {
        let ext_title = if ext.manifest.title.is_empty() {
            ext.manifest.name.clone()
        } else {
            ext.manifest.title.clone()
        };
        let icon = extension_icon(&ext.dir, &ext.manifest.icon);
        for cmd in &ext.manifest.commands {
            if cmd.mode == "menu-bar" {
                continue;
            }
            let subtitle = if cmd.subtitle.is_empty() {
                if cmd.description.is_empty() {
                    ext_title.clone()
                } else {
                    format!("{ext_title} · {}", cmd.description)
                }
            } else {
                format!("{ext_title} · {}", cmd.subtitle)
            };
            items.push(Item {
                id: format!("vx:{}/{}", ext.manifest.name, cmd.name),
                title: if cmd.title.is_empty() {
                    cmd.name.clone()
                } else {
                    cmd.title.clone()
                },
                subtitle,
                keywords: format!(
                    "{} {} extension vicinae raycast",
                    ext.manifest.name.replace('-', " "),
                    ext_title
                ),
                kind: Kind::Extension,
                icon: icon.clone(),
                action: Action::LaunchExtension {
                    dir: ext.dir.clone(),
                    command: cmd.name.clone(),
                },
            });
        }
    }
    items
}

/// One Settings row per installed extension. Values come from the manifest;
/// editing preferences in Settings is not implemented.
pub fn preference_items() -> Vec<Item> {
    let mut items = Vec::new();
    for ext in installed() {
        let title = if ext.manifest.title.is_empty() {
            ext.manifest.name.clone()
        } else {
            ext.manifest.title.clone()
        };
        let names: Vec<&str> = ext
            .manifest
            .preferences
            .iter()
            .map(|p| p.name.as_str())
            .filter(|n| !n.is_empty())
            .collect();
        let subtitle = if names.is_empty() {
            "Manifest defaults · editing preferences is not implemented".into()
        } else {
            format!("Defaults only · {}", names.join(", "))
        };
        items.push(Item {
            id: format!("set:ext-prefs:{}", ext.manifest.name),
            title: format!("{title} preferences"),
            subtitle,
            keywords: format!(
                "preferences settings extension {}",
                ext.manifest.name.replace('-', " ")
            ),
            kind: Kind::Settings,
            icon: Icon::Name("preferences-system".into()),
            action: Action::SaveSettings,
        });
    }
    items
}

fn extension_icon(dir: &Path, icon: &str) -> Icon {
    if icon.is_empty() {
        return Icon::Name("application-x-addon".into());
    }
    for candidate in [dir.join("assets").join(icon), dir.join(icon)] {
        if candidate.is_file() {
            return Icon::Path(candidate);
        }
    }
    Icon::Name("application-x-addon".into())
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

/// Install the pinned host dependencies once, and always refresh the host
/// script so a Flint upgrade carries its runtime with it.
pub fn ensure_runtime() -> Result<PathBuf, String> {
    if !which("node") {
        return Err("Node.js is required to run extensions (install `nodejs`)".into());
    }
    let dir = runtime_dir();
    crate::paths::ensure_dir(&dir);
    let host = dir.join("flint-host.js");
    if fs::read_to_string(&host).ok().as_deref() != Some(HOST_JS) {
        fs::write(&host, HOST_JS).map_err(|e| e.to_string())?;
    }
    let stamp = dir.join(".packages");
    let ready = fs::read_to_string(&stamp).ok().as_deref() == Some(RUNTIME_STAMP)
        && dir.join("node_modules/react-reconciler").is_dir()
        && dir.join("node_modules/@vicinae/api").is_dir()
        && esbuild_bin(&dir, &dir).is_some();
    if ready {
        return Ok(dir);
    }
    if !which("npm") {
        return Err("npm is required to set up the extension runtime".into());
    }
    if !dir.join("package.json").exists() {
        fs::write(
            dir.join("package.json"),
            "{\"name\":\"flint-runtime\",\"private\":true}\n",
        )
        .map_err(|e| e.to_string())?;
    }
    let status = Command::new("npm")
        .args([
            "install",
            "--no-audit",
            "--no-fund",
            "--ignore-scripts",
            "--loglevel=error",
        ])
        .args(RUNTIME_PACKAGES)
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("npm failed to start: {e}"))?;
    if !status.success() {
        return Err("npm could not install the extension runtime".into());
    }
    // esbuild ships as a native binary; --ignore-scripts skipped its postinstall.
    let rebuilt = Command::new("npm")
        .args(["rebuild", "esbuild", "--loglevel=error"])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !rebuilt || esbuild_bin(&dir, &dir).is_none() {
        return Err("esbuild could not be installed for the extension runtime".into());
    }
    fs::write(&stamp, RUNTIME_STAMP).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn entry_source(dir: &Path, command: &str, filename: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if !filename.is_empty() {
        candidates.push(dir.join(filename));
        candidates.push(dir.join("src").join(filename));
    }
    for ext in ["tsx", "ts", "jsx", "js"] {
        candidates.push(dir.join("src").join(format!("{command}.{ext}")));
        candidates.push(dir.join(format!("{command}.{ext}")));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn newest_mtime(path: &Path, best: &mut SystemTime) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if let Ok(m) = meta.modified()
        && m > *best
    {
        *best = m;
    }
    if meta.is_dir()
        && let Ok(entries) = fs::read_dir(path)
    {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name == "node_modules" || name == ".flint" || name == ".git" {
                continue;
            }
            newest_mtime(&entry.path(), best);
        }
    }
}

fn esbuild_bin(dir: &Path, runtime: &Path) -> Option<PathBuf> {
    [
        dir.join("node_modules/.bin/esbuild"),
        runtime.join("node_modules/.bin/esbuild"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// Bundle the command entry into `.flint/<command>.js` when sources are newer
/// than the last bundle. `react` and the SDK stay external so the host's
/// single React instance is used.
pub fn build(dir: &Path, command: &str, filename: &str, runtime: &Path) -> Result<PathBuf, String> {
    let out_dir = dir.join(".flint");
    let out = out_dir.join(format!("{command}.js"));
    let src = entry_source(dir, command, filename)
        .ok_or_else(|| format!("No source for command {command} in {}", dir.display()))?;
    let mut newest = SystemTime::UNIX_EPOCH;
    newest_mtime(&dir.join("src"), &mut newest);
    newest_mtime(&dir.join("package.json"), &mut newest);
    newest_mtime(&src, &mut newest);
    if let Ok(meta) = fs::metadata(&out)
        && let Ok(built) = meta.modified()
        && built >= newest
    {
        return Ok(out);
    }
    if !dir.join("node_modules").is_dir() && which("npm") {
        let _ = Command::new("npm")
            .args([
                "install",
                "--ignore-scripts",
                "--no-audit",
                "--no-fund",
                "--loglevel=error",
            ])
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let esbuild = esbuild_bin(dir, runtime).ok_or("esbuild is missing from the runtime")?;
    crate::paths::ensure_dir(&out_dir);
    let output = Command::new(esbuild)
        .arg(&src)
        .args([
            "--bundle",
            "--platform=node",
            "--format=cjs",
            "--target=node20",
            "--jsx=automatic",
            "--log-level=error",
            "--alias:@raycast/api=@vicinae/api",
            "--external:react",
            "--external:react/jsx-runtime",
            "--external:react/jsx-dev-runtime",
            "--external:@vicinae/api",
            "--external:@raycast/api",
            "--loader:.png=dataurl",
            "--loader:.svg=dataurl",
            "--loader:.jpg=dataurl",
        ])
        .arg(format!("--outfile={}", out.display()))
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("esbuild failed to start: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let first = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("build failed");
        return Err(format!("Build failed: {first}"));
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct Launch {
    pub dir: PathBuf,
    pub command: String,
    pub arguments: HashMap<String, String>,
}

#[derive(Debug)]
pub enum Msg {
    Status(String),
    Ready,
    Render(View),
    Toast {
        title: String,
        message: String,
        style: String,
    },
    Hud(String),
    CloseWindow,
    PopToRoot,
    SetSearchText(String),
    Request {
        id: u64,
        method: String,
        params: Value,
    },
    Done,
    Error(String),
    Exited,
}

pub struct Session {
    pub title: String,
    pub no_view: bool,
    pub view: Option<View>,
    pub depth: usize,
    pub last_search: Option<String>,
    rx: Receiver<Msg>,
    out: Sender<Value>,
    child: Arc<Mutex<Option<Child>>>,
    last_activity: Mutex<Instant>,
}

impl Session {
    /// Spawn in the background: runtime setup and the esbuild bundle can take
    /// a few seconds the first time, and must not block the GTK thread.
    pub fn start(launch: Launch) -> Self {
        let manifest = read_manifest(&launch.dir).unwrap_or_default();
        let cmd = manifest
            .commands
            .iter()
            .find(|c| c.name == launch.command)
            .cloned()
            .unwrap_or_default();
        let title = if cmd.title.is_empty() {
            launch.command.clone()
        } else {
            cmd.title.clone()
        };
        let no_view = cmd.mode == "no-view";
        let (tx, rx) = mpsc::channel();
        let (out, out_rx) = mpsc::channel::<Value>();
        let child = Arc::new(Mutex::new(None));
        let child_slot = child.clone();
        thread::spawn(move || run(launch, manifest, cmd, tx, out_rx, child_slot));
        Self {
            title,
            no_view,
            view: None,
            depth: 1,
            last_search: None,
            rx,
            out,
            child,
            last_activity: Mutex::new(Instant::now()),
        }
    }

    fn touch(&self) {
        if let Ok(mut last) = self.last_activity.lock() {
            *last = Instant::now();
        }
    }

    pub fn idle_expired(&self) -> bool {
        self.last_activity
            .lock()
            .ok()
            .is_some_and(|last| host_idle_expired(*last, Instant::now()))
    }

    pub fn try_recv(&self) -> Option<Msg> {
        let msg = self.rx.try_recv().ok()?;
        self.touch();
        Some(msg)
    }

    pub fn invoke(&self, node: u64, prop: &str, args: Vec<Value>) {
        self.touch();
        let _ = self.out.send(json!({
            "type": "invoke",
            "node": node,
            "prop": prop,
            "args": args,
        }));
    }

    pub fn search(&mut self, text: &str) {
        let Some(view) = &self.view else {
            return;
        };
        let Some((node, prop)) = &view.search_callback else {
            return;
        };
        if self.last_search.as_deref() == Some(text) {
            return;
        }
        self.last_search = Some(text.to_string());
        self.invoke(*node, prop, vec![Value::String(text.to_string())]);
    }

    pub fn respond(&self, id: u64, result: Result<Value, String>) {
        self.touch();
        let msg = match result {
            Ok(result) => json!({"type": "response", "id": id, "result": result}),
            Err(error) => json!({"type": "response", "id": id, "error": error}),
        };
        let _ = self.out.send(msg);
    }

    pub fn pop(&self) {
        self.touch();
        let _ = self.out.send(json!({"type": "pop"}));
    }
}

pub fn host_idle_expired(last: Instant, now: Instant) -> bool {
    now.duration_since(last) >= Duration::from_secs(HOST_IDLE_SECS)
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.out.send(json!({"type": "quit"}));
        if let Ok(mut slot) = self.child.lock()
            && let Some(mut child) = slot.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn run(
    launch: Launch,
    manifest: Manifest,
    cmd: CommandDef,
    tx: Sender<Msg>,
    out_rx: Receiver<Value>,
    child_slot: Arc<Mutex<Option<Child>>>,
) {
    let _ = tx.send(Msg::Status("Preparing extension runtime…".into()));
    let runtime = match ensure_runtime() {
        Ok(dir) => dir,
        Err(err) => {
            let _ = tx.send(Msg::Error(err));
            let _ = tx.send(Msg::Exited);
            return;
        }
    };
    let _ = tx.send(Msg::Status(format!("Building {}…", cmd.title)));
    let entry = match build(&launch.dir, &launch.command, &cmd.filename, &runtime) {
        Ok(path) => path,
        Err(err) => {
            let _ = tx.send(Msg::Error(err));
            let _ = tx.send(Msg::Exited);
            return;
        }
    };
    let storage = support_dir(&manifest.name);
    crate::paths::ensure_dir(&storage);

    let mut preferences = serde_json::Map::new();
    for pref in manifest.preferences.iter().chain(cmd.preferences.iter()) {
        if !pref.default.is_null() {
            preferences.insert(pref.name.clone(), pref.default.clone());
        }
    }
    let config = json!({
        "runtime": runtime,
        "extensionDir": launch.dir,
        "entry": entry,
        "mode": if cmd.mode.is_empty() { "view" } else { cmd.mode.as_str() },
        "storageDir": storage,
        "extensionName": manifest.name,
        "commandName": cmd.name,
        "author": manifest.author,
        "preferences": preferences,
        "arguments": launch.arguments,
    });

    let node_path = format!(
        "{}:{}",
        launch.dir.join("node_modules").display(),
        runtime.join("node_modules").display()
    );
    let mut child = match Command::new("node")
        .arg(runtime.join("flint-host.js"))
        .current_dir(&launch.dir)
        .env("NODE_PATH", node_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            let _ = tx.send(Msg::Error(format!("Could not start node: {err}")));
            let _ = tx.send(Msg::Exited);
            return;
        }
    };

    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    if let Ok(mut slot) = child_slot.lock() {
        *slot = Some(child);
    }

    thread::spawn(move || {
        let Some(mut stdin) = stdin else {
            return;
        };
        let mut first = config.to_string();
        first.push('\n');
        if stdin.write_all(first.as_bytes()).is_err() || stdin.flush().is_err() {
            return;
        }
        while let Ok(value) = out_rx.recv() {
            let quit = value.get("type").and_then(Value::as_str) == Some("quit");
            let mut line = value.to_string();
            line.push('\n');
            if stdin.write_all(line.as_bytes()).is_err() || stdin.flush().is_err() || quit {
                break;
            }
        }
    });

    if let Some(stderr) = stderr {
        let name = manifest.name.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("[{name}] {line}");
            }
        });
    }

    let Some(stdout) = stdout else {
        let _ = tx.send(Msg::Exited);
        return;
    };
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(msg) = decode(&value) else {
            continue;
        };
        if tx.send(msg).is_err() {
            break;
        }
    }
    let _ = tx.send(Msg::Exited);
}

fn decode(value: &Value) -> Option<Msg> {
    let kind = value.get("type")?.as_str()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Some(match kind {
        "ready" => Msg::Ready,
        "render" => Msg::Render(parse_view(value)),
        "toast" => Msg::Toast {
            title: text("title"),
            message: text("message"),
            style: text("style"),
        },
        "hud" => Msg::Hud(text("title")),
        "closeWindow" => Msg::CloseWindow,
        "popToRoot" => Msg::PopToRoot,
        "setSearchText" => Msg::SetSearchText(text("text")),
        "request" => Msg::Request {
            id: value.get("id")?.as_u64()?,
            method: text("method"),
            params: value.get("params").cloned().unwrap_or(Value::Null),
        },
        "done" => Msg::Done,
        "error" => Msg::Error(text("message")),
        _ => return None,
    })
}

#[derive(Debug, Clone, Default)]
pub struct View {
    pub depth: usize,
    pub title: String,
    pub placeholder: String,
    pub is_loading: bool,
    /// Flint filters rows itself (Raycast's default when no search callback).
    pub local_filter: bool,
    pub search_callback: Option<(u64, String)>,
    pub empty_title: String,
    pub empty_description: String,
    pub rows: Vec<Row>,
    /// A `Detail` or unsupported root shows as one row with a preview.
    pub notice: Option<String>,
    pub is_form: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub keywords: String,
    pub section: String,
    pub icon: Icon,
    pub detail: String,
    pub actions: Vec<ExtAction>,
    pub field_id: String,
    pub field_kind: String,
    pub field_value: String,
    pub field_on_change: Option<(u64, String)>,
}

#[derive(Debug, Clone)]
pub struct ConfirmPrompt {
    pub title: String,
    pub message: String,
    pub primary: String,
    pub dismiss: String,
}

pub fn confirm_prompt(params: &Value) -> ConfirmPrompt {
    let title = params
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Confirm")
        .to_string();
    let message = params
        .get("message")
        .or_else(|| params.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let primary = params
        .pointer("/primaryAction/title")
        .and_then(Value::as_str)
        .unwrap_or("Confirm")
        .to_string();
    let dismiss = params
        .pointer("/dismissAction/title")
        .and_then(Value::as_str)
        .unwrap_or("Cancel")
        .to_string();
    ConfirmPrompt {
        title,
        message,
        primary,
        dismiss,
    }
}

pub fn confirm_items(prompt: &ConfirmPrompt) -> Vec<Item> {
    let subtitle = if prompt.message.is_empty() {
        prompt.title.clone()
    } else {
        prompt.message.clone()
    };
    vec![
        Item {
            id: "vx-confirm:yes".into(),
            title: prompt.primary.clone(),
            subtitle: subtitle.clone(),
            keywords: "confirm yes ok".into(),
            kind: Kind::Extension,
            icon: Icon::Name("emblem-ok".into()),
            action: Action::ExtensionConfirm { confirmed: true },
        },
        Item {
            id: "vx-confirm:no".into(),
            title: prompt.dismiss.clone(),
            subtitle: "Esc cancels".into(),
            keywords: "cancel no dismiss".into(),
            kind: Kind::Extension,
            icon: Icon::Name("dialog-error".into()),
            action: Action::ExtensionConfirm { confirmed: false },
        },
    ]
}

pub fn form_values(view: &View) -> Value {
    let mut map = serde_json::Map::new();
    for row in &view.rows {
        if row.field_id.is_empty() {
            continue;
        }
        let value = if row.field_kind == "checkbox" {
            Value::Bool(row.field_value == "true")
        } else {
            Value::String(row.field_value.clone())
        };
        map.insert(row.field_id.clone(), value);
    }
    Value::Object(map)
}

fn children(node: &Value) -> &[Value] {
    node.get("children")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn node_type(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or("")
}

fn prop<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    node.get("props").and_then(|p| p.get(key))
}

fn prop_str(node: &Value, key: &str) -> String {
    match prop(node, key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(o)) => o
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn prop_bool(node: &Value, key: &str) -> Option<bool> {
    prop(node, key).and_then(Value::as_bool)
}

fn callback(node: &Value, key: &str) -> Option<(u64, String)> {
    let cb = prop(node, key)?.get("$cb")?.as_array()?;
    Some((cb.first()?.as_u64()?, cb.get(1)?.as_str()?.to_string()))
}

fn node_id(node: &Value) -> u64 {
    node.get("id").and_then(Value::as_u64).unwrap_or(0)
}

fn find<'a>(nodes: &'a [Value], types: &[&str]) -> Option<&'a Value> {
    for node in nodes {
        if types.contains(&node_type(node)) {
            return Some(node);
        }
        if let Some(hit) = find(children(node), types) {
            return Some(hit);
        }
    }
    None
}

pub fn parse_view(message: &Value) -> View {
    let depth = message.get("depth").and_then(Value::as_u64).unwrap_or(1) as usize;
    let root = message
        .get("root")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let frame = root
        .iter()
        .rev()
        .find(|n| node_type(n) == "nav-frame")
        .map(children)
        .unwrap_or(root);
    let mut view = View {
        depth,
        ..View::default()
    };
    let Some(top) = find(frame, &["list", "grid", "detail", "form"]) else {
        view.notice = Some("This command has nothing to show.".into());
        return view;
    };
    match node_type(top) {
        "list" | "grid" => parse_list(top, &mut view),
        "form" => parse_form(top, &mut view),
        "detail" => {
            view.title = prop_str(top, "navigationTitle");
            let markdown = prop_str(top, "markdown");
            let actions = find(children(top), &["action-panel"])
                .map(collect_actions)
                .unwrap_or_default();
            view.rows.push(Row {
                id: format!("vx-detail:{}", node_id(top)),
                title: markdown
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("Detail")
                    .trim_start_matches('#')
                    .trim()
                    .to_string(),
                subtitle: actions
                    .first()
                    .map(|a| format!("Enter · {}", a.title))
                    .unwrap_or_default(),
                icon: Icon::Name("text-x-generic".into()),
                detail: markdown,
                actions,
                ..Row::default()
            });
            view.local_filter = false;
        }
        _ => {
            view.notice = Some("This command has nothing to show.".into());
        }
    }
    view
}

fn parse_list(list: &Value, view: &mut View) {
    view.title = prop_str(list, "navigationTitle");
    view.placeholder = prop_str(list, "searchBarPlaceholder");
    view.is_loading = prop_bool(list, "isLoading").unwrap_or(false);
    view.search_callback = callback(list, "onSearchTextChange");
    view.local_filter = prop_bool(list, "filtering").unwrap_or(view.search_callback.is_none());
    let shared = children(list)
        .iter()
        .find(|c| node_type(c) == "action-panel")
        .map(collect_actions)
        .unwrap_or_default();
    for child in children(list) {
        match node_type(child) {
            "list-section" | "grid-section" => {
                let section = prop_str(child, "title");
                for item in children(child) {
                    if matches!(node_type(item), "list-item" | "grid-item") {
                        view.rows.push(parse_item(item, &section, &shared));
                    }
                }
            }
            "list-item" | "grid-item" => view.rows.push(parse_item(child, "", &shared)),
            "empty-view" | "list-empty-view" => {
                view.empty_title = prop_str(child, "title");
                view.empty_description = prop_str(child, "description");
            }
            _ => {}
        }
    }
}

fn parse_form(form: &Value, view: &mut View) {
    view.title = prop_str(form, "navigationTitle");
    view.is_loading = prop_bool(form, "isLoading").unwrap_or(false);
    view.local_filter = true;
    view.is_form = true;
    view.placeholder = "Filter fields…".into();
    let shared = children(form)
        .iter()
        .find(|c| node_type(c) == "action-panel")
        .map(collect_actions)
        .unwrap_or_default();
    if !shared.is_empty() {
        let title = shared
            .first()
            .map(|a| a.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Submit".into());
        view.rows.push(Row {
            id: format!("vx-form-submit:{}", node_id(form)),
            title,
            subtitle: "Enter submits · fields are a list (no grid)".into(),
            icon: Icon::Name("emblem-ok".into()),
            actions: shared.clone(),
            ..Row::default()
        });
    }
    for child in children(form) {
        match node_type(child) {
            "text-field" | "password-field" | "text-area-field" | "textarea-field"
            | "checkbox-field" | "dropdown-field" | "date-picker-field" | "file-picker-field"
            | "tag-picker-field" => {
                view.rows.push(parse_form_field(child, &shared));
            }
            "form-description" => {
                let title = prop_str(child, "title");
                let text = prop_str(child, "text");
                let empty_title = title.is_empty();
                view.rows.push(Row {
                    id: format!("vx-form-desc:{}", node_id(child)),
                    title: if empty_title { text.clone() } else { title },
                    subtitle: if empty_title { String::new() } else { text },
                    icon: Icon::Name("dialog-information".into()),
                    actions: shared.clone(),
                    ..Row::default()
                });
            }
            _ => {}
        }
    }
}

fn form_field_kind(node_type: &str) -> &'static str {
    match node_type {
        "password-field" => "password",
        "text-area-field" | "textarea-field" => "textarea",
        "checkbox-field" => "checkbox",
        "dropdown-field" => "dropdown",
        "date-picker-field" => "date",
        "file-picker-field" => "file",
        "tag-picker-field" => "tags",
        _ => "text",
    }
}

fn field_value_string(node: &Value) -> String {
    match prop(node, "value").or_else(|| prop(node, "defaultValue")) {
        Some(Value::Bool(b)) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        Some(Value::Object(o)) => o
            .get("value")
            .map(|v| match v {
                Value::Bool(b) => {
                    if *b {
                        "true".into()
                    } else {
                        "false".into()
                    }
                }
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn field_subtitle(kind: &str, value: &str, extra: &str) -> String {
    let shown = match kind {
        "password" if !value.is_empty() => "••••".into(),
        "checkbox" => {
            if value == "true" {
                "On · Enter toggles".into()
            } else {
                "Off · Enter toggles".into()
            }
        }
        _ if value.is_empty() => "Enter to edit".into(),
        _ => value.to_string(),
    };
    if extra.is_empty() {
        shown
    } else if shown.is_empty() {
        extra.to_string()
    } else {
        format!("{shown}  ·  {extra}")
    }
}

fn parse_form_field(field: &Value, shared: &[ExtAction]) -> Row {
    let kind = form_field_kind(node_type(field)).to_string();
    let id = prop_str(field, "id");
    let title = {
        let title = prop_str(field, "title");
        if title.is_empty() {
            if id.is_empty() {
                kind.clone()
            } else {
                id.clone()
            }
        } else {
            title
        }
    };
    let value = field_value_string(field);
    let extra = if kind == "dropdown" {
        children(field)
            .iter()
            .filter(|c| node_type(c) == "dropdown-item")
            .map(|c| {
                let t = prop_str(c, "title");
                if t.is_empty() {
                    prop_str(c, "value")
                } else {
                    t
                }
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        let info = prop_str(field, "info");
        let error = prop_str(field, "error");
        [error.as_str(), info.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("  ·  ")
    };
    let on_change = callback(field, "onChange");
    Row {
        id: format!("vx-form-field:{}", node_id(field)),
        title,
        subtitle: field_subtitle(&kind, &value, &extra),
        keywords: id.clone(),
        icon: Icon::Name(
            match kind.as_str() {
                "checkbox" => "checkbox-checked-symbolic",
                "password" => "dialog-password",
                "file" => "folder",
                _ => "document-edit",
            }
            .into(),
        ),
        actions: shared.to_vec(),
        field_id: id,
        field_kind: kind,
        field_value: value,
        field_on_change: on_change,
        ..Row::default()
    }
}

fn parse_item(item: &Value, section: &str, shared: &[ExtAction]) -> Row {
    let mut actions = find(children(item), &["action-panel"])
        .map(collect_actions)
        .unwrap_or_default();
    if actions.is_empty() {
        actions = shared.to_vec();
    }
    let detail = find(children(item), &["list-item-detail", "grid-item-detail"])
        .map(|d| prop_str(d, "markdown"))
        .unwrap_or_default();
    let mut subtitle = prop_str(item, "subtitle");
    let accessories: Vec<String> = prop(item, "accessories")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|a| match a.get("text") {
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(Value::Object(o)) => {
                        o.get("value").and_then(Value::as_str).map(String::from)
                    }
                    _ => a.get("tag").and_then(|t| match t {
                        Value::String(s) => Some(s.clone()),
                        Value::Object(o) => {
                            o.get("value").and_then(Value::as_str).map(String::from)
                        }
                        _ => None,
                    }),
                })
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if !accessories.is_empty() {
        if !subtitle.is_empty() {
            subtitle.push_str("  ·  ");
        }
        subtitle.push_str(&accessories.join("  ·  "));
    }
    let keywords = prop(item, "keywords")
        .and_then(Value::as_array)
        .map(|k| {
            k.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let id = prop_str(item, "id");
    Row {
        id: if id.is_empty() {
            format!("vx-item:{}", node_id(item))
        } else {
            format!("vx-item:{id}")
        },
        title: prop_str(item, "title"),
        subtitle,
        keywords,
        section: section.to_string(),
        icon: parse_icon(prop(item, "icon")),
        detail,
        actions,
        ..Row::default()
    }
}

fn collect_actions(panel: &Value) -> Vec<ExtAction> {
    let mut out = Vec::new();
    walk_actions(children(panel), &mut out);
    out
}

fn walk_actions(nodes: &[Value], out: &mut Vec<ExtAction>) {
    for node in nodes {
        match node_type(node) {
            "action" => {
                let prop_name = if prop(node, "onAction").is_some() {
                    "onAction"
                } else if prop(node, "onSubmit").is_some() {
                    "onSubmit"
                } else {
                    continue;
                };
                let title = prop_str(node, "title");
                out.push(ExtAction {
                    title: if title.is_empty() {
                        "Submit".into()
                    } else {
                        title
                    },
                    node: node_id(node),
                    prop: prop_name.into(),
                });
            }
            "action-panel-section" | "action-panel-submenu" => walk_actions(children(node), out),
            _ => {}
        }
    }
}

pub fn parse_icon(icon: Option<&Value>) -> Icon {
    let Some(icon) = icon else {
        return Icon::None;
    };
    if let Some(file) = icon.get("fileIcon").and_then(Value::as_str) {
        return Icon::Path(PathBuf::from(file));
    }
    let raw = icon
        .pointer("/source/raw")
        .or_else(|| icon.pointer("/source/themed/dark"))
        .or_else(|| icon.pointer("/source/themed/light"))
        .or_else(|| icon.pointer("/value/source/raw"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if raw.is_empty() || raw.starts_with("http://") || raw.starts_with("https://") {
        return Icon::None;
    }
    if raw.starts_with("data:") {
        return Icon::None;
    }
    let path = Path::new(raw);
    if path.is_absolute() && path.is_file() {
        return Icon::Path(path.to_path_buf());
    }
    Icon::Name(builtin_icon_name(raw).to_string())
}

fn builtin_icon_name(raw: &str) -> &str {
    match raw {
        "star" | "star-circle" => "starred",
        "copy-clipboard" => "edit-copy",
        "clipboard" => "edit-paste",
        "terminal" => "utilities-terminal",
        "globe" | "globe-01" | "link" => "web-browser",
        "trash" => "user-trash",
        "gear" | "cog" => "preferences-system",
        "folder" | "finder" => "folder",
        "document" | "text-document" | "blank-document" | "text" => "text-x-generic",
        "person" | "person-circle" => "avatar-default",
        "magnifying-glass" => "system-search",
        "check-circle" | "check" | "checkmark" | "check-rosette" => "emblem-ok",
        "x-mark-circle" | "xmark-circle" | "x-mark-top-right-square" => "dialog-error",
        "warning" | "exclamation-mark" | "exclamationmark" => "dialog-warning",
        "info" | "info-01" | "question-mark-circle" => "dialog-information",
        "clock" | "alarm-ringing" => "appointment-soon",
        "calendar" => "x-office-calendar",
        "bell" | "bell-disabled" => "preferences-desktop-notification-bell",
        "play" | "play-filled" => "media-playback-start",
        "pause" | "pause-filled" => "media-playback-pause",
        "stop" | "stop-filled" => "media-playback-stop",
        "download" => "document-save",
        "upload" => "document-send",
        "plus" | "plus-circle" | "plus-square" => "list-add",
        "minus" | "minus-circle" => "list-remove",
        "pencil" | "highlight" => "document-edit",
        "eye" => "view-reveal-symbolic",
        "eye-disabled" => "view-conceal-symbolic",
        "lock" | "lock-disabled" => "changes-prevent",
        "lock-unlocked" => "changes-allow",
        "bolt" | "bolt-disabled" => "battery-full-charging",
        "memory-chip" | "computer-chip" | "hardware-chip" => "cpu",
        "hard-drive" => "drive-harddisk",
        "wifi" | "wifi-disabled" => "network-wireless",
        "bluetooth" => "bluetooth",
        "battery" | "battery-charging" => "battery",
        "speaker-on" | "speaker-high" => "audio-volume-high",
        "speaker-off" | "speaker-disabled" => "audio-volume-muted",
        "monitor" | "desktop" => "video-display",
        "mobile" | "phone" => "phone",
        "message" | "speech-bubble" | "envelope" => "mail-message-new",
        "code" | "code-block" => "text-x-script",
        "bug" => "dialog-warning",
        "rocket" => "emblem-favorite",
        "house" | "home" => "user-home",
        "list" | "bullet-points" => "view-list",
        "app-window" | "app-window-list" => "preferences-system-windows",
        "arrow-right" | "arrow-right-circle" | "chevron-right" => "go-next",
        "arrow-left" | "arrow-left-circle" | "chevron-left" => "go-previous",
        "arrow-up" | "chevron-up" => "go-up",
        "arrow-down" | "chevron-down" => "go-down",
        "arrow-clockwise" | "rotate-clockwise" | "repeat" => "view-refresh",
        "power" | "power-disabled" => "system-shutdown",
        "moon" => "weather-clear-night",
        "sun" => "weather-clear",
        "cloud" => "weather-overcast",
        "music" => "audio-x-generic",
        "image" | "picture" => "image-x-generic",
        "video" | "camera" => "video-x-generic",
        "tag" => "tag",
        "key" => "dialog-password",
        "heart" | "heart-disabled" => "emblem-favorite",
        "circle" | "circle-filled" | "dot" => "media-record",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Value {
        serde_json::from_str(
            r###"{"type":"render","depth":1,"root":[{"id":1,"type":"nav-frame","props":{},"children":[
              {"id":2,"type":"list","props":{"searchBarPlaceholder":"Pick","onSearchTextChange":{"$cb":[2,"onSearchTextChange"]}},"children":[
                {"id":3,"type":"list-section","props":{"title":"Fruits"},"children":[
                  {"id":4,"type":"list-item","props":{"title":"Apple","subtitle":"🍎","keywords":["fruit"],
                     "icon":{"source":{"raw":"star"}},"accessories":[{"text":"red"},{"tag":{"value":"fresh"}}]},
                   "children":[
                     {"id":5,"type":"list-item-detail","props":{"markdown":"## Apple"},"children":[]},
                     {"id":6,"type":"action-panel","props":{},"children":[
                       {"id":7,"type":"action","props":{"title":"Toast","onAction":{"$cb":[7,"onAction"]}},"children":[]},
                       {"id":8,"type":"action-panel-section","props":{},"children":[
                         {"id":9,"type":"action","props":{"title":"Copy","onAction":{"$cb":[9,"onAction"]}},"children":[]}
                       ]}
                     ]}
                   ]}
                ]}
              ]}
            ]}]}"###,
        )
        .unwrap()
    }

    #[test]
    fn list_flattens_to_rows_with_actions_and_detail() {
        let view = parse_view(&tree());
        assert_eq!(view.placeholder, "Pick");
        assert_eq!(view.search_callback, Some((2, "onSearchTextChange".into())));
        assert!(
            !view.local_filter,
            "a search callback disables local filtering"
        );
        assert_eq!(view.rows.len(), 1);
        let row = &view.rows[0];
        assert_eq!(row.title, "Apple");
        assert_eq!(row.section, "Fruits");
        assert_eq!(row.subtitle, "🍎  ·  red  ·  fresh");
        assert_eq!(row.detail, "## Apple");
        assert_eq!(row.keywords, "fruit");
        assert!(matches!(&row.icon, Icon::Name(n) if n == "starred"));
        let titles: Vec<&str> = row.actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["Toast", "Copy"]);
        assert_eq!(row.actions[1].node, 9);
    }

    #[test]
    fn top_nav_frame_wins() {
        let mut msg = tree();
        msg["depth"] = json!(2);
        msg["root"].as_array_mut().unwrap().push(json!({
            "id": 20, "type": "nav-frame", "props": {},
            "children": [{"id": 21, "type": "detail", "props": {"markdown": "# Pushed\n\nbody"}, "children": []}]
        }));
        let view = parse_view(&msg);
        assert_eq!(view.depth, 2);
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].title, "Pushed");
        assert_eq!(view.rows[0].detail, "# Pushed\n\nbody");
    }

    #[test]
    fn list_without_callback_filters_locally() {
        let mut msg = tree();
        msg["root"][0]["children"][0]["props"] = json!({"filtering": true});
        assert!(parse_view(&msg).local_filter);
        msg["root"][0]["children"][0]["props"] = json!({});
        assert!(parse_view(&msg).local_filter);
    }

    #[test]
    fn manifest_commands_become_items() {
        let dir = std::env::temp_dir().join(format!("flint-ext-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("demo")).unwrap();
        fs::write(
            dir.join("demo/package.json"),
            r#"{"name":"demo","title":"Demo","commands":[{"name":"go","title":"Go","mode":"view"},{"name":"bar","title":"Bar","mode":"menu-bar"}]}"#,
        )
        .unwrap();
        let manifest = read_manifest(&dir.join("demo")).unwrap();
        assert_eq!(manifest.commands.len(), 2);
        assert!(read_manifest(&dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn entry_source_honors_filename() {
        let dir = std::env::temp_dir().join(format!("flint-src-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/nested")).unwrap();
        fs::write(dir.join("src/nested/go.tsx"), "export default function(){}").unwrap();
        assert_eq!(
            entry_source(&dir, "go", "src/nested/go.tsx").as_deref(),
            Some(dir.join("src/nested/go.tsx").as_path())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn icons_map_or_pass_through() {
        assert!(
            matches!(parse_icon(Some(&json!({"source": {"raw": "trash"}}))), Icon::Name(n) if n == "user-trash")
        );
        assert!(
            matches!(parse_icon(Some(&json!({"source": {"raw": "firefox"}}))), Icon::Name(n) if n == "firefox")
        );
        assert!(matches!(
            parse_icon(Some(&json!({"source": {"raw": "https://x/y.png"}}))),
            Icon::None
        ));
        assert!(matches!(
            parse_icon(Some(&json!({"fileIcon": "/usr/bin/ls"}))),
            Icon::Path(_)
        ));
        assert!(matches!(parse_icon(None), Icon::None));
    }

    #[test]
    fn form_fields_flatten_to_list_rows() {
        let msg = json!({
            "type": "render",
            "depth": 1,
            "root": [{
                "id": 1, "type": "nav-frame", "props": {},
                "children": [{
                    "id": 2, "type": "form",
                    "props": {"navigationTitle": "New thing"},
                    "children": [
                        {
                            "id": 3, "type": "action-panel", "props": {},
                            "children": [{
                                "id": 4, "type": "action",
                                "props": {"title": "Create", "onSubmit": {"$cb": [4, "onSubmit"]}},
                                "children": []
                            }]
                        },
                        {
                            "id": 5, "type": "text-field",
                            "props": {
                                "id": "name",
                                "title": "Name",
                                "value": "Ada",
                                "onChange": {"$cb": [5, "onChange"]}
                            },
                            "children": []
                        },
                        {
                            "id": 6, "type": "checkbox-field",
                            "props": {"id": "ok", "title": "Agree", "value": true},
                            "children": []
                        }
                    ]
                }]
            }]
        });
        let view = parse_view(&msg);
        assert!(view.is_form);
        assert!(view.notice.is_none());
        assert_eq!(view.title, "New thing");
        assert_eq!(view.rows.len(), 3);
        assert_eq!(view.rows[0].title, "Create");
        assert_eq!(view.rows[0].actions[0].prop, "onSubmit");
        assert_eq!(view.rows[1].title, "Name");
        assert_eq!(view.rows[1].field_id, "name");
        assert_eq!(view.rows[1].field_value, "Ada");
        assert_eq!(view.rows[1].field_on_change, Some((5, "onChange".into())));
        assert_eq!(view.rows[2].field_kind, "checkbox");
        assert_eq!(view.rows[2].field_value, "true");
        let values = form_values(&view);
        assert_eq!(values["name"], "Ada");
        assert_eq!(values["ok"], true);
    }

    #[test]
    fn confirm_prompt_uses_action_titles() {
        let prompt = confirm_prompt(&json!({
            "title": "Delete?",
            "message": "Gone forever",
            "primaryAction": {"title": "Delete"},
            "dismissAction": {"title": "Keep"}
        }));
        assert_eq!(prompt.primary, "Delete");
        assert_eq!(prompt.dismiss, "Keep");
        let items = confirm_items(&prompt);
        assert_eq!(items.len(), 2);
        assert!(matches!(
            items[0].action,
            Action::ExtensionConfirm { confirmed: true }
        ));
        assert!(matches!(
            items[1].action,
            Action::ExtensionConfirm { confirmed: false }
        ));
        let defaults = confirm_prompt(&json!({"title": "Sure?"}));
        assert_eq!(defaults.primary, "Confirm");
        assert_eq!(defaults.dismiss, "Cancel");
    }

    #[test]
    fn host_idle_is_five_minutes_not_a_poll() {
        let start = Instant::now();
        assert!(!host_idle_expired(start, start + Duration::from_secs(60)));
        assert!(host_idle_expired(
            start,
            start + Duration::from_secs(HOST_IDLE_SECS)
        ));
        assert_eq!(HOST_IDLE_SECS, 300);
    }

    #[test]
    fn host_script_requests_selected_text_and_confirm() {
        assert!(HOST_JS.contains("ui.getSelectedText"));
        assert!(HOST_JS.contains("ui.confirmAlert"));
        assert!(
            !HOST_JS.to_ascii_lowercase().contains("at-spi")
                && !HOST_JS.to_ascii_lowercase().contains("atspi")
        );
    }
}
