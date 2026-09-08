//! CalDAV for Apple Calendar and generic CalDAV (Proton, Nextcloud).
//!
//! Apple and Proton do not issue a public third-party OAuth client for
//! Calendar on Linux. Flint opens the account page, stores an app password
//! in the keyring, and reads events over HTTPS CalDAV.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::auth;
use crate::config::Settings;
use crate::item::{Action, Icon, Item, Kind};

pub struct Notice {
    pub url: String,
    pub message: String,
}

struct CachedHref {
    at: Instant,
    href: String,
}

struct CachedEvents {
    at: Instant,
    events: Vec<Event>,
}

static HREFS: Mutex<Option<HashMap<String, CachedHref>>> = Mutex::new(None);
static EVENTS: Mutex<Option<HashMap<String, CachedEvents>>> = Mutex::new(None);
const HREF_TTL: Duration = Duration::from_secs(30 * 60);
const EVENTS_TTL: Duration = Duration::from_secs(2 * 60);

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

pub fn connected(id: &str) -> bool {
    auth::api_key(id).is_some()
}

fn caldav_item(id: &str, title: &str, subtitle: &str) -> Item {
    let connected = connected(id);
    Item {
        id: format!("ext:{id}"),
        title: title.into(),
        subtitle: if connected {
            "Connected · today’s events stay in Flint".into()
        } else {
            subtitle.into()
        },
        keywords: format!("calendar caldav {id} apple proton icloud"),
        kind: Kind::Extension,
        icon: Icon::Name("office-calendar".into()),
        action: if connected {
            Action::ConnectorFetch { id: id.into() }
        } else {
            Action::SignIn {
                provider: id.into(),
            }
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
            "Then set the app password in Settings — Flint reads CalDAV in-app".into()
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
    let events = fetch_events(id, settings)?;
    if events.is_empty() {
        return Ok(vec![Item {
            id: format!("caldav:{id}:empty"),
            title: "No events today".into(),
            subtitle: "Nothing on this calendar for the next day".into(),
            keywords: "calendar caldav".into(),
            kind: Kind::Calendar,
            icon: Icon::Name("office-calendar".into()),
            action: Action::Copy("No events today".into()),
        }]);
    }
    Ok(events.into_iter().take(24).map(event_item).collect())
}

pub fn fetch_events(id: &str, settings: &Settings) -> Result<Vec<Event>, String> {
    if let Some(hit) = cached_events(id) {
        return Ok(hit);
    }
    let password = auth::api_key(id).ok_or_else(|| {
        format!("No app password for {id}. Connect it, then set the password in Settings.")
    })?;
    let (root, user) = match id {
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
    if !root.starts_with("https://") {
        return Err("CalDAV URL must be HTTPS".into());
    }
    let calendars = discover_calendars(id, &root, &user, &password)?;
    if calendars.is_empty() {
        return Err("No CalDAV calendars found. Check the URL and app password.".into());
    }
    let (start, end) = time_range();
    let mut events = Vec::new();
    for href in calendars {
        let xml = report(&href, &user, &password, &start, &end)?;
        events.extend(parse_vevents(&xml));
    }
    events.sort_by(|a, b| a.start.cmp(&b.start));
    store_events(id, events.clone());
    Ok(events)
}

fn event_item(ev: Event) -> Item {
    let when = format_when(&ev.start);
    let copy = if when.is_empty() {
        ev.summary.clone()
    } else {
        format!("{} — {}", when, ev.summary)
    };
    Item {
        id: format!("caldav:{}", ev.uid),
        title: ev.summary,
        subtitle: when,
        keywords: "calendar caldav event agenda".into(),
        kind: Kind::Calendar,
        icon: Icon::Name("office-calendar".into()),
        action: Action::Copy(copy),
    }
}

fn discover_calendars(
    id: &str,
    root: &str,
    user: &str,
    password: &str,
) -> Result<Vec<String>, String> {
    if let Some(href) = cached_href(id) {
        return Ok(vec![href]);
    }
    let origin = origin_of(root)?;
    let mut url = join_url(&origin, root);
    if !url.contains("/calendars/")
        && id == "apple-calendar"
        && let Ok(known) = well_known(&origin, user, password)
    {
        url = known;
    }
    let principal = prop_href(
        &url,
        user,
        password,
        r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:">
  <d:prop><d:current-user-principal/></d:prop>
</d:propfind>"#,
        "current-user-principal",
    )
    .unwrap_or(url.clone());
    let principal = join_url(&origin, &principal);
    let home = prop_href(
        &principal,
        user,
        password,
        r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><c:calendar-home-set/></d:prop>
</d:propfind>"#,
        "calendar-home-set",
    )
    .unwrap_or(principal);
    let home = join_url(&origin, &home);
    let listing = dav(
        &home,
        user,
        password,
        "PROPFIND",
        "1",
        r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:displayname/>
    <d:resourcetype/>
    <c:supported-calendar-component-set/>
  </d:prop>
</d:propfind>"#,
    )?;
    let mut calendars = calendar_hrefs(&listing, &origin);
    if calendars.is_empty() {
        calendars.push(home);
    }
    if let Some(first) = calendars.first() {
        store_href(id, first.clone());
    }
    Ok(calendars)
}

fn well_known(origin: &str, user: &str, password: &str) -> Result<String, String> {
    let url = format!("{}/.well-known/caldav", origin.trim_end_matches('/'));
    dav(
        &url,
        user,
        password,
        "PROPFIND",
        "0",
        r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:">
  <d:prop><d:current-user-principal/></d:prop>
</d:propfind>"#,
    )?;
    Ok(url)
}

fn calendar_hrefs(xml: &str, origin: &str) -> Vec<String> {
    let mut out = Vec::new();
    for block in xml.split("<d:response").skip(1) {
        let lower = block.to_ascii_lowercase();
        if !lower.contains("calendar") {
            continue;
        }
        if lower.contains("supported-calendar-component-set") && !lower.contains("vevent") {
            continue;
        }
        if let Some(href) = first_href(block) {
            let url = join_url(origin, &href);
            if url.starts_with("https://") && !out.contains(&url) {
                out.push(url);
            }
        }
    }
    if out.is_empty() {
        for href in all_hrefs(xml) {
            let url = join_url(origin, &href);
            if url.starts_with("https://") && !out.contains(&url) {
                out.push(url);
            }
        }
    }
    out
}

fn prop_href(
    url: &str,
    user: &str,
    password: &str,
    body: &str,
    tag: &str,
) -> Result<String, String> {
    let xml = dav(url, user, password, "PROPFIND", "0", body)?;
    let needle = tag.to_ascii_lowercase();
    let lower = xml.to_ascii_lowercase();
    if let Some(idx) = lower.find(&needle)
        && let Some(href) = first_href(&xml[idx.min(xml.len())..])
    {
        return Ok(href);
    }
    first_href(&xml).ok_or_else(|| format!("CalDAV {tag} missing"))
}

fn first_href(xml: &str) -> Option<String> {
    all_hrefs(xml).into_iter().next()
}

fn all_hrefs(xml: &str) -> Vec<String> {
    let lower = xml.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = lower[from..]
        .find("<d:href>")
        .or_else(|| lower[from..].find("<href>"))
    {
        let abs = from + start;
        let after = xml[abs..]
            .find('>')
            .map(|i| abs + i + 1)
            .unwrap_or(xml.len());
        let Some(end_rel) = lower[after..]
            .find("</d:href>")
            .or_else(|| lower[after..].find("</href>"))
        else {
            break;
        };
        let href = xml[after..after + end_rel].trim().to_string();
        if !href.is_empty() {
            out.push(href);
        }
        from = after + end_rel + 1;
    }
    out
}

fn origin_of(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "CalDAV URL must be HTTPS".to_string())?;
    let host = rest.split('/').next().unwrap_or(rest);
    if host.is_empty() {
        return Err("CalDAV host missing".into());
    }
    Ok(format!("https://{host}"))
}

fn join_url(origin: &str, href: &str) -> String {
    let href = href.trim();
    if href.starts_with("https://") {
        return href.to_string();
    }
    if href.starts_with("http://") || href.is_empty() {
        return origin.to_string();
    }
    if href.starts_with('/') {
        return format!("{}{href}", origin.trim_end_matches('/'));
    }
    let origin = origin.trim_end_matches('/');
    format!("{origin}/{href}")
}

fn cached_href(id: &str) -> Option<String> {
    let guard = HREFS.lock().ok()?;
    let map = guard.as_ref()?;
    let hit = map.get(id)?;
    if hit.at.elapsed() > HREF_TTL {
        return None;
    }
    Some(hit.href.clone())
}

fn store_href(id: &str, href: String) {
    if let Ok(mut guard) = HREFS.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            id.to_string(),
            CachedHref {
                at: Instant::now(),
                href,
            },
        );
    }
}

