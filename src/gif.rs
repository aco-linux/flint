//! GIF search via Tenor (optional key in the keyring / `TENOR_API_KEY`) or
//! Wikimedia Commons. Without a Tenor key, Commons still returns GIFs and
//! Flint also offers a Tenor page that opens in the browser.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::item::{Action, Icon, Item, Kind};

const TENOR_SEARCH: &str = "https://tenor.googleapis.com/v2/search";
const TENOR_LEGACY: &str = "https://g.tenor.com/v1/search";

#[derive(Debug, Clone)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub url: String,
    pub preview: String,
}

pub fn search_item(query: &str) -> Item {
    let q = query.trim();
    if q.is_empty() {
        Item {
            id: "gif:help".into(),
            title: "Search GIFs".into(),
            subtitle: "Type gif cats · add a Tenor API key in Settings for in-launcher results"
                .into(),
            keywords: "gif giphy tenor".into(),
            kind: Kind::Web,
            icon: Icon::Name("image-x-generic".into()),
            action: Action::EnterMode(crate::mode::Mode::Gif),
        }
    } else {
        Item {
            id: format!("gif:web:{q}"),
            title: format!("Search Tenor for “{q}”"),
            subtitle: "Opens tenor.com in your browser".into(),
            keywords: "gif tenor".into(),
            kind: Kind::Web,
            icon: Icon::Name("web-browser".into()),
            action: Action::OpenUri(format!(
                "https://tenor.com/search/{}-gifs",
                urlencoding_lite(q)
            )),
        }
    }
}

pub fn search(query: &str, key: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    if !key.trim().is_empty() {
        if let Ok(hits) = tenor_search(TENOR_SEARCH, q, key.trim(), limit, true)
            && !hits.is_empty()
        {
            return Ok(hits);
        }
        if let Ok(hits) = tenor_search(TENOR_LEGACY, q, key.trim(), limit, false)
            && !hits.is_empty()
        {
            return Ok(hits);
        }
    }
    commons_search(q, limit)
}

