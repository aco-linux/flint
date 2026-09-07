//! CalDAV for Apple Calendar and generic CalDAV (Proton, Nextcloud).
//!
//! Apple and Proton do not issue a public third-party OAuth client for
//! Calendar on Linux. Flint opens the account page, stores an app password
//! in the keyring, and reads events over HTTPS CalDAV.

use crate::auth;
use crate::config::Settings;
use crate::item::{Action, Icon, Item, Kind};

pub struct Notice {
    pub url: String,
    pub message: String,
}

pub fn browser_notice(id: &str) -> Option<Notice> {
    match id {
        "apple-calendar" => Some(Notice {
            url: "https://appleid.apple.com/account/manage".into(),
            message: "Create an app-specific password at Apple ID, then set Apple ID and Apple app password in Settings.".into(),
        }),
        "proton-calendar" => Some(Notice {
            url: "https://calendar.proton.me".into(),
            message: "Proton Calendar has no public OAuth. If you have a CalDAV URL, set it plus the Proton app password in Settings.".into(),
        }),
        _ => None,
    }
}

pub fn items() -> Vec<Item> {
    vec![
        caldav_item(
            "apple-calendar",
            "Apple Calendar",
            "iCloud CalDAV · app-specific password",
        ),
        caldav_item(
            "proton-calendar",
            "Proton Calendar",
            "CalDAV URL + app password · no public OAuth",
        ),
    ]
}

fn caldav_item(id: &str, title: &str, subtitle: &str) -> Item {
    let connected = auth::api_key(id).is_some();
    Item {
        id: format!("ext:{id}"),
        title: title.into(),
        subtitle: if connected {
            "Connected · Enter lists events".into()
        } else {
            subtitle.into()
        },
        keywords: format!("calendar caldav {id} apple proton icloud"),
        kind: Kind::Extension,
        icon: Icon::Name("office-calendar".into()),
        action: if connected {
            Action::ConnectorFetch { id: id.into() }
        } else {
            Action::SignIn { provider: id.into() }
        },
    }
}

pub fn connect_item(id: &str, title: &str) -> Item {
    Item {
        id: format!("set:signin-{id}"),
        title: format!("Connect {title}"),
        subtitle: if auth::api_key(id).is_some() {
            "App password saved in the keyring".into()
        } else {
            "Opens the account page · then set the app password in Settings".into()
        },
        keywords: format!("oauth caldav {id}"),
        kind: Kind::Settings,
        icon: Icon::Name("network-workgroup".into()),
        action: Action::SignIn {
            provider: id.into(),
        },
    }
}

pub fn fetch(id: &str, settings: &Settings) -> Result<Vec<Item>, String> {
    let password = auth::api_key(id).ok_or_else(|| {
        format!("No app password for {id}. Connect it, then set the password in Settings.")
    })?;
    let (url, user) = match id {
        "apple-calendar" => (
            if settings.connectors.caldav_url.is_empty() {
                "https://caldav.icloud.com/".to_string()
            } else {
                settings.connectors.caldav_url.clone()
            },
            settings.connectors.apple_id.clone(),
        ),
        "proton-calendar" => (
            settings.connectors.caldav_url.clone(),
            settings.connectors.proton_user.clone(),
        ),
        _ => return Err("Unknown CalDAV connector".into()),
    };
    if user.trim().is_empty() {
        return Err("Set the calendar account (Apple ID or Proton user) in Settings first".into());
    }
    if !url.starts_with("https://") {
        return Err("CalDAV URL must be HTTPS".into());
    }
    let xml = report(&url, &user, &password)?;
    let events = parse_vevents(&xml);
    if events.is_empty() {
        return Ok(vec![Item {
            id: format!("caldav:{id}:empty"),
            title: "No upcoming events (or the CalDAV URL needs the calendar path)".into(),
            subtitle: url.clone(),
            keywords: String::new(),
            kind: Kind::Settings,
            icon: Icon::Name("dialog-information".into()),
            action: Action::Copy(url),
        }]);
    }
    Ok(events
        .into_iter()
        .take(24)
        .map(|ev| Item {
            id: format!("caldav:{}:{}", id, ev.uid),
            title: ev.summary,
            subtitle: ev.start.clone(),
            keywords: "calendar caldav".into(),
            kind: Kind::Web,
            icon: Icon::Name("office-calendar".into()),
            action: Action::Copy(ev.start),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub start: String,
}

pub fn parse_vevents(ics_or_xml: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut block = String::new();
    let mut in_event = false;
    for line in ics_or_xml.lines() {
        let line = line.trim_end();
        if line.contains("BEGIN:VEVENT") {
            in_event = true;
            block.clear();
            continue;
        }
        if line.contains("END:VEVENT") {
            if let Some(ev) = parse_event_block(&block) {
                events.push(ev);
            }
            in_event = false;
            continue;
        }
        if in_event {
            block.push_str(line);
            block.push('\n');
        }
    }
    events
}

fn parse_event_block(block: &str) -> Option<Event> {
    let mut summary = String::new();
    let mut start = String::new();
    let mut uid = String::new();
    for line in block.lines() {
        let upper = line.to_ascii_uppercase();
        if let Some(rest) = line.strip_prefix("SUMMARY:") {
            summary = unfold(rest);
        } else if upper.starts_with("DTSTART") {
            if let Some((_, rest)) = line.split_once(':') {
                start = rest.trim().to_string();
            }
        } else if let Some(rest) = line.strip_prefix("UID:") {
            uid = rest.trim().to_string();
        }
    }
    if summary.is_empty() {
        return None;
    }
    if uid.is_empty() {
        uid = summary.clone();
    }
    Some(Event { uid, summary, start })
}

fn unfold(input: &str) -> String {
    input.replace("\\n", " ").replace("\\,", ",").trim().into()
}

fn report(url: &str, user: &str, password: &str) -> Result<String, String> {
    let xml = r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <c:calendar-data/>
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT"/>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#;
    let cfg_path = crate::paths::runtime_dir().join(format!(
        "caldav-curl-{}-{}.cfg",
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
    let output = std::process::Command::new("curl")
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
            cfg_path.to_str().ok_or("CalDAV config path is not UTF-8")?,
            "-H",
            "Content-Type: application/xml",
            "-H",
            "Depth: 1",
            "-X",
            "REPORT",
            "--data-binary",
            xml,
            url,
        ])
        .output()
        .map_err(|e| format!("CalDAV request failed: {e}"))?;
    if output.stdout.len() > 2 * 1024 * 1024 {
        return Err("CalDAV response too large".into());
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() {
        return Err(format!(
            "CalDAV request failed: {}",
            text.chars().take(160).collect::<String>()
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::parse_vevents;

    #[test]
    fn parses_vevent_summary_and_start() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:abc\nSUMMARY:Standup\nDTSTART:20260907T150000Z\nEND:VEVENT\nEND:VCALENDAR\n";
        let events = parse_vevents(ics);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "Standup");
        assert_eq!(events[0].start, "20260907T150000Z");
    }

    #[test]
    fn apple_notice_is_https() {
        let n = super::browser_notice("apple-calendar").unwrap();
        assert!(n.url.starts_with("https://appleid.apple.com"));
    }
}
