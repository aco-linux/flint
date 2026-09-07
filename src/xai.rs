//! xAI Grok authentication.
//!
//! A SuperGrok / X Premium subscription is not an API key. xAI's public Grok
//! CLI client supports device-code OAuth at auth.x.ai. Flint reuses that
//! public client id (not a secret) and can also import `~/.grok/auth.json`.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::auth::{self, Tokens};
use crate::config::Settings;

pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const DEVICE_URL: &str = "https://auth.x.ai/oauth2/device/code";
pub const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
pub const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
pub const API_ORIGIN: &str = "https://api.x.ai";
pub const CHAT_ENDPOINT: &str = "https://api.x.ai/v1";
pub const DEFAULT_MODEL: &str = "grok-4";

#[derive(Debug, Clone)]
pub struct DevicePending {
    pub verification_url: String,
    pub user_code: String,
    device_code: String,
    interval: Duration,
    deadline: InstantLike,
}

#[derive(Debug, Clone, Copy)]
struct InstantLike {
    unix: u64,
}

impl InstantLike {
    fn now() -> Self {
        Self { unix: unix_now() }
    }

    fn after(secs: u64) -> Self {
        Self {
            unix: unix_now().saturating_add(secs),
        }
    }

    fn expired(self) -> bool {
        unix_now() >= self.unix
    }
}

#[derive(Debug, Deserialize)]
struct DeviceResponse {
    device_code: String,
    user_code: String,
    #[serde(default)]
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    interval: u64,
}

pub fn apply_defaults(settings: &mut Settings) {
    settings.ai.provider = "xai".into();
    settings.ai.endpoint = CHAT_ENDPOINT.into();
    if settings.ai.model.is_empty()
        || settings.ai.model.contains("qwen")
        || settings.ai.model.contains("gemini")
        || settings.ai.model.contains("gpt")
    {
        settings.ai.model = DEFAULT_MODEL.into();
    }
    settings.save();
}

pub fn start_device() -> Result<DevicePending, String> {
    let body = form_body(&[("client_id", CLIENT_ID), ("scope", SCOPE)]);
    let url = Url::parse(DEVICE_URL).map_err(|e| e.to_string())?;
    let raw = https_form(&url, &body)?;
    let parsed: DeviceResponse =
        serde_json::from_slice(&raw).map_err(|e| format!("xAI device response invalid: {e}"))?;
    if parsed.device_code.is_empty() || parsed.user_code.is_empty() {
        return Err("xAI did not return a device code".into());
    }
    let url = if !parsed.verification_uri_complete.is_empty() {
        parsed.verification_uri_complete
    } else if !parsed.verification_uri.is_empty() {
        parsed.verification_uri
    } else {
        "https://auth.x.ai/device".into()
    };
    let url = pin_xai_url(&url)?;
    let interval = parsed.interval.max(5);
    let expires = parsed.expires_in.max(60).min(15 * 60);
    Ok(DevicePending {
        verification_url: url,
        user_code: parsed.user_code,
        device_code: parsed.device_code,
        interval: Duration::from_secs(interval),
        deadline: InstantLike::after(expires),
    })
}

pub fn finish_device(pending: DevicePending) -> Result<String, String> {
    let token_url = Url::parse(TOKEN_URL).map_err(|e| e.to_string())?;
    let mut wait = pending.interval;
    while !pending.deadline.expired() {
        thread::sleep(wait);
        let body = form_body(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", &pending.device_code),
            ("client_id", CLIENT_ID),
        ]);
        match https_form(&token_url, &body) {
            Ok(raw) => {
                let value: Value = serde_json::from_slice(&raw)
                    .map_err(|e| format!("xAI token response invalid: {e}"))?;
                if let Some(err) = value.get("error").and_then(Value::as_str) {
                    match err {
                        "authorization_pending" => continue,
                        "slow_down" => {
                            wait += Duration::from_secs(5);
                            continue;
                        }
                        "access_denied" => return Err("xAI sign-in was denied".into()),
                        "expired_token" => {
                            return Err("xAI device code expired. Try Connect Grok again.".into());
                        }
                        other => {
                            let detail = value
                                .get("error_description")
                                .and_then(Value::as_str)
                                .unwrap_or(other);
                            return Err(format!("xAI sign-in failed: {detail}"));
                        }
                    }
                }
                let access = value
                    .get("access_token")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if access.is_empty() {
                    return Err("xAI returned an empty access token".into());
                }
                let refresh = value
                    .get("refresh_token")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let expires_in = value.get("expires_in").and_then(Value::as_u64).unwrap_or(0);
                let account = value
                    .get("email")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let storage = auth::save(&Tokens {
                    provider: "xai".into(),
                    access_token: access,
                    refresh_token: refresh,
                    expires_at: unix_now().saturating_add(expires_in.max(300)),
                    account,
                    token_url: TOKEN_URL.into(),
                    client_id: CLIENT_ID.into(),
                    api_origin: API_ORIGIN.into(),
                })?;
                return Ok(format!("Signed in to Grok (xAI) · credential in {storage}"));
            }
            Err(err) if err.contains("authorization_pending") => continue,
            Err(err) => return Err(err),
        }
    }
    Err("xAI sign-in timed out. Try Connect Grok again.".into())
}

