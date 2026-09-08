//! Light iCloud IMAP inbox using the Apple ID + app-specific password
//! already stored for Calendar. No mail crate — one `curl` IMAPS fetch.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::auth;
use crate::config::Settings;
use crate::item::{Action, Icon, Item, Kind};

const IMAP_HOST: &str = "imap.mail.me.com";
const CACHE_FOR: Duration = Duration::from_secs(2 * 60);

#[derive(Debug, Clone)]
pub struct Message {
    pub uid: String,
    pub from: String,
    pub subject: String,
    pub date: String,
}

struct Cached {
    at: Instant,
    messages: Vec<Message>,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

pub fn connected(settings: &Settings) -> bool {
    !settings.connectors.apple_id.trim().is_empty() && auth::api_key("apple-calendar").is_some()
}

pub fn stub(settings: &Settings) -> Item {
    if connected(settings) {
        Item {
            id: "live:mail".into(),
            title: "Inbox".into(),
            subtitle: "Loading iCloud Mail…".into(),
            keywords: "email inbox mail unread icloud".into(),
            kind: Kind::Mail,
            icon: Icon::Name("mail-unread".into()),
            action: Action::Copy("Inbox".into()),
        }
    } else {
        disconnected()
    }
}

pub fn disconnected() -> Item {
    Item {
        id: "live:mail".into(),
        title: "Inbox".into(),
        subtitle: "Set Apple ID and app password in Settings to read iCloud Mail here".into(),
        keywords: "email inbox mail unread icloud".into(),
        kind: Kind::Mail,
        icon: Icon::Name("mail-unread".into()),
        action: Action::OpenPrefs {
            page: Some("connections".into()),
        },
    }
}

pub fn fetch(settings: &Settings) -> Result<Vec<Item>, String> {
    let messages = fetch_messages(settings)?;
    if messages.is_empty() {
        return Ok(vec![Item {
            id: "mail:empty".into(),
            title: "Inbox is empty".into(),
            subtitle: "No recent iCloud Mail".into(),
            keywords: "email inbox".into(),
            kind: Kind::Mail,
            icon: Icon::Name("mail-read".into()),
            action: Action::Copy("Inbox is empty".into()),
        }]);
    }
    Ok(messages.into_iter().take(15).map(message_item).collect())
}

pub fn fetch_messages(settings: &Settings) -> Result<Vec<Message>, String> {
    if let Some(hit) = cached() {
        return Ok(hit);
    }
    if !connected(settings) {
        return Err("Set Apple ID and the Apple app password in Settings first".into());
    }
    let user = settings.connectors.apple_id.trim();
    let password = auth::api_key("apple-calendar").unwrap_or_default();
    let xml = imap_fetch(user, &password)?;
    let messages = parse_fetch(&xml);
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cached {
            at: Instant::now(),
            messages: messages.clone(),
        });
    }
    Ok(messages)
}

fn cached() -> Option<Vec<Message>> {
    let guard = CACHE.lock().ok()?;
    let hit = guard.as_ref()?;
    if hit.at.elapsed() > CACHE_FOR {
        return None;
    }
    Some(hit.messages.clone())
}

fn message_item(msg: Message) -> Item {
    let from = if msg.from.is_empty() {
        "Unknown sender".into()
    } else {
        msg.from.clone()
    };
    let subject = if msg.subject.is_empty() {
        "(no subject)".into()
    } else {
        msg.subject.clone()
    };
    let copy = format!("{from} — {subject}");
    Item {
        id: format!("mail:{}", msg.uid),
        title: subject,
        subtitle: from,
        keywords: format!("email inbox mail {}", msg.date),
        kind: Kind::Mail,
        icon: Icon::Name("mail-unread".into()),
        action: Action::Copy(copy),
    }
}

