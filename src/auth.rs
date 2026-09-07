use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::{Url, form_urlencoded};

use crate::config::Settings;

const AUTH_VERSION: u8 = 2;
const KEYRING_APP: &str = "dev.flint.launcher";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_CALLBACK_BYTES: usize = 8 * 1024;
const MAX_TOKEN_RESPONSE_BYTES: usize = 1024 * 1024;

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
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub api_origin: String,
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

/// Providers with a documented OAuth flow that can authorize API calls.
/// Consumer ChatGPT and Claude subscriptions do not grant API access, so they
/// intentionally are not represented as OAuth presets here.
pub const PROVIDERS: &[Provider] = &[Provider {
    id: "google",
    title: "Google Gemini API",
    authorize: "https://accounts.google.com/o/oauth2/v2/auth",
    token: "https://oauth2.googleapis.com/token",
    scopes: "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/generative-language.retriever",
    chat_endpoint: "https://generativelanguage.googleapis.com/v1beta/openai",
    default_model: "gemini-3.7-flash",
}];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct PersistedTokens {
    version: u8,
    provider: String,
    account: String,
    expires_at: u64,
    token_url: String,
    client_id: String,
    api_origin: String,
    storage: String,
    // Present only when the desktop Secret Service is unavailable.
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenSecret {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    token_type: String,
}

pub fn path() -> PathBuf {
    crate::paths::config_dir().join("auth.json")
}

fn api_keys_path() -> PathBuf {
    crate::paths::config_dir().join("api-keys.json")
}

pub fn load() -> Option<Tokens> {
    let stored = load_persisted()?;
    materialize(stored)
}

pub fn load_for(provider: &str) -> Option<Tokens> {
    if is_connector_id(provider) {
        return load_connector(provider);
    }
    let tokens = load()?;
    (tokens.provider == provider).then_some(tokens)
}

fn is_connector_id(provider: &str) -> bool {
    crate::connectors::preset(provider).is_some()
        || matches!(provider, "apple-calendar" | "proton-calendar" | "caldav")
}

fn load_persisted() -> Option<PersistedTokens> {
    let raw = fs::read_to_string(path()).ok()?;
    let mut stored: PersistedTokens = serde_json::from_str(&raw).ok()?;
    // Version 1 stored both tokens directly in auth.json.
    if stored.version == 0 {
        stored.version = 1;
        if stored.storage.is_empty() {
            stored.storage = "private-file".into();
        }
    }
    Some(stored)
}

fn materialize(stored: PersistedTokens) -> Option<Tokens> {
    let secret = if stored.storage == "secret-service" {
        keyring_lookup("oauth", &stored.provider)
            .ok()
            .and_then(|raw| serde_json::from_str::<TokenSecret>(&raw).ok())?
    } else {
        TokenSecret {
            access_token: stored.access_token.clone(),
            refresh_token: stored.refresh_token.clone(),
        }
    };
    if secret.access_token.is_empty() {
        return None;
    }
    Some(Tokens {
        provider: stored.provider,
        access_token: secret.access_token,
        refresh_token: secret.refresh_token,
        expires_at: stored.expires_at,
        account: stored.account,
        token_url: stored.token_url,
        client_id: stored.client_id,
        api_origin: stored.api_origin,
    })
}

pub fn save(tokens: &Tokens) -> Result<&'static str, String> {
    if is_connector_id(&tokens.provider) {
        return save_connector(tokens);
    }
    save_ai(tokens)
}