pub fn import_grok_cli() -> Result<String, String> {
    import_grok_cli_from(&grok_auth_path())
}

pub fn import_grok_cli_from(path: &std::path::Path) -> Result<String, String> {
    let raw =
        fs::read_to_string(path).map_err(|_| format!("No Grok CLI login at {}", path.display()))?;
    let value: Value = serde_json::from_str::<Value>(&raw)
        .map_err(|_| "Grok CLI auth.json is not valid JSON".to_string())?;
    let obj = value
        .as_object()
        .ok_or("Grok CLI auth.json is not an object")?;
    let entry = obj
        .iter()
        .find(|(key, _)| key.contains("auth.x.ai"))
        .map(|(_, v)| v)
        .or_else(|| obj.values().next())
        .ok_or("Grok CLI auth.json has no xAI entry")?;
    let access = entry
        .get("key")
        .and_then(Value::as_str)
        .ok_or("Grok CLI login has no access token")?;
    if access.is_empty() {
        return Err("Grok CLI login is empty".into());
    }
    let refresh = entry
        .get("refresh_token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let account = entry
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let expires_at = entry
        .get("expires_at")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339)
        .unwrap_or_else(|| unix_now() + 3600);
    let client_id = entry
        .get("oidc_client_id")
        .and_then(Value::as_str)
        .unwrap_or(CLIENT_ID)
        .to_string();
    let storage = auth::save(&Tokens {
        provider: "xai".into(),
        access_token: access.to_string(),
        refresh_token: refresh,
        expires_at,
        account: account.clone(),
        token_url: TOKEN_URL.into(),
        client_id,
        api_origin: API_ORIGIN.into(),
    })?;
    Ok(if account.is_empty() {
        format!("Imported Grok CLI login · {storage}")
    } else {
        format!("Imported Grok CLI login as {account} · {storage}")
    })
}

pub fn grok_auth_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".grok/auth.json")
}

fn pin_xai_url(url: &str) -> Result<String, String> {
    let parsed = Url::parse(url).map_err(|_| "xAI verification URL is invalid".to_string())?;
    if parsed.scheme() != "https" {
        return Err("xAI returned a non-HTTPS verification URL".into());
    }
    match parsed.host_str() {
        Some("auth.x.ai") | Some("accounts.x.ai") => Ok(parsed.to_string()),
        _ => Err("xAI returned a verification URL on an unexpected host".into()),
    }
}

fn form_body(pairs: &[(&str, &str)]) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        ser.append_pair(k, v);
    }
    ser.finish()
}