fn cached_events(id: &str) -> Option<Vec<Event>> {
    let guard = EVENTS.lock().ok()?;
    let map = guard.as_ref()?;
    let hit = map.get(id)?;
    if hit.at.elapsed() > EVENTS_TTL {
        return None;
    }
    Some(hit.events.clone())
}

fn store_events(id: &str, events: Vec<Event>) {
    if let Ok(mut guard) = EVENTS.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            id.to_string(),
            CachedEvents {
                at: Instant::now(),
                events,
            },
        );
    }
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
    Some(Event {
        uid,
        summary,
        start,
    })
}

fn unfold(input: &str) -> String {
    input.replace("\\n", " ").replace("\\,", ",").trim().into()
}

pub fn format_when(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if let Some(pretty) = date_format(raw) {
        return pretty;
    }
    raw.to_string()
}

fn date_format(raw: &str) -> Option<String> {
    let mut spec = raw.to_string();
    if spec.len() == 8 && spec.chars().all(|c| c.is_ascii_digit()) {
        spec = format!("{spec}T000000");
    }
    if spec.len() >= 15 && spec.as_bytes().get(8) == Some(&b'T') {
        let compact = spec.replace(['-', ':'], "");
        let utc = compact.ends_with('Z');
        let y = &compact[0..4];
        let mo = &compact[4..6];
        let d = &compact[6..8];
        let h = compact.get(9..11).unwrap_or("00");
        let mi = compact.get(11..13).unwrap_or("00");
        let arg = if utc {
            format!("{y}-{mo}-{d} {h}:{mi}:00 UTC")
        } else {
            format!("{y}-{mo}-{d} {h}:{mi}:00")
        };
        return run_date(&arg);
    }
    if spec.contains('T') {
        return run_date(&spec.replace('T', " "));
    }
    None
}

