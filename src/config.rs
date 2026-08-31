use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: General,
    pub ai: Ai,
    pub voice: Voice,
    pub mcp: Vec<McpServer>,
    pub store: Store,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub autostart: bool,
    pub attach_clipboard_to_ai: bool,
    pub max_results: usize,
    /// Third-party script-commands run as sh/python3/node with no signature.
    /// Off until the user opts in.
    #[serde(default)]
    pub allow_script_commands: bool,
    /// Spawning MCP servers (npx, etc.) to list tools for Ask AI.
    /// Off until the user opts in.
    #[serde(default)]
    pub allow_mcp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Ai {
    /// ollama | openai | anthropic | google | custom
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub api_key: String,
    pub system_prompt: String,
    pub client_id: String,
    pub oauth_authorize_url: String,
    pub oauth_token_url: String,
    pub oauth_scopes: String,
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            general: General::default(),
            ai: Ai::default(),
            voice: Voice::default(),
            mcp: Vec::new(),
            store: Store::default(),
        }
    }
}

impl Default for General {
    fn default() -> Self {
        Self {
            autostart: true,
            attach_clipboard_to_ai: false,
            max_results: 12,
            allow_script_commands: false,
            allow_mcp: false,
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
        if settings
            .store
            .script_commands_dir
            .contains("/rayblast/")
        {
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
            "set:mcp" => {
                self.general.allow_mcp = !self.general.allow_mcp;
                if self.general.allow_mcp {
                    "MCP tool listing ON — Flint will spawn configured servers"
                } else {
                    "MCP tool listing off"
                }
                .into()
            }
            "set:provider" => {
                self.ai.provider = match self.ai.provider.as_str() {
                    "ollama" => "openai".into(),
                    "openai" => "anthropic".into(),
                    "anthropic" => "google".into(),
                    "google" => "custom".into(),
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
                    _ => {}
                }
                format!("AI provider → {}", self.ai.provider)
            }
            "set:model" if !typed.is_empty() => {
                self.ai.model = typed.to_string();
                format!("Model → {}", self.ai.model)
            }
            "set:endpoint" if !typed.is_empty() => {
                if !(typed.starts_with("http://") || typed.starts_with("https://")) {
                    "AI endpoint must start with http:// or https://".into()
                } else if typed.contains('@') {
                    "AI endpoint must not include credentials".into()
                } else {
                    self.ai.endpoint = typed.to_string();
                    format!("Endpoint → {}", self.ai.endpoint)
                }
            }
            "set:apikey" if !typed.is_empty() => {
                self.ai.api_key = typed.to_string();
                "API key saved".into()
            }
            "set:client-id" if !typed.is_empty() => {
                self.ai.client_id = typed.to_string();
                "OAuth client ID saved".into()
            }
            "set:oauth-authorize" if !typed.is_empty() => {
                if !typed.starts_with("https://") {
                    "OAuth authorize URL must start with https://".into()
                } else {
                    self.ai.oauth_authorize_url = typed.to_string();
                    "OAuth authorize URL saved".into()
                }
            }
            "set:oauth-token" if !typed.is_empty() => {
                if !typed.starts_with("https://") {
                    "OAuth token URL must start with https://".into()
                } else {
                    self.ai.oauth_token_url = typed.to_string();
                    "OAuth token URL saved".into()
                }
            }
            "set:oauth-scopes" if !typed.is_empty() => {
                self.ai.oauth_scopes = typed.to_string();
                "OAuth scopes saved".into()
            }
            "set:voice-lang" if !typed.is_empty() => {
                self.voice.language = typed.to_string();
                format!("Voice language → {}", self.voice.language)
            }
            "set:voice-model" if !typed.is_empty() => {
                self.voice.model = typed.to_string();
                format!("Voice model → {}", self.voice.model)
            }
            _ => "Nothing to change — type a value, then Enter".into(),
        };
        self.save();
        msg
    }
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
    use super::Settings;

    #[test]
    fn default_provider_is_ollama() {
        let s = Settings::default();
        assert_eq!(s.ai.provider, "ollama");
        assert!(s.general.autostart);
        assert!(!s.general.allow_script_commands);
        assert!(!s.general.allow_mcp);
        assert_eq!(s.voice.engine, "in-app");
    }
}