fn imap_fetch(user: &str, password: &str) -> Result<String, String> {
    let cfg_path = crate::paths::runtime_dir().join(format!(
        "imap-curl-{}-{}.cfg",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let escaped = format!("{user}:{password}")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    crate::paths::write_private(&cfg_path, format!("user = \"{escaped}\"\n"))
        .map_err(|e| e.to_string())?;
    struct Remove(std::path::PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = Remove(cfg_path.clone());
    let url = format!("imaps://{IMAP_HOST}/INBOX");
    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "--fail-with-body",
            "--connect-timeout",
            "10",
            "--max-time",
            "20",
            "--proto",
            "=imaps",
            "-K",
            cfg_path.to_str().ok_or("IMAP config path is not UTF-8")?,
            "--request",
            "FETCH 1:15 BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE)]",
            &url,
        ])
        .output()
        .map_err(|e| format!("IMAP request failed: {e}"))?;
    if output.stdout.len() > 1024 * 1024 {
        return Err("IMAP response too large".into());
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() && text.trim().is_empty() {
        return Err("iCloud Mail lookup failed".into());
    }
    Ok(text)
}

pub fn parse_fetch(raw: &str) -> Vec<Message> {
    let mut messages = Vec::new();
    let mut uid = String::new();
    let mut from = String::new();
    let mut subject = String::new();
    let mut date = String::new();
    let mut in_headers = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        let upper = trimmed.to_ascii_uppercase();
        if upper.contains("FETCH") && (upper.starts_with('*') || upper.contains("UID")) {
            if !from.is_empty() || !subject.is_empty() {
                messages.push(take_message(&mut uid, &mut from, &mut subject, &mut date));
            }
            in_headers = true;
            if let Some(n) = fetch_seq(trimmed) {
                uid = n;
            }
            continue;
        }
        if !in_headers {
            continue;
        }
        if trimmed.is_empty() || trimmed == ")" {
            if !from.is_empty() || !subject.is_empty() {
                messages.push(take_message(&mut uid, &mut from, &mut subject, &mut date));
            }
            in_headers = false;
            continue;
        }
        if let Some(rest) = header(trimmed, "from:") {
            from = decode_from(rest);
        } else if let Some(rest) = header(trimmed, "subject:") {
            subject = rest.trim().to_string();
        } else if let Some(rest) = header(trimmed, "date:") {
            date = rest.trim().to_string();
        }
    }
    if !from.is_empty() || !subject.is_empty() {
        messages.push(take_message(&mut uid, &mut from, &mut subject, &mut date));
    }
    if messages.len() > 15 {
        messages.truncate(15);
    }
    messages
}

fn take_message(
    uid: &mut String,
    from: &mut String,
    subject: &mut String,
    date: &mut String,
) -> Message {
    let msg = Message {
        uid: if uid.is_empty() {
            format!("{}:{}", from, subject)
        } else {
            uid.clone()
        },
        from: std::mem::take(from),
        subject: std::mem::take(subject),
        date: std::mem::take(date),
    };
    uid.clear();
    msg
}

fn fetch_seq(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    while let Some(part) = parts.next() {
        if part == "*"
            && let Some(n) = parts.next()
            && n.chars().all(|c| c.is_ascii_digit())
        {
            return Some(n.to_string());
        }
        if part.eq_ignore_ascii_case("UID")
            && let Some(n) = parts.next()
        {
            let n = n.trim_end_matches(')');
            if n.chars().all(|c| c.is_ascii_digit()) {
                return Some(n.to_string());
            }
        }
    }
    None
}

fn header<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let lower = line.to_ascii_lowercase();
    if lower.starts_with(name) {
        Some(&line[name.len()..])
    } else {
        None
    }
}

fn decode_from(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(start) = raw.find('<')
        && let Some(end) = raw.find('>')
        && end > start
    {
        let name = raw[..start].trim().trim_matches('"').trim();
        if !name.is_empty() {
            return name.to_string();
        }
        return raw[start + 1..end].to_string();
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::{decode_from, parse_fetch};

    #[test]
    fn parses_imap_headers() {
        let raw = r#"* 1 FETCH (BODY[HEADER.FIELDS (FROM SUBJECT DATE)] {80}
From: Ada <ada@icloud.com>
Subject: Lunch
Date: Mon, 7 Sep 2026 09:00:00 +0000
)
* 2 FETCH (BODY[HEADER.FIELDS (FROM SUBJECT DATE)] {60}
From: "Bob" <bob@example.com>
Subject: Invoice
)
"#;
        let msgs = parse_fetch(raw);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].subject, "Lunch");
        assert_eq!(msgs[0].from, "Ada");
        assert_eq!(msgs[1].from, "Bob");
    }

    #[test]
    fn from_falls_back_to_address() {
        assert_eq!(decode_from("<x@y.com>"), "x@y.com");
        assert_eq!(decode_from("Ada Lovelace <ada@icloud.com>"), "Ada Lovelace");
    }

    #[test]
    fn imap_url_is_imaps() {
        assert!(super::IMAP_HOST.contains("mail.me.com"));
    }
}
