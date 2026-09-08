//! In-app web results: DuckDuckGo HTML by default, Instant Answer JSON optional.
//! Enter copies the snippet. The action panel can open a chosen URL.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::files;
use crate::intent::{self, IntentKind};
use crate::item::{Action, Icon, Item, Kind, Live};
use crate::mode::Mode;

const CACHE_FOR: Duration = Duration::from_secs(60);
const LIVE_DEBOUNCE: Duration = Duration::from_millis(300);
const HIT_CAP: usize = 6;
/// Matches `catalog::TITLE_PREFIX`. A fuzzy title-prefix hit suppresses the
/// three-word web fallback so a typed app name is not buried under SERP rows.
pub const TITLE_PREFIX_BONUS: u32 = 4_000;

struct Cached {
    key: String,
    at: Instant,
    rows: Vec<(Item, Live)>,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

#[derive(Debug, Clone)]
pub struct Hit {
    pub title: String,
    pub snippet: String,
    pub url: String,
}

/// One backend for `web.provider`. `searxng` and `brave` are documented
/// fallbacks and currently resolve to DuckDuckGo HTML (no extra API keys).
pub trait Provider {
    fn search(&self, query: &str) -> Result<Vec<Hit>, String>;
}

struct Off;
struct InstantJson;
struct DdgHtml;

impl Provider for Off {
    fn search(&self, _query: &str) -> Result<Vec<Hit>, String> {
        Ok(Vec::new())
    }
}

impl Provider for InstantJson {
    fn search(&self, query: &str) -> Result<Vec<Hit>, String> {
        fetch_instant(query)
    }
}

impl Provider for DdgHtml {
    fn search(&self, query: &str) -> Result<Vec<Hit>, String> {
        let mut hits = Vec::new();
        if let Ok(instant) = fetch_instant(query)
            && let Some(bonus) = instant.into_iter().next()
        {
            hits.push(bonus);
        }
        if files::cancelled() {
            return Err("cancelled".into());
        }
        match fetch_html(query) {
            Ok(html_hits) => {
                for hit in html_hits {
                    if hits.len() >= HIT_CAP {
                        break;
                    }
                    if hits.iter().all(|existing| existing.title != hit.title) {
                        hits.push(hit);
                    }
                }
            }
            Err(err) if hits.is_empty() => return Err(err),
            Err(_) => {}
        }
        hits.truncate(HIT_CAP);
        Ok(hits)
    }
}

pub fn normalize_provider(name: &str) -> &'static str {
    let name = name.trim();
    if name.eq_ignore_ascii_case("off") {
        "off"
    } else if name.eq_ignore_ascii_case("instant") {
        "instant"
    } else {
        // ddg-html (default) · searxng · brave · anything unknown
        "ddg-html"
    }
}

fn provider_backend(name: &str) -> Box<dyn Provider> {
    match normalize_provider(name) {
        "off" => Box::new(Off),
        "instant" => Box::new(InstantJson),
        _ => Box::new(DdgHtml),
    }
}

pub fn search_item(query: &str) -> Item {
    let q = query.trim();
    Item {
        id: format!("search:{q}"),
        title: format!("Search the web for “{q}”"),
        subtitle: "Looking up web results in Flint…".into(),
        keywords: "google ddg web search".into(),
        kind: Kind::Web,
        icon: Icon::Name("system-search".into()),
        action: Action::Copy(q.to_string()),
    }
}

pub fn browser_fallback(query: &str) -> Item {
    let q = query.trim();
    let encoded = urlencoding_lite(q);
    Item {
        id: format!("search:browser:{q}"),
        title: format!("Open DuckDuckGo for “{q}”"),
        subtitle: "Opens the search page in your browser".into(),
        keywords: "google ddg web".into(),
        kind: Kind::Web,
        icon: Icon::Name("web-browser".into()),
        action: Action::OpenUri(format!("https://duckduckgo.com/?q={encoded}")),
    }
}