fn https_form(url: &Url, body: &str) -> Result<Vec<u8>, String> {
    if url.scheme() != "https" {
        return Err("xAI OAuth URLs must be HTTPS".into());
    }
    let output = Command::new("curl")
        .args([
            "-sS",
            "--fail-with-body",
            "--connect-timeout",
            "10",
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
            "-H",
            "Accept: application/json",
            "--data-binary",
            "@-",
            url.as_str(),
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
        .map_err(|e| format!("xAI request failed: {e}"))?;
    if output.stdout.len() > 1024 * 1024 {
        return Err("xAI response was too large".into());
    }
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        let combined = if detail.trim().is_empty() {
            err.trim().to_string()
        } else {
            detail.trim().to_string()
        };
        return Err(format!("xAI request failed: {combined}"));
    }
    Ok(output.stdout)
}

fn parse_rfc3339(raw: &str) -> Option<u64> {
    let t = raw.trim();
    let date = t.get(..10)?;
    let time = t.get(11..19)?;
    let mut parts = date.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    let mut tparts = time.split(':');
    let hour: u32 = tparts.next()?.parse().ok()?;
    let min: u32 = tparts.next()?.parse().ok()?;
    let sec: u32 = tparts.next()?.parse().ok()?;
    if year < 1970 || year > 9999 {
        return None;
    }
    if !(1..=12).contains(&month) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    const MD: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut dim = MD[(month - 1) as usize];
    if month == 2 && is_leap(year) {
        dim = 29;
    }
    if day < 1 || day > dim {
        return None;
    }
    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    for m in 1..month {
        days += i64::from(MD[(m - 1) as usize]);
        if m == 2 && is_leap(year) {
            days += 1;
        }
    }
    days += i64::from(day.saturating_sub(1));
    let secs = days * 86400 + i64::from(hour) * 3600 + i64::from(min) * 60 + i64::from(sec);
    u64::try_from(secs).ok()
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{DeviceResponse, parse_rfc3339};

    #[test]
    fn pin_xai_url_allows_known_hosts() {
        assert!(super::pin_xai_url("https://accounts.x.ai/oauth2/device?user_code=AB").is_ok());
        assert!(super::pin_xai_url("https://auth.x.ai/device").is_ok());
        assert!(super::pin_xai_url("https://evil.example/device").is_err());
        assert!(super::pin_xai_url("https://auth.x.ai.attacker/device").is_err());
    }

    #[test]
    fn parses_device_json() {
        let raw = r#"{
            "device_code": "dev-1",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://auth.x.ai/device",
            "verification_uri_complete": "https://auth.x.ai/device?user_code=ABCD-EFGH",
            "expires_in": 600,
            "interval": 5
        }"#;
        let parsed: DeviceResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.user_code, "ABCD-EFGH");
        assert!(parsed.verification_uri_complete.starts_with("https://"));
    }

    #[test]
    fn parses_grok_cli_expiry() {
        let ts = parse_rfc3339("2026-09-07T05:53:04.570283924Z").expect("ts");
        assert!(ts > 1_700_000_000);
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert!(parse_rfc3339("2024-02-29T00:00:00Z").is_some());
    }

    #[test]
    fn rejects_invalid_grok_cli_expiry() {
        assert!(parse_rfc3339("2026-13-01T00:00:00Z").is_none());
        assert!(parse_rfc3339("2026-00-01T00:00:00Z").is_none());
        assert!(parse_rfc3339("2026-04-31T00:00:00Z").is_none());
        assert!(parse_rfc3339("2025-02-29T00:00:00Z").is_none());
        assert!(parse_rfc3339("2026-09-07T24:00:00Z").is_none());
        assert!(parse_rfc3339("2026-09-07T00:60:00Z").is_none());
        assert!(parse_rfc3339("1969-12-31T23:59:59Z").is_none());
    }

    #[test]
    fn import_requires_a_file() {
        let err = super::import_grok_cli_from(std::path::Path::new("/tmp/flint-no-such-grok.json"))
            .unwrap_err();
        assert!(
            err.contains("Grok CLI") || err.contains("auth.json"),
            "{err}"
        );
    }

    #[test]
    #[ignore = "hits auth.x.ai"]
    fn live_device_start_returns_https_url() {
        let pending = super::start_device().expect("xAI device endpoint must respond");
        assert!(
            pending.verification_url.starts_with("https://"),
            "{}",
            pending.verification_url
        );
        assert!(!pending.user_code.is_empty());
        eprintln!(
            "xAI device URL {} code {}",
            pending.verification_url, pending.user_code
        );
    }

    #[test]
    fn import_parses_cli_fixture_without_saving_if_missing_key() {
        let raw = r#"{
          "https://auth.x.ai::abc": {
            "key": "",
            "refresh_token": "r",
            "email": "a@b.c",
            "expires_at": "2026-09-07T05:53:04Z",
            "oidc_client_id": "abc"
          }
        }"#;
        let dir = std::env::temp_dir().join("flint-grok-fixture.json");
        std::fs::write(&dir, raw).unwrap();
        let err = super::import_grok_cli_from(&dir).unwrap_err();
        let _ = std::fs::remove_file(&dir);
        assert!(err.contains("empty"), "{err}");
    }
}