fn save_ai(tokens: &Tokens) -> Result<&'static str, String> {
    crate::paths::ensure();
    if let Some(previous) = load_persisted()
        && previous.provider != tokens.provider
        && previous.storage == "secret-service"
    {
        let _ = keyring_clear("oauth", &previous.provider);
    }
    let secret = serde_json::to_string(&TokenSecret {
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
    })
    .map_err(|error| error.to_string())?;
    let keyring_saved = keyring_store("oauth", &tokens.provider, &secret).is_ok();
    let stored = PersistedTokens {
        version: AUTH_VERSION,
        provider: tokens.provider.clone(),
        account: tokens.account.clone(),
        expires_at: tokens.expires_at,
        token_url: tokens.token_url.clone(),
        client_id: tokens.client_id.clone(),
        api_origin: tokens.api_origin.clone(),
        storage: if keyring_saved {
            "secret-service".into()
        } else {
            "private-file".into()
        },
        access_token: if keyring_saved {
            String::new()
        } else {
            tokens.access_token.clone()
        },
        refresh_token: if keyring_saved {
            String::new()
        } else {
            tokens.refresh_token.clone()
        },
    };
    let raw = serde_json::to_string_pretty(&stored).map_err(|error| error.to_string())?;
    crate::paths::write_private(&path(), raw).map_err(|error| error.to_string())?;
    Ok(if keyring_saved {
        "desktop keyring"
    } else {
        "private file (mode 600)"
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct ConnectorStore {
    accounts: BTreeMap<String, PersistedTokens>,
}

fn connectors_path() -> PathBuf {
    crate::paths::config_dir().join("connectors-auth.json")
}

fn load_connector_store() -> ConnectorStore {
    fs::read_to_string(connectors_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn upsert_connector_account(mut store: ConnectorStore, stored: PersistedTokens) -> ConnectorStore {
    store.accounts.insert(stored.provider.clone(), stored);
    store
}

fn save_connector(tokens: &Tokens) -> Result<&'static str, String> {
    crate::paths::ensure();
    let secret = serde_json::to_string(&TokenSecret {
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
    })
    .map_err(|error| error.to_string())?;
    let keyring_saved = keyring_store("oauth", &tokens.provider, &secret).is_ok();
    let stored = PersistedTokens {
        version: AUTH_VERSION,
        provider: tokens.provider.clone(),
        account: tokens.account.clone(),
        expires_at: tokens.expires_at,
        token_url: tokens.token_url.clone(),
        client_id: tokens.client_id.clone(),
        api_origin: tokens.api_origin.clone(),
        storage: if keyring_saved {
            "secret-service".into()
        } else {
            "private-file".into()
        },
        access_token: if keyring_saved {
            String::new()
        } else {
            tokens.access_token.clone()
        },
        refresh_token: if keyring_saved {
            String::new()
        } else {
            tokens.refresh_token.clone()
        },
    };
    let store = upsert_connector_account(load_connector_store(), stored);
    let raw = serde_json::to_string_pretty(&store).map_err(|error| error.to_string())?;
    crate::paths::write_private(&connectors_path(), raw).map_err(|error| error.to_string())?;
    Ok(if keyring_saved {
        "desktop keyring"
    } else {
        "private file (mode 600)"
    })
}

fn load_connector(provider: &str) -> Option<Tokens> {
    let stored = load_connector_store().accounts.remove(provider)?;
    materialize(stored)
}

pub fn clear() -> Result<(), String> {
    let mut first_error = None;
    if let Some(stored) = load_persisted().filter(|stored| stored.storage == "secret-service")
        && let Err(err) = keyring_clear("oauth", &stored.provider)
    {
        first_error = Some(err);
    }
    let _ = fs::remove_file(path());
    for (id, stored) in load_connector_store().accounts {
        if stored.storage == "secret-service"
            && let Err(err) = keyring_clear("oauth", &id)
        {
            first_error = first_error.or(Some(err));
        }
    }
    let _ = fs::remove_file(connectors_path());
    let _ = clear_api_key("apple-calendar");
    let _ = clear_api_key("proton-calendar");
    first_error.map_or(Ok(()), Err)
}

pub fn access_token(provider: &str) -> Result<String, String> {
    let Some(mut tokens) = load_for(provider) else {
        return Err(format!(
            "Not connected to {provider}. Run Connect {provider} in Settings."
        ));
    };
    if tokens.access_token.is_empty() {
        return Err("Saved connector token is empty. Connect again.".into());
    }
    if !is_expired(&tokens) {
        return Ok(tokens.access_token);
    }
    if tokens.refresh_token.is_empty() {
        return Err("OAuth access expired. Connect again.".into());
    }
    refresh(&mut tokens)?;
    Ok(tokens.access_token)
}

pub fn bearer(settings: &Settings) -> Result<Option<String>, String> {
    let provider = settings.ai.provider.as_str();
    let Some(mut tokens) = load_for(provider) else {
        return Ok(None);
    };
    let configured_origin = api_origin(&settings.ai.endpoint)?;
    if tokens.api_origin.is_empty() || tokens.api_origin != configured_origin {
        return Err(
            "The saved OAuth token is bound to a different API origin. Sign in again for this endpoint."
                .into(),
        );
    }
    if !is_expired(&tokens) {
        return Ok(Some(tokens.access_token));
    }
    if tokens.refresh_token.is_empty() {
        return Err("OAuth access expired. Sign in again in Settings.".into());
    }
    refresh(&mut tokens)?;
    Ok(Some(tokens.access_token))
}

pub fn signed_in_label() -> String {
    match load() {
        Some(tokens) if !tokens.access_token.is_empty() => {
            if tokens.account.is_empty() {
                format!("Signed in · {}", tokens.provider)
            } else {
                format!("Signed in as {} · {}", tokens.account, tokens.provider)
            }
        }
        _ => "Not signed in".into(),
    }
}

pub fn api_key(provider: &str) -> Option<String> {
    if let Ok(secret) = keyring_lookup("api-key", provider)
        && !secret.is_empty()
    {
        return Some(secret);
    }
    let raw = fs::read_to_string(api_keys_path()).ok()?;
    let keys: BTreeMap<String, String> = serde_json::from_str(&raw).ok()?;
    keys.get(provider).filter(|key| !key.is_empty()).cloned()
}

pub fn has_api_key(provider: &str) -> bool {
    api_key(provider).is_some()
}

pub fn save_api_key(provider: &str, value: &str) -> Result<&'static str, String> {
    let provider = validate_provider_id(provider)?;
    let value = value.trim();
    if value.is_empty() {
        return Err("API key cannot be empty".into());
    }
    if value.len() > 16 * 1024 || value.chars().any(char::is_control) {
        return Err("API key is not a valid credential".into());
    }
    if keyring_store("api-key", provider, value).is_ok() {
        remove_fallback_api_key(provider)?;
        return Ok("desktop keyring");
    }
    let mut keys = load_fallback_api_keys();
    keys.insert(provider.to_string(), value.to_string());
    write_fallback_api_keys(&keys)?;
    Ok("private file (mode 600)")
}

pub fn clear_api_key(provider: &str) -> Result<(), String> {
    let provider = validate_provider_id(provider)?;
    let _ = keyring_clear("api-key", provider);
    remove_fallback_api_key(provider)
}

fn load_fallback_api_keys() -> BTreeMap<String, String> {
    fs::read_to_string(api_keys_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_fallback_api_keys(keys: &BTreeMap<String, String>) -> Result<(), String> {
    if keys.is_empty() {
        let _ = fs::remove_file(api_keys_path());
        return Ok(());
    }
    let raw = serde_json::to_string_pretty(keys).map_err(|error| error.to_string())?;
    crate::paths::write_private(&api_keys_path(), raw).map_err(|error| error.to_string())
}

fn remove_fallback_api_key(provider: &str) -> Result<(), String> {
    let mut keys = load_fallback_api_keys();
    keys.remove(provider);
    write_fallback_api_keys(&keys)
}

pub struct BrowserJob {
    pub url: String,
    pub message: String,
    pub user_code: Option<String>,
}

pub enum PendingLogin {
    Loopback(LoopbackPending),
    XaiDevice(crate::xai::DevicePending),
    Notice(String),
}

pub struct LoopbackPending {
    listener: TcpListener,
    port: u16,
    state: String,
    verifier: String,
    token_url: Url,
    client_id: String,
    provider_id: String,
    api_origin: String,
    redirect: String,
}

/// Start OAuth without opening a browser. The UI must open `BrowserJob.url`
/// on the GTK main thread, then call [`finish_login`] on a worker.
pub fn start_login(
    provider_id: &str,
    settings: &Settings,
) -> Result<(BrowserJob, PendingLogin), String> {
    if provider_id == "xai" || provider_id == "grok" {
        let pending = crate::xai::start_device()?;
        let job = BrowserJob {
            url: pending.verification_url.clone(),
            message: format!(
                "Opening xAI in your browser. Confirm the code {} if asked.",
                pending.user_code
            ),
            user_code: Some(pending.user_code.clone()),
        };
        return Ok((job, PendingLogin::XaiDevice(pending)));
    }
    if provider_id == "cursor" {
        return Err(
            "Cursor does not offer third-party OAuth. Flint cannot sign into Cursor. Use Connect Grok with your xAI subscription, or a custom provider you registered."
                .into(),
        );
    }
    if let Some(notice) = crate::connectors::browser_notice(provider_id) {
        let job = BrowserJob {
            url: notice.url.clone(),
            message: notice.message.clone(),
            user_code: None,
        };
        return Ok((job, PendingLogin::Notice(notice.message)));
    }
    if let Some(connector) = crate::connectors::preset(provider_id) {
        if matches!(provider_id, "notion" | "todoist")
            && api_key(&format!("{provider_id}-secret")).is_none()
        {
            return Err(format!(
                "Set the {provider_id} OAuth client secret in Settings first. This provider does not support a public PKCE client."
            ));
        }
        let client_id = crate::connectors::client_id_for(provider_id, settings);
        return start_loopback(
            provider_id,
            connector.authorize,
            connector.token,
            connector.scopes,
            &client_id,
            connector.api_origin,
            provider_id == "google-calendar",
        );
    }
    let preset = PROVIDERS.iter().find(|provider| provider.id == provider_id);
    if provider_id != "custom" && preset.is_none() {
        return Err(format!(
            "{provider_id} does not expose a supported API OAuth flow. Use Connect Grok, an API key, or a custom provider."
        ));
    }
    let authorize = if provider_id == "custom" {
        settings.ai.oauth_authorize_url.trim()
    } else {
        preset
            .map(|provider| provider.authorize)
            .unwrap_or_default()
    };
    let token_url = if provider_id == "custom" {
        settings.ai.oauth_token_url.trim()
    } else {
        preset.map(|provider| provider.token).unwrap_or_default()
    };
    let scopes = if provider_id == "custom" {
        settings.ai.oauth_scopes.trim()
    } else {
        preset.map(|provider| provider.scopes).unwrap_or_default()
    };
    let client_id = settings.ai.client_id.trim();
    if client_id.is_empty() {
        return Err("Add a desktop OAuth client ID in Settings first — Flint cannot open a provider that has not issued you a client.".into());
    }
    if client_id.len() > 2048 || client_id.chars().any(char::is_control) {
        return Err("OAuth client ID is invalid".into());
    }
    if authorize.is_empty() {
        return Err("Set the OAuth authorize URL in Settings first".into());
    }
    if token_url.is_empty() {
        return Err("Set the OAuth token URL in Settings first".into());
    }
    start_loopback(
        provider_id,
        authorize,
        token_url,
        scopes,
        client_id,
        &api_origin(&settings.ai.endpoint)?,
        provider_id == "google",
    )
}

fn start_loopback(
    provider_id: &str,
    authorize: &str,
    token_url: &str,
    scopes: &str,
    client_id: &str,
    api_origin: &str,
    google_offline: bool,
) -> Result<(BrowserJob, PendingLogin), String> {
    if client_id.is_empty() {
        return Err("Add a desktop OAuth client ID in Settings first — Flint cannot open a provider that has not issued you a client.".into());
    }
    let mut authorize_url = secure_url(authorize, "OAuth authorize URL")?;
    let token_url = secure_url(token_url, "OAuth token URL")?;
    reject_reserved_authorize_parameters(&authorize_url)?;
    let api_origin = api_origin.to_string();

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("Could not secure OAuth callback listener: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let verifier = random_token(64)?;
    let challenge = pkce_challenge(&verifier);
    let state = random_token(32)?;

    {
        let mut query = authorize_url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", &redirect)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        if !scopes.is_empty() {
            query.append_pair("scope", scopes);
        }
        if google_offline {
            query
                .append_pair("access_type", "offline")
                .append_pair("prompt", "consent");
        }
        if provider_id == "notion" {
            query.append_pair("owner", "user");
        }
    }

    let job = BrowserJob {
        url: authorize_url.to_string(),
        message: format!("Opening {provider_id} sign-in in your browser…"),
        user_code: None,
    };
    Ok((
        job,
        PendingLogin::Loopback(LoopbackPending {
            listener,
            port,
            state,
            verifier,
            token_url,
            client_id: client_id.to_string(),
            provider_id: provider_id.to_string(),
            api_origin,
            redirect,
        }),
    ))
}

pub fn finish_login(pending: PendingLogin) -> Result<String, String> {
    match pending {
        PendingLogin::XaiDevice(device) => crate::xai::finish_device(device),
        PendingLogin::Loopback(loopback) => finish_loopback(loopback),
        PendingLogin::Notice(message) => Ok(message),
    }
}

fn finish_loopback(pending: LoopbackPending) -> Result<String, String> {
    let code = wait_for_code(&pending.listener, pending.port, &pending.state)?;
    let response = match pending.provider_id.as_str() {
        "notion" => notion_token(&pending, &code)?,
        "todoist" => todoist_token(&pending, &code)?,
        _ => {
            let body = form_urlencoded::Serializer::new(String::new())
                .append_pair("grant_type", "authorization_code")
                .append_pair("code", &code)
                .append_pair("redirect_uri", &pending.redirect)
                .append_pair("client_id", &pending.client_id)
                .append_pair("code_verifier", &pending.verifier)
                .finish();
            token_request(&pending.token_url, &body)?
        }
    };
    validate_token_response(&response)?;
    let now = unix_now();
    let storage = save(&Tokens {
        provider: pending.provider_id.clone(),
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        expires_at: expiry(now, response.expires_in),
        account: String::new(),
        token_url: pending.token_url.to_string(),
        client_id: pending.client_id,
        api_origin: pending.api_origin,
    })?;
    Ok(format!(
        "Signed in with {} · credentials stored in {storage}",
        PROVIDERS
            .iter()
            .find(|provider| provider.id == pending.provider_id)
            .map(|provider| provider.title)
            .unwrap_or(&pending.provider_id)
    ))
}

pub fn apply_provider_defaults(settings: &mut Settings, provider_id: &str) {
    if let Some(preset) = PROVIDERS.iter().find(|provider| provider.id == provider_id) {
        settings.ai.provider = preset.id.to_string();
        settings.ai.endpoint = preset.chat_endpoint.to_string();
        settings.ai.model = preset.default_model.to_string();
        settings.save();
    } else if provider_id == "custom" {
        settings.ai.provider = "custom".into();
        settings.save();
    } else if provider_id == "xai" || provider_id == "grok" {
        crate::xai::apply_defaults(settings);
    }
}

fn refresh(tokens: &mut Tokens) -> Result<(), String> {
    let token_url = secure_url(&tokens.token_url, "Saved OAuth token URL")?;
    if tokens.client_id.is_empty() {
        return Err("OAuth client information is missing. Sign in again.".into());
    }
    let body = form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("refresh_token", &tokens.refresh_token)
        .append_pair("client_id", &tokens.client_id)
        .finish();
    let response = token_request(&token_url, &body)?;
    validate_token_response(&response)?;
    tokens.access_token = response.access_token;
    if !response.refresh_token.is_empty() {
        tokens.refresh_token = response.refresh_token;
    }
    tokens.expires_at = expiry(unix_now(), response.expires_in);
    save(tokens)?;
    Ok(())
}

fn wait_for_code(
    listener: &TcpListener,
    port: u16,
    expected_state: &str,
) -> Result<String, String> {
    let deadline = Instant::now() + CALLBACK_TIMEOUT;
    loop {
        let (mut stream, peer) = match listener.accept() {
            Ok(pair) => pair,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("Sign-in timed out. Nothing reached Flint’s local callback.".into());
                }
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(error) => return Err(format!("OAuth callback failed: {error}")),
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        match read_callback(&mut stream, port, expected_state) {
            Ok(Callback::Success(code)) => {
                send_callback_page(&mut stream, true, "Sign-in complete");
                return Ok(code);
            }
            Ok(Callback::ProviderError(error)) => {
                send_callback_page(&mut stream, false, &error);
                return Err(format!("Provider denied sign-in: {error}"));
            }
            Err(error) => {
                send_callback_page(&mut stream, false, &error);
                // Ignore unrelated or forged loopback requests and keep waiting for
                // the browser callback until the original deadline.
            }
        }
    }
}

enum Callback {
    Success(String),
    ProviderError(String),
}

fn read_callback(
    stream: &mut TcpStream,
    port: u16,
    expected_state: &str,
) -> Result<Callback, String> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut raw = Vec::new();
    let mut chunk = [0u8; 1024];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
        if raw.len() > MAX_CALLBACK_BYTES {
            return Err("OAuth callback headers were too large".into());
        }
    }
    let request = std::str::from_utf8(&raw).map_err(|_| "OAuth callback was not UTF-8")?;
    parse_callback_request(request, port, expected_state)
}

fn parse_callback_request(
    request: &str,
    port: u16,
    expected_state: &str,
) -> Result<Callback, String> {
    let mut lines = request.split("\r\n");
    let mut request_line = lines.next().unwrap_or("").split_whitespace();
    if request_line.next() != Some("GET") {
        return Err("Unexpected OAuth callback method".into());
    }
    let target = request_line
        .next()
        .ok_or("OAuth callback target was missing")?;
    if request_line.next() != Some("HTTP/1.1") {
        return Err("OAuth callback must use HTTP/1.1".into());
    }
    let expected_host = format!("127.0.0.1:{port}");
    let host = lines
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
                .map(|(_, value)| value.trim())
        })
        .ok_or("OAuth callback Host was missing")?;
    if host != expected_host {
        return Err("OAuth callback Host did not match the listener".into());
    }
    let callback_url = Url::parse(&format!("http://{expected_host}{target}"))
        .map_err(|_| "OAuth callback URL was invalid")?;
    if callback_url.path() != "/callback" || callback_url.fragment().is_some() {
        return Err("Unexpected OAuth callback path".into());
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in callback_url.query_pairs() {
        match key.as_ref() {
            "code" if code.is_none() => code = Some(value.into_owned()),
            "state" if state.is_none() => state = Some(value.into_owned()),
            "error_description" => error = Some(value.into_owned()),
            "error" if error.is_none() => error = Some(value.into_owned()),
            _ => {}
        }
    }
    let returned_state = state.ok_or("OAuth callback state was missing")?;
    if !constant_time_eq(expected_state.as_bytes(), returned_state.as_bytes()) {
        return Err("OAuth state mismatch".into());
    }
    if let Some(error) = error {
        return Ok(Callback::ProviderError(error));
    }
    let code = code.ok_or("No authorization code in the callback")?;
    if code.is_empty() || code.len() > 16 * 1024 || code.chars().any(char::is_control) {
        return Err("OAuth authorization code was invalid".into());
    }
    Ok(Callback::Success(code))
}

fn send_callback_page(stream: &mut TcpStream, success: bool, message: &str) {
    let title = if success {
        "Signed in"
    } else {
        "Sign-in failed"
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'>\
         <title>Flint — {title}</title><style>body{{background:#121214;color:#f6f1ea;\
         font:16px system-ui;padding:48px;max-width:42rem}}h1{{color:#ff5a1f}}</style>\
         <h1>Flint</h1><p>{}</p><p>You can close this tab and return to Flint.</p>",
        html_escape(message)
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\n\
         Referrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn notion_token(pending: &LoopbackPending, code: &str) -> Result<TokenResponse, String> {
    let secret = api_key("notion-secret").ok_or_else(|| {
        "Notion client secret is missing. Set it in Settings and connect again.".to_string()
    })?;
    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": pending.redirect,
    })
    .to_string();
    token_request_with(
        &pending.token_url,
        &body,
        "application/json",
        Some((&pending.client_id, &secret)),
    )
}

fn todoist_token(pending: &LoopbackPending, code: &str) -> Result<TokenResponse, String> {
    let secret = api_key("todoist-secret").ok_or_else(|| {
        "Todoist client secret is missing. Set it in Settings and connect again.".to_string()
    })?;
    let body = form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &pending.client_id)
        .append_pair("client_secret", &secret)
        .append_pair("code", code)
        .append_pair("redirect_uri", &pending.redirect)
        .finish();
    token_request(&pending.token_url, &body)
}