fn run_date(input: &str) -> Option<String> {
    let output = std::process::Command::new("date")
        .args(["-d", input, "+%a %-I:%M %p"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn time_range() -> (String, String) {
    let start = date_shift(0).unwrap_or_else(|| "19700101T000000Z".into());
    let end = date_shift(2).unwrap_or_else(|| "19700103T000000Z".into());
    (start, end)
}

fn date_shift(days: i32) -> Option<String> {
    let output = std::process::Command::new("date")
        .args(["-u", "-d", &format!("{days} day"), "+%Y%m%dT000000Z"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn report(url: &str, user: &str, password: &str, start: &str, end: &str) -> Result<String, String> {
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <c:calendar-data/>
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{start}" end="{end}"/>
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#
    );
    dav(url, user, password, "REPORT", "1", &xml)
}

fn dav(
    url: &str,
    user: &str,
    password: &str,
    method: &str,
    depth: &str,
    body: &str,
) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err("CalDAV URL must be HTTPS".into());
    }
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
            &format!("Depth: {depth}"),
            "-X",
            method,
            "--data-binary",
            body,
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
    use super::{format_when, join_url, origin_of, parse_vevents};

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

    #[test]
    fn formats_ics_utc_timestamp() {
        let pretty = format_when("20260907T150000Z");
        assert!(
            pretty.contains(':')
                || pretty.contains("PM")
                || pretty.contains("AM")
                || pretty.contains("15"),
            "expected a clock time, got {pretty:?}"
        );
        assert!(!pretty.contains("T15"));
    }

    #[test]
    fn join_keeps_https() {
        assert_eq!(
            origin_of("https://caldav.icloud.com/").unwrap(),
            "https://caldav.icloud.com"
        );
        assert_eq!(
            join_url("https://caldav.icloud.com", "/123/calendars/home/"),
            "https://caldav.icloud.com/123/calendars/home/"
        );
        assert!(join_url("https://caldav.icloud.com", "http://evil").starts_with("https://"));
    }

    #[test]
    fn time_range_xml_uses_utc_compact() {
        let xml = super::time_range();
        assert!(xml.0.ends_with('Z'));
        assert!(xml.1.ends_with('Z'));
        assert!(xml.0.contains("T000000Z"));
    }
}