/// Title-tier bonus used to suppress the three-word web fallback.
pub fn title_bonus(query: &str, title: &str) -> u32 {
    let q_lc = query.trim().to_lowercase();
    if q_lc.is_empty() {
        return 0;
    }
    let title_lc = title.to_lowercase();
    if title_lc == q_lc {
        10_000
    } else if title_lc.starts_with(&q_lc) {
        TITLE_PREFIX_BONUS
    } else if title_lc
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| !word.is_empty() && word.starts_with(&q_lc))
    {
        2_500
    } else {
        0
    }
}

pub fn best_title_bonus<'a>(query: &str, items: impl Iterator<Item = &'a Item>) -> u32 {
    items
        .filter(|item| item.kind != Kind::Web && !item.id.starts_with("search:"))
        .map(|item| title_bonus(query, &item.title))
        .max()
        .unwrap_or(0)
}

/// When to spawn a live web fetch. Never a single-token app name; never when
/// a weather/calendar/mail/gif tool intent already owns the query.
pub fn should_fetch_web(query: &str, best_title_bonus: u32, provider: &str) -> bool {
    if normalize_provider(provider) == "off" {
        return false;
    }
    let (mode, rest) = Mode::parse(query);
    if mode != Mode::Root {
        return false;
    }
    let q = rest.trim();
    if q.is_empty() {
        return false;
    }
    let meaning = intent::resolve(q);
    if meaning.tool_intent() {
        return false;
    }
    if meaning.has(IntentKind::Web) {
        return true;
    }
    if q.ends_with('?') {
        return true;
    }
    let words = q.split_whitespace().count();
    words >= 3 && best_title_bonus < TITLE_PREFIX_BONUS
}

pub fn fetch(query: &str) -> Result<Vec<(Item, Live)>, String> {
    fetch_with(query, "ddg-html")
}

pub fn fetch_with(query: &str, provider: &str) -> Result<Vec<(Item, Live)>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let provider = normalize_provider(provider);
    if provider == "off" {
        return Ok(Vec::new());
    }
    let key = format!("{provider}\0{q}");
    if let Ok(guard) = CACHE.lock()
        && let Some(hit) = guard.as_ref()
        && hit.key == key
        && hit.at.elapsed() < CACHE_FOR
    {
        return Ok(hit.rows.clone());
    }
    let hits = provider_backend(provider).search(q)?;
    let rows = hits_to_rows(&hits);
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cached {
            key,
            at: Instant::now(),
            rows: rows.clone(),
        });
    }
    Ok(rows)
}

