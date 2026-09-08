//! Authenticated connectors (calendar, tasks, notes).
//!
//! Each connector uses PKCE OAuth with a client ID you register at the
//! provider. Google Calendar reuses the Google client ID from Settings.
//! Tokens are stored per connector id, never mixed with Ask AI.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;
use url::Url;

use crate::auth;
use crate::config::Settings;
use crate::item::{Action, Icon, Item, Kind};

pub struct Preset {
    pub id: &'static str,
    pub title: &'static str,
    pub authorize: &'static str,
    pub token: &'static str,
    pub scopes: &'static str,
    pub api_origin: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "google-calendar",
        title: "Google Calendar",
        authorize: "https://accounts.google.com/o/oauth2/v2/auth",
        token: "https://oauth2.googleapis.com/token",
        scopes: "https://www.googleapis.com/auth/calendar.readonly",
        api_origin: "https://www.googleapis.com",
    },
    Preset {
        id: "outlook",
        title: "Outlook / Microsoft 365",
        authorize: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        token: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        scopes: "offline_access Calendars.Read",
        api_origin: "https://graph.microsoft.com",
    },
    Preset {
        id: "notion",
        title: "Notion",
        authorize: "https://api.notion.com/v1/oauth/authorize",
        token: "https://api.notion.com/v1/oauth/token",
        scopes: "",
        api_origin: "https://api.notion.com",
    },
    Preset {
        id: "todoist",
        title: "Todoist",
        authorize: "https://todoist.com/oauth/authorize",
        token: "https://todoist.com/oauth/access_token",
        scopes: "data:read",
        api_origin: "https://api.todoist.com",
    },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

pub struct BrowserNotice {
    pub url: String,
    pub message: String,
}

pub fn browser_notice(id: &str) -> Option<BrowserNotice> {
    crate::caldav::browser_notice(id).map(|n| BrowserNotice {
        url: n.url,
        message: n.message,
    })
}

pub fn client_id_for(id: &str, settings: &Settings) -> String {
    match id {
        "google-calendar" => settings.ai.client_id.clone(),
        "outlook" => settings.connectors.outlook_client_id.clone(),
        "notion" => settings.connectors.notion_client_id.clone(),
        "todoist" => settings.connectors.todoist_client_id.clone(),
        _ => String::new(),
    }
}

pub fn calendar_connected() -> bool {
    auth::load_for("google-calendar").is_some()
        || auth::load_for("outlook").is_some()
        || crate::caldav::connected("apple-calendar")
        || crate::caldav::connected("proton-calendar")
}

pub fn items() -> Vec<Item> {
    let mut items: Vec<Item> = PRESETS
        .iter()
        .map(|p| {
            let connected = auth::load_for(p.id).is_some();
            Item {
                id: format!("ext:{}", p.id),
                title: p.title.into(),
                subtitle: if connected {
                    "Connected · Enter lists today / inbox".into()
                } else {
                    format!("Connect with OAuth · {}", p.title)
                },
                keywords: format!("connector oauth calendar {}", p.id),
                kind: Kind::Extension,
                icon: Icon::Name("office-calendar".into()),
                action: if connected {
                    Action::ConnectorFetch { id: p.id.into() }
                } else {
                    Action::SignIn {
                        provider: p.id.into(),
                    }
                },
            }
        })
        .collect();
    items.extend(crate::caldav::items());
    items
}

pub fn connect_item(p: &Preset) -> Item {
    Item {
        id: format!("set:signin-{}", p.id),
        title: format!("Connect {}", p.title),
        subtitle: if auth::load_for(p.id).is_some() {
            auth::signed_in_label()
        } else {
            "PKCE · token stored in the keyring when available".into()
        },
        keywords: format!("oauth connect {}", p.id),
        kind: Kind::Settings,
        icon: Icon::Name("network-workgroup".into()),
        action: Action::SignIn {
            provider: p.id.into(),
        },
    }
}

pub fn fetch(id: &str, settings: &Settings) -> Result<Vec<Item>, String> {
    match id {
        "google-calendar" => google_today(),
        "outlook" => outlook_today(),
        "todoist" => todoist_inbox(),
        "notion" => notion_search(),
        "apple-calendar" | "proton-calendar" => crate::caldav::fetch(id, settings),
        _ => Err("Unknown connector".into()),
    }
}

