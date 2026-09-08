use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub general: General,
    pub files: Files,
    pub ai: Ai,
    pub voice: Voice,
    pub mcp: Vec<McpServer>,
    pub store: Store,
    pub connectors: Connectors,
    pub web: Web,
    pub media: Media,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Connectors {
    pub notion_client_id: String,
    pub todoist_client_id: String,
    pub outlook_client_id: String,
    pub tenor_key: String,
    pub apple_id: String,
    pub proton_user: String,
    pub caldav_url: String,
    /// Local Obsidian vault — added to file search roots when set.
    pub obsidian_vault: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub autostart: bool,
    pub attach_clipboard_to_ai: bool,
    /// Mixed root-search cap (apps, commands, fallbacks). Honored as written.
    pub max_results: usize,
    /// Third-party script-commands run as sh/python3/node with no signature.
    /// Off until the user opts in.
    #[serde(default)]
    pub allow_script_commands: bool,
    /// Spawning MCP servers (npx, etc.) to list tools for Ask AI.
    /// Off until the user opts in.
    #[serde(default)]
    pub allow_mcp: bool,
    /// Installed Vicinae / Raycast extensions run as Node processes with your
    /// user's privileges and no signature. Off until the user opts in.
    #[serde(default)]
    pub allow_extensions: bool,
    /// Rank and empty-state use the Hyprland focused window (class/title) captured
    /// when Flint opens. Off disables that and the 10s clipboard chips.
    #[serde(default = "default_true")]
    pub context_aware: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Files {
    /// Show file hits in the empty-prefix launcher, like Raycast Root Search.
    pub include_in_root: bool,
    /// Use `plocate`/`locate` so type queries can see files outside $HOME.
    pub system_wide: bool,
    pub include_hidden: bool,
    pub max_results: usize,
    /// Extra folders to scan (external drives, project roots). Home is always included.
    pub search_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Web {
    /// `ddg-html` (default) · `instant` (DuckDuckGo Instant Answer JSON only) · `off`.
    /// `searxng` and `brave` are documented fallbacks; they currently use `ddg-html`.
    #[serde(default = "default_web_provider")]
    pub provider: String,
}

impl Default for Web {
    fn default() -> Self {
        Self {
            provider: "ddg-html".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Media {
    /// When Enter opens the external player, hide Flint. Off (default) keeps
    /// the launcher (or restores query + selection after Hyprland closewindow /
    /// activewindow).
    pub hide_on_external_play: bool,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            include_in_root: true,
            system_wide: true,
            include_hidden: false,
            max_results: 250,
            search_roots: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Ai {
    /// ollama | openai | anthropic | google | custom
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    /// Kept only to migrate pre-0.2 config files into the credential store.
    #[serde(default, skip_serializing)]
    pub api_key: String,
    pub system_prompt: String,
    pub client_id: String,
    pub oauth_authorize_url: String,
    pub oauth_token_url: String,
    pub oauth_scopes: String,
    pub oauth_project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Voice {
    /// in-app (pw-record + voxtype transcribe)
    pub engine: String,
    pub language: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub script_commands_dir: String,
}

fn default_true() -> bool {
    true
}

fn default_web_provider() -> String {
    "ddg-html".into()
}

impl Default for General {
    fn default() -> Self {
        Self {
            autostart: true,
            attach_clipboard_to_ai: false,
            max_results: 48,
            allow_script_commands: false,
            allow_mcp: false,
            allow_extensions: false,
            context_aware: true,
        }
    }
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            provider: "ollama".into(),
            model: "qwen3.5:9b-hermes".into(),
            endpoint: "http://127.0.0.1:11434".into(),
            api_key: String::new(),
            system_prompt: "You are Flint, a fast desktop assistant on Linux. Be concise, exact, and useful. Prefer short answers unless the user asks for depth.".into(),
            client_id: String::new(),
            oauth_authorize_url: String::new(),
            oauth_token_url: String::new(),
            oauth_scopes: String::new(),
            oauth_project_id: String::new(),
        }
    }
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            engine: "in-app".into(),
            language: "en".into(),
            model: String::new(),
        }
    }
}

impl Default for Store {
    fn default() -> Self {
        Self {
            script_commands_dir: crate::paths::data_dir()
                .join("store/script-commands")
                .to_string_lossy()
                .into_owned(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let path = path();
        let mut settings: Settings = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let defaults = Settings::default();
        let mut dirty = false;

        if settings.ai.model.is_empty() {
            settings.ai.model = defaults.ai.model.clone();
            dirty = true;
        }
        if !settings.ai.api_key.is_empty() {
            let legacy_key = std::mem::take(&mut settings.ai.api_key);
            if crate::auth::save_api_key(&settings.ai.provider, &legacy_key).is_ok() {
                dirty = true;
            } else {
                settings.ai.api_key = legacy_key;
            }
        }
        if !settings.connectors.tenor_key.is_empty() {
            let legacy = std::mem::take(&mut settings.connectors.tenor_key);
            if crate::auth::save_api_key("tenor", &legacy).is_ok() {
                dirty = true;
            } else {
                settings.connectors.tenor_key = legacy;
            }
        }
        if settings.ai.endpoint.is_empty() {
            settings.ai.endpoint = defaults.ai.endpoint.clone();
            dirty = true;
        }
        if settings.ai.system_prompt.contains("Rayblast") {
            settings.ai.system_prompt = defaults.ai.system_prompt.clone();
            dirty = true;
        }
        if settings.ai.provider == "openai"
            && (settings.ai.endpoint.contains("127.0.0.1:11434")
                || settings.ai.endpoint.contains("localhost:11434"))
        {
            settings.ai.provider = "ollama".into();
            dirty = true;
        }
        if settings.voice.engine == "voxtype" || settings.voice.engine == "pw-record" {
            settings.voice.engine = "in-app".into();
            dirty = true;
        }
        if settings.store.script_commands_dir.contains("/rayblast/") {
            settings.store.script_commands_dir = defaults.store.script_commands_dir.clone();
            dirty = true;
        }

        if dirty || !path.exists() {
            settings.save();
        } else {
            paths::tighten_private_file(&path);
            sync_autostart(settings.general.autostart);
        }
        settings
    }

    pub fn save(&self) {
        paths::ensure();
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = paths::write_private(&path(), raw);
        }
        sync_autostart(self.general.autostart);
    }

    pub fn set_provider(&mut self, id: &str) -> String {
        let id = id.trim();
        self.ai.provider = match id {
            "openai" | "anthropic" | "google" | "xai" | "custom" | "ollama" => id.into(),
            "grok" => "xai".into(),
            "chatgpt" => "openai".into(),
            "claude" => "anthropic".into(),
            _ => self.ai.provider.clone(),
        };
        match self.ai.provider.as_str() {
            "ollama" => {
                self.ai.endpoint = "http://127.0.0.1:11434".into();
            }
            "openai" => {
                self.ai.endpoint = "https://api.openai.com".into();
            }
            "anthropic" => {
                self.ai.endpoint = "https://api.anthropic.com".into();
            }
            "google" => {
                self.ai.endpoint = "https://generativelanguage.googleapis.com/v1beta/openai".into();
            }
            "xai" => {
                crate::xai::apply_defaults(self);
                return format!("AI provider → {}", self.ai.provider);
            }
            _ => {}
        }
        self.save();
        format!("AI provider → {}", self.ai.provider)
    }

    pub fn add_mcp(&mut self, name: &str, command: &str, args: Vec<String>) -> Result<(), String> {
        let name = name.trim();
        let command = command.trim();
        if name.is_empty() || command.is_empty() {
            return Err("MCP name and command are required".into());
        }
        if !crate::mcp::is_safe_mcp_command(command) {
            return Err("MCP command must be npx".into());
        }
        if self.mcp.iter().any(|s| s.name == name) {
            return Err("An MCP server with that name already exists".into());
        }
        self.mcp.push(McpServer {
            name: name.into(),
            command: command.into(),
            args,
            enabled: true,
        });
        self.save();
        Ok(())
    }

    pub fn remove_mcp(&mut self, name: &str) {
        self.mcp.retain(|s| s.name != name);
        self.save();
    }

    pub fn set_mcp_enabled(&mut self, name: &str, enabled: bool) {
        if let Some(server) = self.mcp.iter_mut().find(|s| s.name == name) {
            server.enabled = enabled;
            self.save();
        }
    }

    pub fn use_model(&mut self, source: &str, model: &str, endpoint: &str, api: &str) -> String {
        self.ai.provider = if api == "ollama" || source == "ollama" {
            "ollama".into()
        } else {
            source.to_string()
        };
        self.ai.model = model.to_string();
        if !endpoint.is_empty() {
            self.ai.endpoint = endpoint.to_string();
        }
        self.save();
        format!("Using {model} via {source}")
    }

    pub fn apply(&mut self, id: &str, typed: &str) -> String {
        let typed = typed.trim();
        let msg = match id {
            "set:autostart" => {
                self.general.autostart = !self.general.autostart;
                if self.general.autostart {
                    "Launch at login on"
                } else {
                    "Launch at login off"
                }
                .into()
            }
            "set:clip-ai" => {
                self.general.attach_clipboard_to_ai = !self.general.attach_clipboard_to_ai;
                if self.general.attach_clipboard_to_ai {
                    "Clipboard will be attached to Ask AI"
                } else {
                    "Clipboard stays out of Ask AI"
                }
                .into()
            }
            "set:scripts" => {
                self.general.allow_script_commands = !self.general.allow_script_commands;
                if self.general.allow_script_commands {
                    "Unsigned script-commands ON — they run as sh/python3/node with no signature"
                } else {
                    "Unsigned script-commands off"
                }
                .into()
            }
            "set:extensions" => {
                self.general.allow_extensions = !self.general.allow_extensions;
                if self.general.allow_extensions {
                    "Extensions ON — installed Vicinae/Raycast extensions run as Node with no signature"
                } else {
                    "Extensions off"
                }
                .into()
            }
            "set:mcp" => {
                self.general.allow_mcp = !self.general.allow_mcp;
                if self.general.allow_mcp {
                    "MCP tool listing ON — Flint will spawn configured servers"
                } else {
                    "MCP tool listing off"
                }
                .into()
            }
            "set:context" => {
                self.general.context_aware = !self.general.context_aware;
                if self.general.context_aware {
                    "Context-aware search ON — focused window class ranks matching items"
                } else {
                    "Context-aware search off — no window class or 10s clipboard chips"
                }
                .into()
            }
            "set:web" => {
                let typed = typed.trim();
                if !typed.is_empty() {
                    self.web.provider = match typed.to_ascii_lowercase().as_str() {
                        "off" => "off".into(),
                        "instant" => "instant".into(),
                        "searxng" | "brave" | "ddg-html" | "ddg" | "duckduckgo" => {
                            "ddg-html".into()
                        }
                        _ => self.web.provider.clone(),
                    };
                } else {
                    self.web.provider = match self.web.provider.as_str() {
                        "ddg-html" => "instant".into(),
                        "instant" => "off".into(),
                        _ => "ddg-html".into(),
                    };
                }
                match self.web.provider.as_str() {
                    "off" => "Web search off — Flint will not fetch DuckDuckGo".into(),
                    "instant" => "Web search → DuckDuckGo Instant Answer JSON only".into(),
                    _ => "Web search → DuckDuckGo HTML (searxng/brave use this too)".into(),
                }
            }
            "set:media-hide" => {
                self.media.hide_on_external_play = !self.media.hide_on_external_play;
                if self.media.hide_on_external_play {
                    "Hide Flint when Enter opens an external player".into()
                } else {
                    "Stay open (or restore) when Enter opens an external player".into()
                }
            }
            "set:files-root" => {
                self.files.include_in_root = !self.files.include_in_root;
                if self.files.include_in_root {
                    "Files appear in root search"
                } else {
                    "Files only appear in Search Files"
                }
                .into()
            }
            "set:files-system" => {
                self.files.system_wide = !self.files.system_wide;
                if self.files.system_wide {
                    "System-wide file search ON — uses locate/plocate when available"
                } else {
                    "File search stays in home and extra folders"
                }
                .into()
            }
            "set:files-hidden" => {
                self.files.include_hidden = !self.files.include_hidden;
                if self.files.include_hidden {
                    "Hidden files included in Search Files"
                } else {
                    "Hidden files excluded"
                }
                .into()
            }
            "set:max-results" if !typed.is_empty() => match typed.parse::<usize>() {
                Ok(n) if (8..=500).contains(&n) => {
                    self.general.max_results = n.min(80);
                    self.files.max_results = n.max(48);
                    format!(
                        "Result limits → root {} · files {}",
                        self.general.max_results, self.files.max_results
                    )
                }
                _ => "Type a number between 8 and 500".into(),
            },
            "set:provider" => {
                self.ai.provider = match self.ai.provider.as_str() {
                    "ollama" => "openai".into(),
                    "openai" => "anthropic".into(),
                    "anthropic" => "google".into(),
                    "google" => "xai".into(),
                    "xai" => "custom".into(),
                    _ => "ollama".into(),
                };
                match self.ai.provider.as_str() {
                    "ollama" => {
                        self.ai.endpoint = "http://127.0.0.1:11434".into();
                    }
                    "openai" => {
                        self.ai.endpoint = "https://api.openai.com".into();
                    }
                    "anthropic" => {
                        self.ai.endpoint = "https://api.anthropic.com".into();
                    }
                    "google" => {
                        self.ai.endpoint =
                            "https://generativelanguage.googleapis.com/v1beta/openai".into();
                    }
                    "xai" => {
                        self.ai.endpoint = crate::xai::CHAT_ENDPOINT.into();
                        self.ai.model = crate::xai::DEFAULT_MODEL.into();
                    }
                    _ => {}
                }
                format!("AI provider → {}", self.ai.provider)
            }
            "set:model" if !typed.is_empty() => {
                if typed.len() > 256 || typed.chars().any(char::is_control) {
                    "Model name is invalid".into()
                } else {
                    self.ai.model = typed.to_string();
                    format!("Model → {}", self.ai.model)
                }
            }
            "set:endpoint" if !typed.is_empty() => match validate_endpoint(typed) {
                Ok(()) => {
                    self.ai.endpoint = typed.trim_end_matches('/').to_string();
                    format!("Endpoint → {}", self.ai.endpoint)
                }
                Err(error) => error,
            },
            "set:apikey" if !typed.is_empty() => {
                match crate::auth::save_api_key(&self.ai.provider, typed) {
                    Ok(storage) => format!("API key saved in {storage}"),
                    Err(error) => error,
                }
            }
            "set:tenor-key" if !typed.is_empty() => {
                match crate::auth::save_api_key("tenor", typed) {
                    Ok(storage) => {
                        self.connectors.tenor_key.clear();
                        format!("Tenor API key saved in {storage}")
                    }
                    Err(error) => error,
                }
            }
            "set:notion-client" if !typed.is_empty() => {
                self.connectors.notion_client_id = typed.to_string();
                "Notion OAuth client ID saved".into()
            }
            "set:todoist-client" if !typed.is_empty() => {
                self.connectors.todoist_client_id = typed.to_string();
                "Todoist OAuth client ID saved".into()
            }
            "set:notion-secret" if !typed.is_empty() => {
                match crate::auth::save_api_key("notion-secret", typed) {
                    Ok(storage) => format!("Notion client secret saved in {storage}"),
                    Err(error) => error,
                }
            }
            "set:todoist-secret" if !typed.is_empty() => {
                match crate::auth::save_api_key("todoist-secret", typed) {
                    Ok(storage) => format!("Todoist client secret saved in {storage}"),
                    Err(error) => error,
                }
            }
            "set:outlook-client" if !typed.is_empty() => {
                self.connectors.outlook_client_id = typed.to_string();
                "Outlook OAuth client ID saved".into()
            }
            "set:apple-id" if !typed.is_empty() => {
                self.connectors.apple_id = typed.to_string();
                "Apple ID saved".into()
            }
            "set:proton-user" if !typed.is_empty() => {
                self.connectors.proton_user = typed.to_string();
                "Proton user saved".into()
            }
            "set:caldav-url" if !typed.is_empty() => {
                match validate_https_url(typed, "CalDAV URL") {
                    Ok(()) => {
                        self.connectors.caldav_url = typed.to_string();
                        "CalDAV URL saved".into()
                    }
                    Err(error) => error,
                }
            }
            "set:obsidian-vault" if !typed.is_empty() => {
                let path = typed.trim().trim_end_matches('/').to_string();
                if path.len() > 4096 || path.contains('\0') {
                    "Obsidian vault path is invalid".into()
                } else {
                    self.connectors.obsidian_vault = path.clone();
                    if !self.files.search_roots.iter().any(|r| r == &path) {
                        self.files.search_roots.push(path);
                    }
                    "Obsidian vault added to file search".into()
                }
            }
            "set:apple-password" if !typed.is_empty() => {
                match crate::auth::save_api_key("apple-calendar", typed) {
                    Ok(storage) => format!("Apple app password saved in {storage}"),
                    Err(error) => error,
                }
            }
            "set:proton-password" if !typed.is_empty() => {
                match crate::auth::save_api_key("proton-calendar", typed) {
                    Ok(storage) => format!("Proton app password saved in {storage}"),
                    Err(error) => error,
                }
            }
            "set:clear-apikey" => match crate::auth::clear_api_key(&self.ai.provider) {
                Ok(()) => format!("{} API key removed", self.ai.provider),
                Err(error) => error,
            },
            "set:client-id" if !typed.is_empty() => {
                if typed.len() > 2048 || typed.chars().any(char::is_control) {
                    "OAuth client ID is invalid".into()
                } else {
                    self.ai.client_id = typed.to_string();
                    "OAuth client ID saved".into()
                }
            }
            "set:oauth-authorize" if !typed.is_empty() => {
                match validate_https_url(typed, "OAuth authorize URL") {
                    Ok(()) => {
                        self.ai.oauth_authorize_url = typed.to_string();
                        "OAuth authorize URL saved".into()
                    }
                    Err(error) => error,
                }
            }
            "set:oauth-token" if !typed.is_empty() => {
                match validate_https_url(typed, "OAuth token URL") {
                    Ok(()) => {
                        self.ai.oauth_token_url = typed.to_string();
                        "OAuth token URL saved".into()
                    }
                    Err(error) => error,
                }
            }
            "set:oauth-scopes" if !typed.is_empty() => {
                if typed.len() > 4096 || typed.chars().any(char::is_control) {
                    "OAuth scopes are invalid".into()
                } else {
                    self.ai.oauth_scopes = typed.to_string();
                    "OAuth scopes saved".into()
                }
            }
            "set:oauth-project" if !typed.is_empty() => {
                if typed.len() > 256
                    || typed.bytes().any(|byte| {
                        !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':' | b'.'))
                    })
                {
                    "OAuth quota project ID is invalid".into()
                } else {
                    self.ai.oauth_project_id = typed.to_string();
                    "OAuth quota project ID saved".into()
                }
            }
            "set:voice-lang" if !typed.is_empty() => {
                self.voice.language = typed.to_string();
                format!("Voice language → {}", self.voice.language)
            }
            "set:voice-model" if !typed.is_empty() => {
                self.voice.model = typed.to_string();
                format!("Voice model → {}", self.voice.model)
            }
            other if other.starts_with("set:ext-prefs:") => {
                "Extension preferences use package.json defaults. Editing them here is not implemented."
                    .into()
            }
            _ => "Nothing to change — type a value, then Enter".into(),
        };
        self.save();
        msg
    }
}

fn validate_endpoint(input: &str) -> Result<(), String> {
    let url = url::Url::parse(input).map_err(|_| "AI endpoint is not a valid URL".to_string())?;
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("AI endpoint must not contain credentials or a fragment".into());
    }
    match url.scheme() {
        "https" => Ok(()),
        "http" if is_loopback_host(url.host_str().unwrap_or_default()) => Ok(()),
        "http" => Err("Remote AI endpoints must use HTTPS".into()),
        _ => Err("AI endpoint must use HTTPS, or HTTP on loopback".into()),
    }
}

fn validate_https_url(input: &str, label: &str) -> Result<(), String> {
    let url = url::Url::parse(input).map_err(|_| format!("{label} is not a valid URL"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(format!(
            "{label} must be HTTPS without credentials or a fragment"
        ));
    }
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host
            .parse::<std::net::IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

pub fn path() -> PathBuf {
    crate::paths::config_dir().join("config.json")
}

fn sync_autostart(enabled: bool) {
    let Some(config) = dirs::config_dir() else {
        return;
    };
    let dir = config.join("autostart");
    let desktop = dir.join("flint.desktop");
    let legacy = dir.join("rayblast.desktop");
    if legacy.exists() {
        let _ = fs::remove_file(legacy);
    }
    if enabled {
        let _ = fs::create_dir_all(&dir);
        let body = "\
[Desktop Entry]
Type=Application
Name=Flint
Comment=Flint command launcher
Exec=flint --daemon
Icon=flint
Terminal=false
X-GNOME-Autostart-enabled=true
";
        let _ = fs::write(desktop, body);
    } else if desktop.exists() {
        let _ = fs::remove_file(desktop);
    }
}

#[cfg(test)]
mod tests {
    use super::{Settings, validate_endpoint, validate_https_url};

    #[test]
    fn default_provider_is_ollama() {
        let s = Settings::default();
        assert_eq!(s.ai.provider, "ollama");
        assert!(s.general.autostart);
        assert!(!s.general.allow_script_commands);
        assert!(!s.general.allow_mcp);
        assert!(!s.general.allow_extensions);
        assert!(s.general.context_aware);
        assert_eq!(s.web.provider, "ddg-html");
        assert!(!s.media.hide_on_external_play);
        assert_eq!(s.voice.engine, "in-app");
        assert!(s.files.include_in_root);
        assert!(s.files.system_wide);
        assert!(s.files.max_results >= 80);
        assert!(s.general.max_results > 12);
    }

    #[test]
    fn missing_context_aware_defaults_on() {
        let s: Settings = serde_json::from_str(r#"{"general":{"autostart":true}}"#).unwrap();
        assert!(s.general.context_aware);
    }

    #[test]
    fn missing_web_provider_defaults_to_ddg_html() {
        let s: Settings = serde_json::from_str(r#"{"general":{"autostart":true}}"#).unwrap();
        assert_eq!(s.web.provider, "ddg-html");
    }

    #[test]
    fn endpoint_policy_allows_local_http_and_requires_remote_https() {
        assert!(validate_endpoint("http://127.0.0.1:11434").is_ok());
        assert!(validate_endpoint("http://[::1]:8080").is_ok());
        assert!(validate_endpoint("https://api.example.com/v1").is_ok());
        assert!(validate_endpoint("http://api.example.com/v1").is_err());
        assert!(validate_endpoint("https://user@example.com/v1").is_err());
    }

    #[test]
    fn oauth_urls_require_uncredentialed_https() {
        assert!(validate_https_url("https://id.example.com/auth", "authorize").is_ok());
        assert!(validate_https_url("http://id.example.com/auth", "authorize").is_err());
        assert!(validate_https_url("https://id.example.com/auth#x", "authorize").is_err());
    }

    #[test]
    fn mcp_add_rejects_empty_and_non_npx() {
        let mut s = Settings::default();
        assert!(s.add_mcp("", "npx", Vec::new()).is_err());
        assert!(s.add_mcp("evil", "bash", Vec::new()).is_err());
    }
}