pub(crate) fn copy_gif_bytes(url: &str) -> bool {
    if !url.starts_with("https://") {
        return false;
    }
    let Ok(output) = Command::new("curl")
        .args([
            "-sS",
            "--fail",
            "--connect-timeout",
            "8",
            "--max-time",
            "15",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "-L",
            url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };
    if !output.status.success()
        || output.stdout.is_empty()
        || output.stdout.len() > 8 * 1024 * 1024
    {
        return false;
    }
    let Ok(mut child) = Command::new("wl-copy")
        .args(["--type", "image/gif"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take()
        && stdin.write_all(&output.stdout).is_err()
    {
        return false;
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

pub fn to_item(hit: &Hit) -> Item {
    Item {
        id: format!("gif:{}", hit.id),
        title: if hit.title.is_empty() {
            "GIF".into()
        } else {
            hit.title.clone()
        },
        subtitle: "Enter copies the GIF URL, then the image when the download finishes".into(),
        keywords: format!("gif {}", hit.title),
        kind: Kind::Media,
        icon: Icon::Name("image-x-generic".into()),
        action: Action::Copy(hit.url.clone()),
    }
}

fn tenor_search(
    endpoint: &str,
    query: &str,
    key: &str,
    limit: usize,
    v2: bool,
) -> Result<Vec<Hit>, String> {
    if !endpoint.starts_with("https://") {
        return Err("GIF search URL must be HTTPS".into());
    }
    let cfg_path = crate::paths::runtime_dir().join(format!(
        "tenor-curl-{}-{}.cfg",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let mut cfg = format!("url = \"{}\"\n", curl_cfg_escape(endpoint));
    cfg.push_str(&format!(
        "data-urlencode = \"q={}\"\n",
        curl_cfg_escape(query)
    ));
    cfg.push_str(&format!(
        "data-urlencode = \"key={}\"\n",
        curl_cfg_escape(key)
    ));
    cfg.push_str(&format!(
        "data-urlencode = \"limit={}\"\n",
        limit.min(24)
    ));
    if v2 {
        cfg.push_str("data-urlencode = \"media_filter=gif,tinygif\"\n");
    }
    crate::paths::write_private(&cfg_path, cfg).map_err(|e| e.to_string())?;
    struct Remove(PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _guard = Remove(cfg_path.clone());
    let output = Command::new("curl")
        .args([
            "-sS",
            "--fail-with-body",
            "--connect-timeout",
            "8",
            "--max-time",
            "12",
            "--proto",
            "=https",
            "-G",
            "-K",
            cfg_path.to_str().ok_or("Tenor config path is not UTF-8")?,
            "-H",
            "Accept: application/json",
            "-H",
            "User-Agent: Flint/0.4 (https://github.com/aco-linux/flint; GIF picker)",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("GIF search failed: {e}"))?;
    if output.stdout.len() > 2 * 1024 * 1024 {
        return Err("GIF search response too large".into());
    }
    if !output.status.success() {
        return Err("GIF search failed".into());
    }
    let value: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("GIF JSON invalid: {e}"))?;
    if v2 {
        parse_v2(&value)
    } else {
        parse_v1(&value)
    }
}

fn curl_cfg_escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn fetch_json(url: &str) -> Result<Value, String> {
    if !url.starts_with("https://") {
        return Err("GIF search URL must be HTTPS".into());
    }
    let output = Command::new("curl")
        .args([
            "-sS",
            "--fail-with-body",
            "--connect-timeout",
            "8",
            "--max-time",
            "12",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "-H",
            "Accept: application/json",
            "-H",
            "User-Agent: Flint/0.4 (https://github.com/aco-linux/flint; GIF picker)",
            url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("GIF search failed: {e}"))?;
    if output.stdout.len() > 2 * 1024 * 1024 {
        return Err("GIF search response too large".into());
    }
    if !output.status.success() {
        return Err("GIF search failed".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("GIF JSON invalid: {e}"))
}

fn parse_v2(value: &Value) -> Result<Vec<Hit>, String> {
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or("no results")?;
    Ok(results.iter().filter_map(hit_v2).collect())
}

fn parse_v1(value: &Value) -> Result<Vec<Hit>, String> {
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or("no results")?;
    Ok(results.iter().filter_map(hit_v1).collect())
}

fn hit_v2(value: &Value) -> Option<Hit> {
    let id = value.get("id")?.as_str()?.to_string();
    let title = value
        .get("content_description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let media = value.get("media_formats")?;
    let url = media
        .get("gif")
        .and_then(|m| m.get("url"))
        .and_then(Value::as_str)
        .or_else(|| {
            media
                .get("tinygif")
                .and_then(|m| m.get("url"))
                .and_then(Value::as_str)
        })?
        .to_string();
    if !url.starts_with("https://") {
        return None;
    }
    let preview = media
        .get("tinygif")
        .and_then(|m| m.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(Hit {
        id,
        title,
        url,
        preview,
    })
}

fn hit_v1(value: &Value) -> Option<Hit> {
    let id = value.get("id")?.as_str()?.to_string();
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let media = value.get("media")?.as_array()?.first()?;
    let url = media
        .get("gif")
        .and_then(|m| m.get("url"))
        .and_then(Value::as_str)?
        .to_string();
    if !url.starts_with("https://") {
        return None;
    }
    let preview = media
        .get("tinygif")
        .and_then(|m| m.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(Hit {
        id,
        title,
        url,
        preview,
    })
}

fn commons_search(query: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let encoded = urlencoding_lite(query);
    let url = format!(
        "https://commons.wikimedia.org/w/api.php?action=query&format=json&generator=search&gsrsearch=filemime:image/gif+{encoded}&gsrlimit={}&prop=imageinfo&iiprop=url&gsrnamespace=6",
        limit.min(16)
    );
    let value = fetch_json(&url)?;
    let hits = parse_commons(&value);
    if hits.is_empty() {
        Err("No GIFs found".into())
    } else {
        Ok(hits)
    }
}

pub(crate) fn parse_commons(value: &Value) -> Vec<Hit> {
    let Some(pages) = value
        .get("query")
        .and_then(|q| q.get("pages"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for (id, page) in pages {
        let title = page
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("GIF")
            .trim_start_matches("File:")
            .to_string();
        let Some(url) = page
            .get("imageinfo")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|i| i.get("url"))
            .and_then(Value::as_str)
            .and_then(clean_commons_url)
        else {
            continue;
        };
        hits.push(Hit {
            id: id.clone(),
            title,
            url: url.clone(),
            preview: url,
        });
    }
    hits
}

fn clean_commons_url(url: &str) -> Option<String> {
    if !url.starts_with("https://upload.wikimedia.org/") {
        return None;
    }
    let base = url.split('?').next().unwrap_or(url);
    let lower = base.to_ascii_lowercase();
    lower.ends_with(".gif").then(|| base.to_string())
}

fn urlencoding_lite(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{hit_v1, parse_v2};
    use serde_json::json;

    #[test]
    fn parses_tenor_v2() {
        let value = json!({
            "results": [{
                "id": "1",
                "content_description": "cat",
                "media_formats": {
                    "gif": { "url": "https://media.tenor.com/cat.gif" },
                    "tinygif": { "url": "https://media.tenor.com/cat-tiny.gif" }
                }
            }]
        });
        let hits = parse_v2(&value).unwrap();
        assert_eq!(hits[0].url, "https://media.tenor.com/cat.gif");
    }

    #[test]
    fn rejects_non_https_gif() {
        let value = json!({
            "id": "1",
            "media": [{ "gif": { "url": "http://evil.example/x.gif" } }]
        });
        assert!(hit_v1(&value).is_none());
    }

    #[test]
    fn parses_commons_gif_and_strips_tracking() {
        let value = json!({
            "query": {
                "pages": {
                    "1": {
                        "title": "File:Cat funny gif.gif",
                        "imageinfo": [{
                            "url": "https://upload.wikimedia.org/wikipedia/commons/8/81/Cat_funny_gif.gif?utm_source=commons"
                        }]
                    }
                }
            }
        });
        let hits = super::parse_commons(&value);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].url,
            "https://upload.wikimedia.org/wikipedia/commons/8/81/Cat_funny_gif.gif"
        );
    }

    #[test]
    #[ignore = "hits commons.wikimedia.org"]
    fn live_commons_returns_https_gifs() {
        let hits = super::search("cat", "", 4).expect("commons GIF search");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.url.starts_with("https://upload.wikimedia.org/")));
    }

    #[test]
    fn tenor_search_item_is_https() {
        let item = super::search_item("cats");
        match item.action {
            crate::item::Action::OpenUri(url) => {
                assert!(url.starts_with("https://tenor.com/"));
            }
            other => panic!("expected OpenUri, got {other:?}"),
        }
    }
}