fn google_today() -> Result<Vec<Item>, String> {
    let token = bearer("google-calendar")?;
    let now = chrono_now();
    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/primary/events?maxResults=16&singleEvents=true&orderBy=startTime&timeMin={now}"
    );
    let value = https_get(&url, &token, &[])?;
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return Ok(vec![message(
            "cal:empty",
            "No upcoming Google Calendar events",
        )]);
    }
    Ok(items.iter().filter_map(google_event).collect())
}

fn google_event(value: &Value) -> Option<Item> {
    let summary = value.get("summary")?.as_str()?.to_string();
    let start = value
        .get("start")
        .and_then(|s| s.get("dateTime").or_else(|| s.get("date")))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let when = crate::caldav::format_when(&start);
    let copy = if when.is_empty() {
        summary.clone()
    } else {
        format!("{when} — {summary}")
    };
    Some(Item {
        id: format!(
            "cal:{}",
            value.get("id").and_then(Value::as_str).unwrap_or(&summary)
        ),
        title: summary,
        subtitle: when,
        keywords: "calendar event google".into(),
        kind: Kind::Calendar,
        icon: Icon::Name("office-calendar".into()),
        action: Action::Copy(copy),
    })
}

fn outlook_today() -> Result<Vec<Item>, String> {
    let token = bearer("outlook")?;
    let value = https_get(
        "https://graph.microsoft.com/v1.0/me/events?$top=16&$orderby=start/dateTime",
        &token,
        &[],
    )?;
    let items = value
        .get("value")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return Ok(vec![message("ol:empty", "No upcoming Outlook events")]);
    }
    Ok(items
        .iter()
        .filter_map(|v| {
            let title = v.get("subject")?.as_str()?.to_string();
            let start = v
                .get("start")
                .and_then(|s| s.get("dateTime"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let when = crate::caldav::format_when(start);
            let copy = if when.is_empty() {
                title.clone()
            } else {
                format!("{when} — {title}")
            };
            Some(Item {
                id: format!(
                    "ol:{}",
                    v.get("id").and_then(Value::as_str).unwrap_or(&title)
                ),
                title,
                subtitle: when,
                keywords: "outlook calendar".into(),
                kind: Kind::Calendar,
                icon: Icon::Name("office-calendar".into()),
                action: Action::Copy(copy),
            })
        })
        .collect())
}

fn todoist_inbox() -> Result<Vec<Item>, String> {
    let token = bearer("todoist")?;
    let value = https_get("https://api.todoist.com/rest/v2/tasks", &token, &[])?;
    let items = value.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return Ok(vec![message("td:empty", "No open Todoist tasks")]);
    }
    Ok(items
        .iter()
        .take(24)
        .filter_map(|v| {
            let title = v.get("content")?.as_str()?.to_string();
            Some(Item {
                id: format!("td:{}", v.get("id")?.as_str().unwrap_or("")),
                title,
                subtitle: v
                    .get("due")
                    .and_then(|d| d.get("string"))
                    .and_then(Value::as_str)
                    .unwrap_or("Todoist")
                    .into(),
                keywords: "todoist task".into(),
                kind: Kind::Web,
                icon: Icon::Name("view-list-bullet".into()),
                action: Action::Copy(v.get("url").and_then(Value::as_str).unwrap_or("").into()),
            })
        })
        .collect())
}

fn notion_search() -> Result<Vec<Item>, String> {
    let token = bearer("notion")?;
    let value = https_post(
        "https://api.notion.com/v1/search",
        &token,
        &[
            ("Notion-Version", "2022-06-28"),
            ("Content-Type", "application/json"),
        ],
        "{}",
    )?;
    let items = value
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return Ok(vec![message("no:empty", "No Notion pages found")]);
    }
    Ok(items
        .iter()
        .take(16)
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?;
            let title = notion_title(v).unwrap_or_else(|| "Untitled".into());
            let url = v
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some(Item {
                id: format!("no:{id}"),
                title,
                subtitle: "Notion".into(),
                keywords: "notion page".into(),
                kind: Kind::Web,
                icon: Icon::Name("text-x-generic".into()),
                action: if url.starts_with("https://") {
                    Action::OpenUri(url)
                } else {
                    Action::Copy(id.into())
                },
            })
        })
        .collect())
}

fn notion_title(value: &Value) -> Option<String> {
    let props = value.get("properties")?;
    for (_k, v) in props.as_object()? {
        if v.get("type")?.as_str() == Some("title") {
            let arr = v.get("title")?.as_array()?;
            let mut out = String::new();
            for t in arr {
                if let Some(s) = t.get("plain_text").and_then(Value::as_str) {
                    out.push_str(s);
                }
            }
            if !out.is_empty() {
                return Some(out);
            }
        }
    }
    None
}

