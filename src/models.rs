use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::item::{Action, Icon, Item, Kind};

#[derive(Debug, Clone)]
pub struct LocalModel {
    pub id: String,
    pub label: String,
    pub source: String,
    pub endpoint: String,
    pub api: String,
}

struct Cache {
    at: Instant,
    models: Vec<LocalModel>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

pub fn warm() {
    let _ = discover();
}

pub fn discover() -> Vec<LocalModel> {
    if let Ok(guard) = CACHE.lock()
        && let Some(cache) = guard.as_ref()
        && cache.at.elapsed() < Duration::from_secs(45)
    {
        return cache.models.clone();
    }
    let models = probe();
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cache {
            at: Instant::now(),
            models: models.clone(),
        });
    }
    models
}

pub fn items() -> Vec<Item> {
    discover()
        .into_iter()
        .map(|m| Item {
            id: format!("model:{}:{}", m.source, m.id),
            title: m.label.clone(),
            subtitle: format!("{} · {}", m.source, m.endpoint),
            keywords: format!("model local {} {} ollama lmstudio", m.source, m.id),
            kind: Kind::Ai,
            icon: Icon::Name("computer".into()),
            action: Action::UseModel {
                source: m.source,
                model: m.id,
                endpoint: m.endpoint,
                api: m.api,
            },
        })
        .collect()
}

fn probe() -> Vec<LocalModel> {
    let mut out = Vec::new();
    out.extend(ollama_models());
    out.extend(openai_compat(
        "lmstudio",
        "LM Studio",
        "http://127.0.0.1:1234",
    ));
    out.extend(openai_compat(
        "llamacpp",
        "llama.cpp",
        "http://127.0.0.1:8080",
    ));
    if out.is_empty() {
        if which("ollama") {
            out.push(LocalModel {
                id: "ollama-serve".into(),
                label: "Start Ollama".into(),
                source: "ollama".into(),
                endpoint: "http://127.0.0.1:11434".into(),
                api: "ollama".into(),
            });
        }
        if which("lm-studio") {
            out.push(LocalModel {
                id: "lmstudio-app".into(),
                label: "LM Studio is installed · start the local server".into(),
                source: "lmstudio".into(),
                endpoint: "http://127.0.0.1:1234".into(),
                api: "openai".into(),
            });
        }
    }
    out
}

fn ollama_models() -> Vec<LocalModel> {
    let Some(raw) = get_json("http://127.0.0.1:11434/api/tags") else {
        return Vec::new();
    };
    raw.get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let name = m.get("name")?.as_str()?.to_string();
            Some(LocalModel {
                label: format!("{name} · Ollama"),
                id: name,
                source: "ollama".into(),
                endpoint: "http://127.0.0.1:11434".into(),
                api: "ollama".into(),
            })
        })
        .collect()
}

fn openai_compat(source: &str, title: &str, endpoint: &str) -> Vec<LocalModel> {
    let url = format!("{}/v1/models", endpoint.trim_end_matches('/'));
    let Some(raw) = get_json(&url) else {
        return Vec::new();
    };
    raw.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            Some(LocalModel {
                label: format!("{id} · {title}"),
                id,
                source: source.into(),
                endpoint: endpoint.into(),
                api: "openai".into(),
            })
        })
        .collect()
}

fn get_json(url: &str) -> Option<Value> {
    let output = Command::new("curl")
        .args([
            "-sS",
            "--fail",
            "--max-time",
            "1",
            "--proto",
            "=http,https",
            "-H",
            "Accept: application/json",
            url,
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn reads_ollama_names() {
        let raw = json!({"models":[{"name":"qwen3.5:9b-hermes"}]});
        let name = raw["models"][0]["name"].as_str().unwrap();
        assert_eq!(name, "qwen3.5:9b-hermes");
    }
}