fn token_request_with(
    url: &Url,
    body: &str,
    content_type: &str,
    basic: Option<(&str, &str)>,
) -> Result<TokenResponse, String> {
    let mut cmd = Command::new("curl");
    cmd.args([
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
        &format!("Content-Type: {content_type}"),
        "-H",
        "Accept: application/json",
        "--data-binary",
        "@-",
        url.as_str(),
    ]);
    let cfg_path = crate::paths::runtime_dir().join(format!(
        "oauth-curl-{}-{}.cfg",
        std::process::id(),
        unix_now()
    ));
    let _guard = if let Some((user, pass)) = basic {
        let escaped = format!("{user}:{pass}")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        crate::paths::write_private(&cfg_path, format!("user = \"{escaped}\"\n"))
            .map_err(|e| e.to_string())?;
        cmd.arg("-K").arg(&cfg_path);
        Some(RemoveFile(cfg_path))
    } else {
        None
    };
    let output = cmd
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
        .map_err(|error| format!("Token exchange failed: {error}"))?;
    parse_token_output(output)
}

struct RemoveFile(PathBuf);
impl Drop for RemoveFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn token_request(url: &Url, body: &str) -> Result<TokenResponse, String> {
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
        .map_err(|error| format!("Token exchange failed: {error}"))?;
    parse_token_output(output)
}

