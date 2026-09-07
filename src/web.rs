//! DuckDuckGo Instant Answer rows, shown inside Flint.
//! Enter copies the snippet. The action panel can open a chosen URL.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::item::{Action, Icon, Item, Kind, Live};

const CACHE_FOR: Duration = Duration::from_secs(60);

struct Cached {
    query: String,
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

pub fn search_item(query: &str) -> Item {
    let q = query.trim();
    Item {
        id: format!("search:{q}"),
        title: format!("Search the web for “{q}”"),
        subtitle: "Looking up Instant Answers in Flint…".into(),
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

pub fn fetch(query: &str) -> Result<Vec<(Item, Live)>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(guard) = CACHE.lock()
        && let Some(hit) = guard.as_ref()
        && hit.query == q
        && hit.at.elapsed() < CACHE_FOR
    {
        return Ok(hit.rows.clone());
    }
    let encoded = urlencoding_lite(q);
    let url = format!(
        "https://api.duckduckgo.com/?q={encoded}&format=json&no_html=1&no_redirect=1&skip_disambig=1"
    );
    let output = std::process::Command::new("curl")
        .args(["-fsS", "--max-time", "4", "-A", "flint/0.4", &url])
        .output()
        .map_err(|_| "curl is required for in-app web search".to_string())?;
    if !output.status.success() {
        return Err("Web search failed".into());
    }
    let value: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "Web search returned invalid JSON")?;
    let hits = parse(&value);
    let rows = if hits.is_empty() {
        vec![(
            Item {
                id: format!("search:empty:{q}"),
                title: format!("No instant answer for “{q}”"),
                subtitle: "Enter copies the query · action panel opens the browser".into(),
                keywords: "web search".into(),
                kind: Kind::Web,
                icon: Icon::Name("system-search".into()),
                action: Action::Copy(q.to_string()),
            },
            Live::None,
        )]
    } else {
        hits.into_iter()
            .enumerate()
            .map(|(i, hit)| {
                let item = hit_item(&hit, i);
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
    };
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cached {
            query: q.to_string(),
            at: Instant::now(),
            rows: rows.clone(),
        });
    }
    Ok(rows)
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
            if hits.len() >= 6 {
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
        action: Action::Copy(copy),
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
    use super::parse;
    use serde_json::json;

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
}