pub fn debounce_live() {
    let start = Instant::now();
    while start.elapsed() < LIVE_DEBOUNCE {
        if files::cancelled() {
            return;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

pub fn hits_to_rows(hits: &[Hit]) -> Vec<(Item, Live)> {
    hits.iter()
        .take(HIT_CAP)
        .enumerate()
        .map(|(i, hit)| {
            let item = hit_item(hit, i);
            let live = if hit.snippet.is_empty() {
                Live::None
            } else {
                Live::Snippet {
                    text: hit.snippet.clone(),
                }
            };
            (item, live)
        })
        .collect()
}

pub fn with_browser_fallback(query: &str, mut rows: Vec<(Item, Live)>) -> Vec<(Item, Live)> {
    rows.push((browser_fallback(query), Live::None));
    rows
}

pub fn parse(value: &Value) -> Vec<Hit> {
    let mut hits = Vec::new();
    let heading = value
        .get("Heading")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let abstract_text = value
        .get("AbstractText")
        .and_then(Value::as_str)
        .or_else(|| value.get("Abstract").and_then(Value::as_str))
        .unwrap_or("")
        .trim();
    let abstract_url = value
        .get("AbstractURL")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let answer = value
        .get("Answer")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let definition = value
        .get("Definition")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !answer.is_empty() {
        hits.push(Hit {
            title: if heading.is_empty() {
                answer.chars().take(72).collect()
            } else {
                heading.to_string()
            },
            snippet: answer.to_string(),
            url: https_only(abstract_url).unwrap_or_default(),
        });
    } else if !abstract_text.is_empty() {
        hits.push(Hit {
            title: if heading.is_empty() {
                abstract_text.chars().take(72).collect()
            } else {
                heading.to_string()
            },
            snippet: abstract_text.to_string(),
            url: https_only(abstract_url).unwrap_or_default(),
        });
    } else if !definition.is_empty() {
        hits.push(Hit {
            title: if heading.is_empty() {
                "Definition".into()
            } else {
                heading.to_string()
            },
            snippet: definition.to_string(),
            url: https_only(
                value
                    .get("DefinitionURL")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
            .unwrap_or_default(),
        });
    }
    if let Some(topics) = value.get("RelatedTopics").and_then(Value::as_array) {
        for topic in topics.iter().take(6) {
            if hits.len() >= HIT_CAP {
                break;
            }
            if let Some(hit) = related(topic)
                && hits.iter().all(|existing| existing.title != hit.title)
            {
                hits.push(hit);
            }
        }
    }
    hits
}

/// Defensive DuckDuckGo HTML parser. No HTML crate — scan `result__a` /
/// `result__snippet`, resolve `uddg=`, drop javascript: and non-https.
pub fn parse_ddg_html(html: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut from = 0;
    while hits.len() < HIT_CAP {
        let Some(rel) = find_result_anchor(&html[from..]) else {
            break;
        };
        let abs = from + rel;
        let Some((attrs, tag_end)) = parse_open_tag(&html[abs..]) else {
            from = abs + 1;
            continue;
        };
        let Some(inner_end) = html[abs + tag_end..].find("</a>") else {
            from = abs + 1;
            continue;
        };
        let title_raw = &html[abs + tag_end..abs + tag_end + inner_end];
        let title = html_unescape(&strip_tags(title_raw))
            .trim()
            .chars()
            .take(120)
            .collect::<String>();
        let after = abs + tag_end + inner_end + 4;
        from = after;
        if title.is_empty() {
            continue;
        }
        let href = attrs.get("href").map(String::as_str).unwrap_or("");
        let Some(url) = resolve_result_url(&html_unescape(href)) else {
            continue;
        };
        let window_end = html.len().min(after.saturating_add(2_500));
        let window = html.get(after..window_end).unwrap_or("");
        let snippet = snippet_near(window);
        hits.push(Hit {
            title,
            snippet,
            url,
        });
    }
    hits
}

fn related(value: &Value) -> Option<Hit> {
    if let Some(topics) = value.get("Topics").and_then(Value::as_array) {
        return related(topics.first()?);
    }
    let text = value.get("Text").and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }
    let url = value.get("FirstURL").and_then(Value::as_str).unwrap_or("");
    Some(Hit {
        title: text.chars().take(72).collect(),
        snippet: text.to_string(),
        url: https_only(url).unwrap_or_default(),
    })
}

fn hit_item(hit: &Hit, index: usize) -> Item {
    let copy = if hit.snippet.is_empty() {
        hit.title.clone()
    } else {
        hit.snippet.clone()
    };
    Item {
        id: format!("search:hit:{}:{}", index, slug(&hit.title)),
        title: hit.title.clone(),
        subtitle: if hit.url.is_empty() {
            "Enter copies".into()
        } else {
            hit.url.clone()
        },
        keywords: "web search ddg".into(),
        kind: Kind::Web,
        icon: Icon::Name("system-search".into()),
        action: if hit.url.is_empty() {
            Action::Copy(copy)
        } else {
            Action::OpenUri(hit.url.clone())
        },
    }
}

fn slug(title: &str) -> String {
    title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(32)
        .collect()
}

fn https_only(url: &str) -> Option<String> {
    let url = url.trim();
    url.starts_with("https://").then(|| url.to_string())
}

fn resolve_result_url(href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.starts_with("javascript:") {
        return None;
    }
    if let Some(direct) = https_only(href)
        && !direct.contains("duckduckgo.com/l/")
    {
        return Some(direct);
    }
    if let Some(uddg) = uddg_value(href) {
        return https_only(&percent_decode(&uddg));
    }
    None
}

fn uddg_value(href: &str) -> Option<String> {
    let marker = "uddg=";
    let idx = href.find(marker)?;
    let rest = &href[idx + marker.len()..];
    let encoded = rest.split('&').next().unwrap_or(rest);
    if encoded.is_empty() {
        return None;
    }
    Some(encoded.to_string())
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn fetch_instant(query: &str) -> Result<Vec<Hit>, String> {
    if files::cancelled() {
        return Err("cancelled".into());
    }
    let encoded = urlencoding_lite(query);
    let url = format!(
        "https://api.duckduckgo.com/?q={encoded}&format=json&no_html=1&no_redirect=1&skip_disambig=1"
    );
    let body = curl_get(&url)?;
    let value: Value =
        serde_json::from_slice(&body).map_err(|_| "Web search returned invalid JSON")?;
    Ok(parse(&value))
}

fn fetch_html(query: &str) -> Result<Vec<Hit>, String> {
    if files::cancelled() {
        return Err("cancelled".into());
    }
    let encoded = urlencoding_lite(query);
    let url = format!("https://html.duckduckgo.com/html/?q={encoded}");
    let body = curl_get(&url)?;
    let html = String::from_utf8_lossy(&body);
    Ok(parse_ddg_html(&html))
}

fn curl_get(url: &str) -> Result<Vec<u8>, String> {
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "4",
            "-A",
            "flint/0.4",
            "--compressed",
            url,
        ])
        .output()
        .map_err(|_| "curl is required for in-app web search".to_string())?;
    if !output.status.success() {
        return Err("Web search failed".into());
    }
    Ok(output.stdout)
}

fn find_result_anchor(html: &str) -> Option<usize> {
    let mut search = 0;
    while let Some(pos) = html[search..].find("result__a") {
        let abs = search + pos;
        if let Some(a_at) = html[..abs].rfind("<a") {
            let between = &html[a_at..abs];
            if !between.contains('>') {
                return Some(a_at);
            }
        }
        search = abs + 1;
    }
    None
}

fn parse_open_tag(html: &str) -> Option<(HashMap<String, String>, usize)> {
    let end = html.find('>')?;
    let raw = &html[1..end];
    let mut attrs = HashMap::new();
    let mut rest = raw;
    if let Some(space) = rest.find(char::is_whitespace) {
        rest = rest[space..].trim_start();
    } else {
        return Some((attrs, end + 1));
    }
    while !rest.is_empty() {
        let name_end = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let name = rest[..name_end].trim().to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        if name.is_empty() {
            break;
        }
        if rest.starts_with('=') {
            rest = rest[1..].trim_start();
            let (value, consumed) = parse_attr_value(rest);
            attrs.insert(name, value);
            rest = rest[consumed..].trim_start();
        } else {
            attrs.insert(name, String::new());
        }
    }
    Some((attrs, end + 1))
}

fn parse_attr_value(input: &str) -> (String, usize) {
    let bytes = input.as_bytes();
    if bytes.first() == Some(&b'"') || bytes.first() == Some(&b'\'') {
        let quote = bytes[0];
        if let Some(end) = input[1..].find(quote as char) {
            return (input[1..1 + end].to_string(), end + 2);
        }
        return (input[1..].to_string(), input.len());
    }
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    (input[..end].to_string(), end)
}

fn snippet_near(window: &str) -> String {
    let Some(pos) = window.find("result__snippet") else {
        return String::new();
    };
    let after_class = pos + "result__snippet".len();
    let Some(gt) = window[after_class..].find('>') else {
        return String::new();
    };
    let start = after_class + gt + 1;
    let rest = &window[start..];
    let end = rest
        .find("</")
        .or_else(|| rest.find('<'))
        .unwrap_or(rest.len().min(240));
    html_unescape(&strip_tags(&rest[..end]))
        .trim()
        .chars()
        .take(240)
        .collect()
}

fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

pub fn urlencoding_lite(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Action;
    use serde_json::json;

    const FIXTURE: &str = include_str!("../tests/fixtures/ddg-html.html");

    #[test]
    fn parses_abstract_and_related() {
        let value = json!({
            "Heading": "Paris",
            "AbstractText": "Capital of France.",
            "AbstractURL": "https://en.wikipedia.org/wiki/Paris",
            "RelatedTopics": [
                { "Text": "Paris, Texas", "FirstURL": "https://en.wikipedia.org/wiki/Paris,_Texas" },
                { "Text": "javascript:alert(1)", "FirstURL": "javascript:alert(1)" }
            ]
        });
        let hits = parse(&value);
        assert_eq!(hits[0].title, "Paris");
        assert!(hits[0].snippet.contains("France"));
        assert!(hits[0].url.starts_with("https://"));
        assert!(hits.iter().any(|h| h.title.contains("Texas")));
        assert!(
            hits.iter()
                .all(|h| h.url.is_empty() || h.url.starts_with("https://"))
        );
    }

    #[test]
    fn prefers_answer_field() {
        let value = json!({
            "Heading": "2 + 2",
            "Answer": "4",
            "AbstractText": ""
        });
        let hits = parse(&value);
        assert_eq!(hits[0].snippet, "4");
    }

    #[test]
    fn parse_ddg_html_keeps_https_and_uddg() {
        let hits = parse_ddg_html(FIXTURE);
        assert_eq!(
            hits.len(),
            2,
            "javascript and http rows must drop: {hits:?}"
        );
        assert_eq!(hits[0].title, "The Rust Programming Language");
        assert_eq!(hits[0].url, "https://www.rust-lang.org/");
        assert!(hits[0].snippet.contains("reliable"));
        assert_eq!(
            hits[1].url,
            "https://en.wikipedia.org/wiki/Rust_(programming_language)"
        );
        assert!(hits.iter().all(|h| h.url.starts_with("https://")));
        assert!(hits.iter().all(|h| !h.url.contains("javascript:")));
    }

    #[test]
    fn browser_fallback_is_last_after_hits() {
        let hits = parse_ddg_html(FIXTURE);
        let rows = with_browser_fallback("rust lang", hits_to_rows(&hits));
        assert!(rows.len() <= HIT_CAP + 1);
        let last = rows.last().expect("browser row");
        assert!(
            last.0.id.starts_with("search:browser:"),
            "browser must be last, got {}",
            last.0.id
        );
        assert!(matches!(last.0.action, Action::OpenUri(ref u) if u.contains("duckduckgo")));
        assert!(
            rows.iter()
                .filter(|(item, _)| item.id.starts_with("search:hit:"))
                .count()
                <= HIT_CAP
        );
    }

    #[test]
    fn never_fetches_single_token_app_match() {
        assert!(!should_fetch_web("firefox", 0, "ddg-html"));
        assert!(!should_fetch_web("Firefox", TITLE_PREFIX_BONUS, "ddg-html"));
        assert!(!should_fetch_web("smile", 0, "ddg-html"));
    }

    #[test]
    fn fetches_web_intent_and_question_suffix() {
        assert!(should_fetch_web("search rust", 0, "ddg-html"));
        assert!(should_fetch_web("lookup ownership", 0, "ddg-html"));
        assert!(should_fetch_web("install rustc?", 0, "ddg-html"));
        assert!(!should_fetch_web("? install rustc", 0, "ddg-html"));
    }

    #[test]
    fn three_words_only_without_prefix_hit() {
        assert!(should_fetch_web("best pizza berlin", 0, "ddg-html"));
        assert!(!should_fetch_web(
            "best pizza berlin",
            TITLE_PREFIX_BONUS,
            "ddg-html"
        ));
        assert!(!should_fetch_web("two words", 0, "ddg-html"));
    }

    #[test]
    fn provider_off_never_fetches() {
        assert!(!should_fetch_web("search rust", 0, "off"));
        assert!(!should_fetch_web("best pizza berlin", 0, "off"));
        assert_eq!(normalize_provider("searxng"), "ddg-html");
        assert_eq!(normalize_provider("brave"), "ddg-html");
        assert_eq!(normalize_provider("ddg-html"), "ddg-html");
    }

    #[test]
    fn tool_intents_do_not_schedule_web() {
        assert!(!should_fetch_web(
            "what's on my calendar today?",
            0,
            "ddg-html"
        ));
        assert!(!should_fetch_web("what's the weather", 0, "ddg-html"));
    }
}
