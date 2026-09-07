//! Native Ask AI tools. These run inside Flint (calendar, weather, inbox,
//! web search) — the model never launches a browser.

use serde_json::{Value, json};

use crate::config::Settings;

pub fn definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "get_calendar_today",
                "description": "List today's events from the user's connected calendars (Apple CalDAV, Google, Outlook). Use this instead of searching the web for the user's agenda.",
                "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Current local weather and rain/precipitation. Use this instead of searching the web for weather.",
                "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_inbox",
                "description": "Recent iCloud Mail using the Apple app-specific password. Use this instead of searching the web for email.",
                "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "DuckDuckGo Instant Answer for a factual lookup. Returns snippets, not a browser page.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        }
    ])
}

pub fn anthropic_definitions() -> Value {
    json!([
        {
            "name": "get_calendar_today",
            "description": "List today's events from the user's connected calendars.",
            "input_schema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_weather",
            "description": "Current local weather and precipitation.",
            "input_schema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_inbox",
            "description": "Recent iCloud Mail messages.",
            "input_schema": { "type": "object", "properties": {} }
        },
        {
            "name": "web_search",
            "description": "DuckDuckGo Instant Answer snippets.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }
        }
    ])
}

pub fn run(name: &str, args: &Value, settings: &Settings) -> String {
    match name {
        "get_calendar_today" => calendar_today(settings),
        "get_weather" => weather_now(),
        "get_inbox" => inbox(settings),
        "web_search" => {
            let q = args
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            web_search(q)
        }
        other => format!("Unknown tool {other}"),
    }
}

fn calendar_today(settings: &Settings) -> String {
    let mut lines = Vec::new();
    for id in [
        "apple-calendar",
        "proton-calendar",
        "google-calendar",
        "outlook",
    ] {
        match crate::connectors::fetch(id, settings) {
            Ok(items) => {
                for item in items.iter().take(12) {
                    if item.subtitle.is_empty() {
                        lines.push(item.title.clone());
                    } else {
                        lines.push(format!("{} — {}", item.subtitle, item.title));
                    }
                }
            }
            Err(err) => {
                if id == "apple-calendar" || id == "google-calendar" {
                    lines.push(format!("{id}: {err}"));
                }
            }
        }
    }
    if lines.is_empty() {
        "No calendar is connected. Set Apple ID and an app-specific password in Settings, or connect Google/Outlook.".into()
    } else {
        lines.join("\n")
    }
}

fn weather_now() -> String {
    match crate::weather::fetch() {
        Ok(snap) => {
            if snap.extra.is_empty() {
                format!("{} — {}", snap.location, snap.summary)
            } else {
                format!("{} — {} · {}", snap.location, snap.summary, snap.extra)
            }
        }
        Err(err) => err,
    }
}

fn inbox(settings: &Settings) -> String {
    if !crate::mail::connected(settings) {
        return "iCloud Mail is not connected. Set Apple ID and the app-specific password in Settings.".into();
    }
    match crate::mail::fetch_messages(settings) {
        Ok(msgs) if msgs.is_empty() => "Inbox is empty.".into(),
        Ok(msgs) => msgs
            .into_iter()
            .take(12)
            .map(|m| format!("{} — {}", m.from, m.subject))
            .collect::<Vec<_>>()
            .join("\n"),
        Err(err) => err,
    }
}

fn web_search(query: &str) -> String {
    if query.is_empty() {
        return "Missing search query".into();
    }
    match crate::web::fetch(query) {
        Ok(rows) => {
            let mut lines = Vec::new();
            for (item, live) in rows.iter().take(6) {
                lines.push(item.title.clone());
                if let crate::item::Live::Snippet { text } = live
                    && !text.is_empty()
                {
                    lines.push(text.clone());
                } else if !item.subtitle.is_empty() {
                    lines.push(item.subtitle.clone());
                }
            }
            if lines.is_empty() {
                format!("No instant answer for {query}")
            } else {
                lines.join("\n")
            }
        }
        Err(err) => err,
    }
}

#[cfg(test)]
mod tests {
    use super::definitions;

    #[test]
    fn ships_the_four_native_tools() {
        let tools = definitions();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.pointer("/function/name").and_then(|v| v.as_str()))
            .collect();
        assert!(names.contains(&"get_calendar_today"));
        assert!(names.contains(&"get_weather"));
        assert!(names.contains(&"get_inbox"));
        assert!(names.contains(&"web_search"));
    }
}