fn parse_token_output(output: std::process::Output) -> Result<TokenResponse, String> {
    if output.stdout.len() > MAX_TOKEN_RESPONSE_BYTES {
        return Err("Token endpoint response was too large".into());
    }
    if !output.status.success() {
        let detail = oauth_error_detail(&output.stdout)
            .unwrap_or_else(|| String::from_utf8_lossy(&output.stderr).trim().to_string());
        return Err(format!("Token exchange failed: {detail}"));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Invalid token response: {error}"))
}

fn oauth_error_detail(raw: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(raw).ok()?;
    value
        .get("error_description")
        .or_else(|| value.get("error"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn validate_token_response(response: &TokenResponse) -> Result<(), String> {
    if response.access_token.is_empty()
        || response.access_token.len() > 256 * 1024
        || response.access_token.chars().any(char::is_control)
    {
        return Err("Provider returned an invalid access token".into());
    }
    if response.refresh_token.len() > 256 * 1024
        || response.refresh_token.chars().any(char::is_control)
    {
        return Err("Provider returned an invalid refresh token".into());
    }
    if !response.token_type.is_empty() && !response.token_type.eq_ignore_ascii_case("bearer") {
        return Err(format!(
            "Unsupported OAuth token type: {}",
            response.token_type
        ));
    }
    Ok(())
}

/// Open an https (or loopback) URL from the GTK main thread.
pub fn open_browser(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1")) {
        return Err("Refusing to open a non-HTTPS sign-in URL".into());
    }
    crate::action::open_uri(url)
}

fn pkce_challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

fn random_token(bytes: usize) -> Result<String, String> {
    let mut buffer = vec![0u8; bytes];
    let mut random = fs::File::open("/dev/urandom").map_err(|_| "No /dev/urandom".to_string())?;
    random
        .read_exact(&mut buffer)
        .map_err(|_| "Could not read /dev/urandom".to_string())?;
    Ok(b64url(&buffer))
}

fn b64url(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::new();
    let mut index = 0;
    while index < data.len() {
        let first = data[index];
        let second = data.get(index + 1).copied().unwrap_or(0);
        let third = data.get(index + 2).copied().unwrap_or(0);
        output.push(TABLE[(first >> 2) as usize] as char);
        output.push(TABLE[(((first & 3) << 4) | (second >> 4)) as usize] as char);
        if index + 1 < data.len() {
            output.push(TABLE[(((second & 15) << 2) | (third >> 6)) as usize] as char);
        }
        if index + 2 < data.len() {
            output.push(TABLE[(third & 63) as usize] as char);
        }
        index += 3;
    }
    output
}

fn secure_url(input: &str, label: &str) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|_| format!("{label} is invalid"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(format!(
            "{label} must be an HTTPS URL without credentials or a fragment"
        ));
    }
    Ok(url)
}

fn api_origin(endpoint: &str) -> Result<String, String> {
    let url = secure_url(endpoint, "OAuth API endpoint")?;
    Ok(url.origin().ascii_serialization())
}

fn reject_reserved_authorize_parameters(url: &Url) -> Result<(), String> {
    const RESERVED: &[&str] = &[
        "response_type",
        "client_id",
        "redirect_uri",
        "state",
        "code_challenge",
        "code_challenge_method",
        "scope",
    ];
    if url
        .query_pairs()
        .any(|(key, _)| RESERVED.contains(&key.as_ref()))
    {
        return Err("OAuth authorize URL contains a reserved query parameter".into());
    }
    Ok(())
}

fn validate_provider_id(provider: &str) -> Result<&str, String> {
    if provider.is_empty()
        || provider.len() > 64
        || provider
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    {
        return Err("Provider ID is invalid".into());
    }
    Ok(provider)
}

fn keyring_store(kind: &str, provider: &str, secret: &str) -> Result<(), String> {
    let provider = validate_provider_id(provider)?;
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label=Flint AI credential",
            "application",
            KEYRING_APP,
            "kind",
            kind,
            "provider",
            provider,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    child
        .stdin
        .take()
        .ok_or("Could not open keyring input")?
        .write_all(secret.as_bytes())
        .map_err(|error| error.to_string())?;
    let status = child.wait().map_err(|error| error.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "Desktop keyring rejected the credential".into())
}

fn keyring_lookup(kind: &str, provider: &str) -> Result<String, String> {
    let provider = validate_provider_id(provider)?;
    let output = Command::new("secret-tool")
        .args([
            "lookup",
            "application",
            KEYRING_APP,
            "kind",
            kind,
            "provider",
            provider,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("Credential not found in desktop keyring".into());
    }
    let mut secret = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    if secret.ends_with('\n') {
        secret.pop();
        if secret.ends_with('\r') {
            secret.pop();
        }
    }
    Ok(secret)
}

fn keyring_clear(kind: &str, provider: &str) -> Result<(), String> {
    let provider = validate_provider_id(provider)?;
    let status = Command::new("secret-tool")
        .args([
            "clear",
            "application",
            KEYRING_APP,
            "kind",
            kind,
            "provider",
            provider,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "Credential was not present in desktop keyring".into())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && bool::from(left.ct_eq(right))
}

fn is_expired(tokens: &Tokens) -> bool {
    tokens.expires_at > 0 && unix_now().saturating_add(60) >= tokens.expires_at
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn expiry(now: u64, expires_in: u64) -> u64 {
    if expires_in == 0 {
        0
    } else {
        now.saturating_add(expires_in)
    }
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::{
        Callback, TokenResponse, api_origin, b64url, constant_time_eq, parse_callback_request,
        pkce_challenge, reject_reserved_authorize_parameters, secure_url, validate_token_response,
    };
    use url::Url;

    #[test]
    fn b64url_is_url_safe_and_unpadded() {
        let encoded = b64url(&[0xff, 0xee, 0xdd, 0xcc]);
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn pkce_matches_rfc_7636_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn secure_urls_reject_credential_and_fragment_confusion() {
        assert!(secure_url("http://example.com/oauth", "test").is_err());
        assert!(secure_url("https://user@example.com/oauth", "test").is_err());
        assert!(secure_url("https://example.com/oauth#fragment", "test").is_err());
        assert!(secure_url("https://example.com/oauth", "test").is_ok());
    }

    #[test]
    fn custom_oauth_without_urls_does_not_claim_to_open_a_browser() {
        let settings = crate::config::Settings::default();
        let err = match super::start_login("custom", &settings) {
            Err(err) => err,
            Ok(_) => panic!("expected an error"),
        };
        assert!(
            err.contains("client ID") || err.contains("authorize"),
            "{err}"
        );
        assert!(!err.to_ascii_lowercase().contains("opening"));
    }

    #[test]
    fn unknown_provider_is_rejected() {
        let settings = crate::config::Settings::default();
        let err = match super::start_login("cursor", &settings) {
            Err(err) => err,
            Ok(_) => panic!("expected an error"),
        };
        assert!(err.contains("Cursor"), "{err}");
        assert!(err.contains("Grok"), "{err}");
    }

    #[test]
    fn connector_accounts_do_not_share_one_slot() {
        let mut store = super::ConnectorStore::default();
        store = super::upsert_connector_account(
            store,
            super::PersistedTokens {
                version: 2,
                provider: "google-calendar".into(),
                account: "a@b.c".into(),
                ..super::PersistedTokens::default()
            },
        );
        store = super::upsert_connector_account(
            store,
            super::PersistedTokens {
                version: 2,
                provider: "notion".into(),
                account: "n@b.c".into(),
                ..super::PersistedTokens::default()
            },
        );
        assert_eq!(store.accounts.len(), 2);
        assert_eq!(
            store.accounts.get("google-calendar").unwrap().account,
            "a@b.c"
        );
        assert_eq!(store.accounts.get("notion").unwrap().account, "n@b.c");
    }

    #[test]
    fn open_browser_refuses_javascript_urls() {
        let err = super::open_browser("javascript:alert(1)").unwrap_err();
        assert!(err.to_ascii_lowercase().contains("https"), "{err}");
    }

    #[test]
    fn notion_without_secret_does_not_open_a_browser() {
        let mut settings = crate::config::Settings::default();
        settings.connectors.notion_client_id = "client".into();
        let err = match super::start_login("notion", &settings) {
            Err(err) => err,
            Ok(_) => panic!("expected an error"),
        };
        assert!(err.contains("secret"), "{err}");
    }

    #[test]
    fn google_calendar_without_client_id_does_not_open_a_browser() {
        let settings = crate::config::Settings::default();
        let err = match super::start_login("google-calendar", &settings) {
            Err(err) => err,
            Ok(_) => panic!("expected an error"),
        };
        assert!(err.contains("client ID"), "{err}");
    }

    #[test]
    fn apple_calendar_starts_by_opening_appleid() {
        let settings = crate::config::Settings::default();
        let (job, pending) = super::start_login("apple-calendar", &settings).expect("notice");
        assert!(
            job.url.starts_with("https://appleid.apple.com"),
            "{}",
            job.url
        );
        match pending {
            super::PendingLogin::Notice(msg) => assert!(msg.contains("app-specific")),
            _ => panic!("expected a notice login, not a 180s wait"),
        }
    }

    #[test]
    fn api_tokens_are_bound_to_origin() {
        assert_eq!(
            api_origin("https://api.example.com/v1/chat").unwrap(),
            "https://api.example.com"
        );
        assert_eq!(
            api_origin("https://api.example.com:8443/v1/chat").unwrap(),
            "https://api.example.com:8443"
        );
    }

    #[test]
    fn reserved_authorize_parameters_are_rejected() {
        let bad = Url::parse("https://id.example.com/auth?client_id=attacker").unwrap();
        let good = Url::parse("https://id.example.com/auth?audience=api").unwrap();
        assert!(reject_reserved_authorize_parameters(&bad).is_err());
        assert!(reject_reserved_authorize_parameters(&good).is_ok());
    }

    #[test]
    fn state_comparison_checks_length_and_content() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"diff"));
        assert!(!constant_time_eq(b"same", b"same-longer"));
    }

    #[test]
    fn callback_requires_exact_host_port_and_state() {
        let valid =
            "GET /callback?code=code-123&state=expected HTTP/1.1\r\nHost: 127.0.0.1:4242\r\n\r\n";
        match parse_callback_request(valid, 4242, "expected").unwrap() {
            Callback::Success(code) => assert_eq!(code, "code-123"),
            Callback::ProviderError(error) => panic!("unexpected provider error: {error}"),
        }

        let forged_state =
            "GET /callback?code=x&state=forged HTTP/1.1\r\nHost: 127.0.0.1:4242\r\n\r\n";
        assert!(parse_callback_request(forged_state, 4242, "expected").is_err());

        let forged_host =
            "GET /callback?code=x&state=expected HTTP/1.1\r\nHost: 127.0.0.1:9999\r\n\r\n";
        assert!(parse_callback_request(forged_host, 4242, "expected").is_err());
    }

    #[test]
    fn token_response_rejects_control_characters() {
        let response = TokenResponse {
            access_token: "valid-access-token".into(),
            refresh_token: "invalid\nrefresh-token".into(),
            expires_in: 3600,
            token_type: "Bearer".into(),
        };
        assert!(validate_token_response(&response).is_err());
    }
}
