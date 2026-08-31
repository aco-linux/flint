use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::Settings;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tokens {
    pub provider: String,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default)]
    pub account: String,
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub id: &'static str,
    pub title: &'static str,
    pub authorize: &'static str,
    pub token: &'static str,
    pub scopes: &'static str,
    pub chat_endpoint: &'static str,
    pub default_model: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "openai",
        title: "OpenAI",
        authorize: "https://auth.openai.com/oauth/authorize",
        token: "https://auth.openai.com/oauth/token",
        scopes: "openid profile email api.completions",
        chat_endpoint: "https://api.openai.com",
        default_model: "gpt-4.1",
    },
    Provider {
        id: "google",
        title: "Google Gemini",
        authorize: "https://accounts.google.com/o/oauth2/v2/auth",
        token: "https://oauth2.googleapis.com/token",
        scopes: "https://www.googleapis.com/auth/generative-language.retriever openid email",
        chat_endpoint: "https://generativelanguage.googleapis.com/v1beta/openai",
        default_model: "gemini-2.0-flash",
    },
];

pub fn path() -> PathBuf {
    crate::paths::config_dir().join("auth.json")
}

pub fn load() -> Option<Tokens> {
    let raw = fs::read_to_string(path()).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save(tokens: &Tokens) {
    crate::paths::ensure();
    if let Ok(raw) = serde_json::to_string_pretty(tokens) {
        let _ = crate::paths::write_private(&path(), raw);
    }
}

pub fn clear() {
    let _ = fs::remove_file(path());
}

pub fn bearer() -> Option<String> {
    let tokens = load()?;
    if tokens.access_token.is_empty() {
        return None;
    }
    if tokens.expires_at > 0 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now + 30 >= tokens.expires_at {
            return None;
        }
    }
    Some(tokens.access_token)
}

pub fn signed_in_label() -> String {
    match load() {
        Some(t) if !t.access_token.is_empty() => {
            if t.account.is_empty() {
                format!("Signed in · {}", t.provider)
            } else {
                format!("Signed in as {} · {}", t.account, t.provider)
            }
        }
        _ => "Not signed in".into(),
    }
}

pub fn login(provider_id: &str, settings: &Settings) -> Result<String, String> {
    let preset = PROVIDERS.iter().find(|p| p.id == provider_id);
    let authorize = if provider_id == "custom" {
        settings.ai.oauth_authorize_url.trim().to_string()
    } else {
        preset
            .map(|p| p.authorize.to_string())
            .unwrap_or_default()
    };
    let token_url = if provider_id == "custom" {
        settings.ai.oauth_token_url.trim().to_string()
    } else {
        preset
            .map(|p| p.token.to_string())
            .unwrap_or_default()
    };
    let scopes = if provider_id == "custom" {
        settings.ai.oauth_scopes.clone()
    } else {
        preset.map(|p| p.scopes.to_string()).unwrap_or_default()
    };
    if authorize.is_empty() {
        return Err("Set an OAuth authorize URL in Settings, then try again".into());
    }
    if !authorize.starts_with("https://") {
        return Err("OAuth authorize URL must start with https://".into());
    }
    if token_url.is_empty() {
        return Err(
            "Set an OAuth token URL in Settings. Flint will not store the authorization code as a bearer token."
                .into(),
        );
    }
    if !token_url.starts_with("https://") {
        return Err("OAuth token URL must start with https://".into());
    }
    let client_id = settings.ai.client_id.trim();
    if client_id.is_empty() {
        return Err("Add an OAuth client ID in Settings — Flint never ships a shared secret".into());
    }

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).ok();
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let verifier = random_token(32)?;
    let challenge = pkce_challenge(&verifier)?;
    let state = random_token(16)?;

    let mut url = authorize;
    let join = if url.contains('?') { "&" } else { "?" };
    url.push_str(join);
    url.push_str(&format!(
        "response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
        urlencode(client_id),
        urlencode(&redirect),
        urlencode(&state),
        urlencode(&challenge),
    ));
    if !scopes.is_empty() {
        url.push_str("&scope=");
        url.push_str(&urlencode(&scopes));
    }

    open_browser(&url)?;
    let (code, returned_state) = wait_for_code(&listener)?;
    if returned_state != state {
        return Err("OAuth state mismatch — sign-in cancelled".into());
    }
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencode(&code),
        urlencode(&redirect),
        urlencode(client_id),
        urlencode(&verifier),
    );
    let raw = token_request(&token_url, &body)?;
    let access = json_str(&raw, "access_token").ok_or("Provider did not return an access token")?;
    let refresh = json_str(&raw, "refresh_token").unwrap_or_default();
    let expires_in = json_u64(&raw, "expires_in").unwrap_or(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let account = json_str(&raw, "email")
        .or_else(|| json_str(&raw, "name"))
        .unwrap_or_default();
    save(&Tokens {
        provider: provider_id.to_string(),
        access_token: access,
        refresh_token: refresh,
        expires_at: if expires_in == 0 { 0 } else { now + expires_in },
        account,
    });
    Ok(format!(
        "Signed in with {}",
        preset.map(|p| p.title).unwrap_or("custom OAuth")
    ))
}