fn bearer(provider: &str) -> Result<String, String> {
    auth::access_token(provider)
}

struct CurlCfg(std::path::PathBuf);
impl Drop for CurlCfg {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn bearer_cfg(token: &str) -> Result<CurlCfg, String> {
    let path = crate::paths::runtime_dir().join(format!(
        "conn-curl-{}-{}.cfg",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let escaped = format!("Authorization: Bearer {token}")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    crate::paths::write_private(&path, format!("header = \"{escaped}\"\n"))
        .map_err(|e| e.to_string())?;
    Ok(CurlCfg(path))
}

fn https_get(url: &str, token: &str, extra: &[(&str, &str)]) -> Result<Value, String> {
    let parsed = Url::parse(url).map_err(|_| "Invalid connector URL".to_string())?;
    if parsed.scheme() != "https" {
        return Err("Connector URLs must be HTTPS".into());
    }
    let cfg = bearer_cfg(token)?;
    let extra_owned: Vec<String> = extra.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    let mut args = vec![
        "-sS".into(),
        "--fail-with-body".into(),
        "--connect-timeout".into(),
        "10".into(),
        "--max-time".into(),
        "20".into(),
        "--proto".into(),
        "=https".into(),
        "-K".into(),
        cfg.0.to_string_lossy().into_owned(),
        "-H".into(),
        "Accept: application/json".into(),
    ];
    for h in &extra_owned {
        args.push("-H".into());
        args.push(h.clone());
    }
    args.push(url.into());
    let str_args: Vec<&str> = args.iter().map(String::as_str).collect();
    curl_json(&str_args)
}

fn https_post(url: &str, token: &str, extra: &[(&str, &str)], body: &str) -> Result<Value, String> {
    let parsed = Url::parse(url).map_err(|_| "Invalid connector URL".to_string())?;
    if parsed.scheme() != "https" {
        return Err("Connector URLs must be HTTPS".into());
    }
    let cfg = bearer_cfg(token)?;
    let mut child = Command::new("curl")
        .args([
            "-sS",
            "--fail-with-body",
            "--connect-timeout",
            "10",
            "--max-time",
            "20",
            "--proto",
            "=https",
            "-K",
            cfg.0.to_str().ok_or("curl config path is not UTF-8")?,
            "-X",
            "POST",
            "-H",
            "Accept: application/json",
        ])
        .args(
            extra
                .iter()
                .flat_map(|(k, v)| ["-H".into(), format!("{k}: {v}")]),
        )
        .args(["--data-binary", "@-", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Connector request failed: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(body.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Connector request failed: {e}"))?;
    parse_curl(output)
}

fn curl_json(args: &[&str]) -> Result<Value, String> {
    let output = Command::new("curl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Connector request failed: {e}"))?;
    parse_curl(output)
}

fn parse_curl(output: std::process::Output) -> Result<Value, String> {
    if output.stdout.len() > 2 * 1024 * 1024 {
        return Err("Connector response too large".into());
    }
    if !output.status.success() {
        let body = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "Connector request failed: {}",
            body.chars().take(180).collect::<String>()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("Connector JSON invalid: {e}"))
}

fn message(id: &str, title: &str) -> Item {
    Item {
        id: id.into(),
        title: title.into(),
        subtitle: String::new(),
        keywords: String::new(),
        kind: Kind::Settings,
        icon: Icon::Name("dialog-information".into()),
        action: Action::Copy(title.into()),
    }
}

fn chrono_now() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() >= 20)
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into())
}

#[cfg(test)]
mod tests {
    use super::{google_event, preset};
    use serde_json::json;

    #[test]
    fn knows_google_calendar_preset() {
        let p = preset("google-calendar").unwrap();
        assert!(p.scopes.contains("calendar"));
        assert!(p.authorize.starts_with("https://"));
    }

    #[test]
    fn parses_google_event() {
        let v = json!({
            "id": "abc",
            "summary": "Standup",
            "start": { "dateTime": "2026-09-07T15:00:00Z" },
            "htmlLink": "https://calendar.google.com/event?eid=abc"
        });
        let item = google_event(&v).unwrap();
        assert_eq!(item.title, "Standup");
        match item.action {
            crate::item::Action::Copy(text) => assert!(text.contains("Standup")),
            other => panic!("expected copy, got {other:?}"),
        }
    }
}
