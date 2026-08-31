use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::{Value, json};

use crate::auth;
use crate::config::Settings;
use crate::mcp;

pub struct Reply {
    pub text: String,
    pub source: String,
}

pub fn ask(prompt: &str, settings: &Settings) -> Result<Reply, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Ask something first".into());
    }
    let mut system = settings.ai.system_prompt.clone();
    if settings.general.allow_mcp
        && let Some(tools) = mcp::tool_primer(&settings.mcp)
    {
        system.push_str("\n\n");
        system.push_str(&tools);
    }
    match settings.ai.provider.as_str() {
        "openai" | "google" | "custom" | "lmstudio" | "llamacpp" => {
            let credential = credential(settings)?;
            let label = match settings.ai.provider.as_str() {
                "google" => "Google",
                "lmstudio" => "LM Studio",
                "llamacpp" => "llama.cpp",
                "custom" => "Custom",
                _ => "OpenAI",
            };
            chat_completions(
                settings,
                &chat_url(&settings.ai.endpoint, "/v1/chat/completions"),
                &credential,
                &settings.ai.model,
                &system,
                prompt,
                label,
            )
        }
        "anthropic" => anthropic(settings, &system, prompt),
        _ => ollama(settings, &system, prompt),
    }
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

fn ollama(settings: &Settings, system: &str, prompt: &str) -> Result<Reply, String> {
    let url = chat_url(&settings.ai.endpoint, "/api/chat");
    let body = json!({
        "model": settings.ai.model,
        "stream": false,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": prompt}
        ]
    });
    let raw = http_json("POST", &url, &body, &[])?;
    let text = raw
        .pointer("/message/content")
        .and_then(Value::as_str)
        .or_else(|| raw.get("response").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("Ollama returned an empty answer".into());
    }
    Ok(Reply {
        text,
        source: format!("Ollama · {}", settings.ai.model),
    })
}

fn chat_completions(
    settings: &Settings,
    url: &str,
    credential: &Credential,
    model: &str,
    system: &str,
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
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": prompt}
        ]
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
    let raw = http_json("POST", url, &body, &headers)?;
    let text = raw
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(format!("{label} returned an empty answer"));
    }
    Ok(Reply {
        text,
        source: format!("{label} · {}", settings.ai.model),
    })
}

fn anthropic(settings: &Settings, system: &str, prompt: &str) -> Result<Reply, String> {
    let key = auth::api_key("anthropic")
        .ok_or("Anthropic consumer subscriptions do not include API access. Add an Anthropic API key in Settings.")?;
    let url = chat_url(&settings.ai.endpoint, "/v1/messages");
    let endpoint = if settings.ai.endpoint.contains("anthropic") {
        url
    } else {
        "https://api.anthropic.com/v1/messages".into()
    };
    let body = json!({
        "model": settings.ai.model,
        "max_tokens": 1024,
        "system": system,
        "messages": [{"role": "user", "content": prompt}]
    });
    let raw = http_json(
        "POST",
        &endpoint,
        &body,
        &[("x-api-key", &key), ("anthropic-version", "2023-06-01")],
    )?;
    let text = raw
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
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
    if base.ends_with(path) || base.contains("/chat/completions") || base.contains("/messages") {
        base.to_string()
    } else {
        format!("{base}{path}")
    }
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
    use super::parse_url;

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
}