pub fn apply_provider_defaults(settings: &mut Settings, provider_id: &str) {
    if let Some(preset) = PROVIDERS.iter().find(|p| p.id == provider_id) {
        settings.ai.provider = preset.id.to_string();
        if settings.ai.endpoint.is_empty()
            || settings.ai.endpoint.contains("127.0.0.1")
            || settings.ai.endpoint.contains("localhost")
        {
            settings.ai.endpoint = preset.chat_endpoint.to_string();
        }
        if settings.ai.model.is_empty()
            || settings.ai.model.contains("qwen")
            || settings.ai.model.contains("gemma")
            || settings.ai.model.contains("llama")
        {
            settings.ai.model = preset.default_model.to_string();
        }
        settings.save();
    }
}

fn wait_for_code(listener: &TcpListener) -> Result<(String, String), String> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(pair) => break pair,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(
                        "Sign-in timed out. Nothing reached Flint’s local callback.".into(),
                    );
                }
                thread::sleep(Duration::from_millis(120));
            }
            Err(_) => {
                return Err("Sign-in timed out. Nothing reached Flint’s local callback.".into());
            }
        }
    };
    stream.set_nonblocking(false).ok();
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let line = req.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if method != "GET" {
        return Err("Unexpected OAuth callback method".into());
    }
    let path_only = path.split('?').next().unwrap_or("");
    if path_only != "/callback" {
        return Err("Unexpected OAuth callback path".into());
    }
    let host_ok = req.lines().any(|l| {
        let lower = l.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("host:") {
            let host = rest.trim();
            host == "127.0.0.1"
                || host == "localhost"
                || host.starts_with("127.0.0.1:")
                || host.starts_with("localhost:")
        } else {
            false
        }
    });
    if !host_ok {
        return Err("OAuth callback Host was not local".into());
    }
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code = String::new();
    let mut state = String::new();
    let mut error = String::new();
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = urldecode(v);
        match k {
            "code" => code = v,
            "state" => state = v,
            "error" | "error_description" => error = v,
            _ => {}
        }
    }
    let body = if code.is_empty() {
        format!(
            "<html><body style='background:#121214;color:#f6f1ea;font-family:Inter,sans-serif;padding:48px'>
             <h1>Flint</h1><p>Sign-in did not finish. {}</p></body></html>",
            html_escape(&error)
        )
    } else {
        "<html><body style='background:#121214;color:#f6f1ea;font-family:Inter,sans-serif;padding:48px'>
         <h1 style='color:#ff5a1f'>Flint</h1><p>You’re signed in. You can return to the launcher.</p></body></html>".into()
    };
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    if !error.is_empty() && code.is_empty() {
        return Err(format!("Provider returned {error}"));
    }
    if code.is_empty() {
        return Err("No authorization code in the callback".into());
    }
    Ok((code, state))
}

fn token_request(url: &str, body: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args([
            "-sS",
            "--fail",
            "--max-time",
            "30",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/x-www-form-urlencoded",
            "--data-binary",
            "@-",
            url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(body.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(|e| format!("Token exchange failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Token exchange failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

fn open_browser(url: &str) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open browser: {e}"))
}

fn pkce_challenge(verifier: &str) -> Result<String, String> {
    let output = Command::new("openssl")
        .args(["dgst", "-sha256", "-binary"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(verifier.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(|e| format!("openssl is required for PKCE: {e}"))?;
    if !output.status.success() {
        return Err("openssl sha256 failed".into());
    }
    Ok(b64url(&output.stdout))
}

fn random_token(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    let mut f = fs::File::open("/dev/urandom").map_err(|_| "No /dev/urandom".to_string())?;
    f.read_exact(&mut buf)
        .map_err(|_| "Could not read /dev/urandom".to_string())?;
    Ok(b64url(&buf))
}

fn b64url(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };
        out.push(T[(b0 >> 2) as usize] as char);
        out.push(T[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < data.len() {
            out.push(T[(((b1 & 15) << 2) | (b2 >> 6)) as usize] as char);
        }
        if i + 2 < data.len() {
            out.push(T[(b2 & 63) as usize] as char);
        }
        i += 3;
    }
    out
}

fn urlencode(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn urldecode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = |c: u8| match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => 0,
            };
            out.push((h(bytes[i + 1]) << 4) | h(bytes[i + 2]));
            i += 3;
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn json_str(raw: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let idx = raw.find(&needle)?;
    let rest = &raw[idx + needle.len()..];
    let rest = rest.trim_start().trim_start_matches(':').trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let bytes: Vec<char> = rest.chars().skip(1).collect();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            '"' => break,
            '\\' if i + 1 < bytes.len() => {
                out.push(bytes[i + 1]);
                i += 2;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn json_u64(raw: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let idx = raw.find(&needle)?;
    let rest = &raw[idx + needle.len()..];
    let rest = rest.trim_start().trim_start_matches(':').trim_start();
    rest.split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse().ok())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::{b64url, json_str};

    #[test]
    fn b64url_is_url_safe() {
        let s = b64url(&[0xff, 0xee, 0xdd, 0xcc]);
        assert!(!s.contains('+'));
        assert!(!s.contains('/'));
    }

    #[test]
    fn pulls_access_token() {
        let raw = r#"{"token_type":"bearer","access_token":"abc-123"}"#;
        assert_eq!(json_str(raw, "access_token").as_deref(), Some("abc-123"));
    }
}
