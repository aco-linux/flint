use std::borrow::Cow;

/// Clock fields used by `{date}` / `{time}` / `{day}` / `{datetime}`.
#[derive(Debug, Clone)]
pub struct Stamp {
    pub date: String,
    pub time: String,
    pub day: String,
}

impl Stamp {
    pub fn local() -> Self {
        local_stamp().unwrap_or(Self {
            date: "1970-01-01".into(),
            time: "00:00".into(),
            day: "Thursday".into(),
        })
    }
}

/// Values substituted into a snippet or quicklink template.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    pub clipboard: &'a str,
    pub selection: &'a str,
    pub argument: &'a str,
    pub stamp: &'a Stamp,
    pub increment: u32,
}

impl<'a> Input<'a> {
    pub fn snippet(clipboard: &'a str, stamp: &'a Stamp, increment: u32) -> Self {
        Self {
            clipboard,
            selection: "",
            argument: "",
            stamp,
            increment,
        }
    }

    pub fn quicklink(argument: &'a str, stamp: &'a Stamp) -> Self {
        Self {
            clipboard: "",
            selection: "",
            argument,
            stamp,
            increment: 0,
        }
    }
}

/// Replace known placeholders. Unknown `{foo}` stays intact.
/// `{increment}` uses the current counter and returns current+1 when present.
/// `{argument}` / `{Query}` are URL-encoded when `template` looks like a URI.
pub fn expand(template: &str, input: &Input<'_>) -> (String, u32) {
    let encoded;
    let argument = if looks_like_uri(template) {
        encoded = urlencode(input.argument);
        encoded.as_str()
    } else {
        input.argument
    };
    let mut out = String::with_capacity(template.len());
    let mut next = input.increment;
    let mut i = 0;
    while i < template.len() {
        if template[i..].starts_with('{')
            && let Some(rel) = template[i + 1..].find('}')
        {
            let key = &template[i + 1..i + 1 + rel];
            let piece: Option<Cow<'_, str>> = match key {
                "clipboard" => Some(Cow::Borrowed(input.clipboard)),
                "selection" => Some(Cow::Borrowed(input.selection)),
                "date" => Some(Cow::Borrowed(input.stamp.date.as_str())),
                "time" => Some(Cow::Borrowed(input.stamp.time.as_str())),
                "day" => Some(Cow::Borrowed(input.stamp.day.as_str())),
                "datetime" => Some(Cow::Owned(format!(
                    "{} {}",
                    input.stamp.date, input.stamp.time
                ))),
                "increment" => {
                    next = input.increment.saturating_add(1);
                    Some(Cow::Owned(input.increment.to_string()))
                }
                "cursor" => Some(Cow::Borrowed("")),
                "argument" | "Query" | "query" | "QUERY" => Some(Cow::Borrowed(argument)),
                _ => None,
            };
            if let Some(piece) = piece {
                out.push_str(&piece);
                i += key.len() + 2;
                continue;
            }
        }
        let ch = template[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, next)
}

pub fn expand_snippet(text: &str, clipboard: &str, now: &Stamp, increment: u32) -> (String, u32) {
    expand(text, &Input::snippet(clipboard, now, increment))
}

pub fn expand_quicklink(target: &str, argument: &str) -> String {
    let stamp = Stamp::local();
    expand(target, &Input::quicklink(argument, &stamp)).0
}

fn looks_like_uri(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.contains("://") || lower.starts_with("javascript:")
}

fn urlencode(input: &str) -> String {
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

fn local_stamp() -> Option<Stamp> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as libc::time_t;
    const DAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    // SAFETY: `tm` is written by localtime_r before we read it.
    unsafe {
        let mut tm = std::mem::zeroed::<libc::tm>();
        if libc::localtime_r(&ts, &mut tm).is_null() {
            return None;
        }
        let wday = tm.tm_wday.clamp(0, 6) as usize;
        Some(Stamp {
            date: format!(
                "{:04}-{:02}-{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday
            ),
            time: format!("{:02}:{:02}", tm.tm_hour, tm.tm_min),
            day: DAYS[wday].to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Input, Stamp, expand};

    #[test]
    fn expand_table() {
        let now = Stamp {
            date: "2026-09-06".into(),
            time: "14:05".into(),
            day: "Sunday".into(),
        };
        let cases = [
            (
                "see {clipboard}",
                Input {
                    clipboard: "copied",
                    selection: "",
                    argument: "",
                    stamp: &now,
                    increment: 0,
                },
                "see copied",
                0u32,
            ),
            ("{date}", Input::snippet("", &now, 0), "2026-09-06", 0),
            ("{time}", Input::snippet("", &now, 0), "14:05", 0),
            (
                "{datetime}",
                Input::snippet("", &now, 0),
                "2026-09-06 14:05",
                0,
            ),
            ("{day}", Input::snippet("", &now, 0), "Sunday", 0),
            (
                "ticket-{increment}",
                Input::snippet("", &now, 7),
                "ticket-7",
                8,
            ),
            (
                "keep {foo} intact",
                Input::snippet("", &now, 0),
                "keep {foo} intact",
                0,
            ),
            ("x{cursor}y", Input::snippet("", &now, 0), "xy", 0),
            (
                "sel={selection}",
                Input {
                    clipboard: "",
                    selection: "hi there",
                    argument: "",
                    stamp: &now,
                    increment: 0,
                },
                "sel=hi there",
                0,
            ),
            (
                "https://github.com/search?q={argument}",
                Input::quicklink("rust gtk", &now),
                "https://github.com/search?q=rust+gtk",
                0,
            ),
            (
                "https://example.com?q={Query}",
                Input::quicklink("hi", &now),
                "https://example.com?q=hi",
                0,
            ),
            (
                "~/notes/{argument}.md",
                Input::quicklink("rust gtk", &now),
                "~/notes/rust gtk.md",
                0,
            ),
        ];
        for (text, input, want, next) in cases {
            let (got, got_next) = expand(text, &input);
            assert_eq!((got.as_str(), got_next), (want, next), "expand {text}");
        }
        let (once, next) = expand("{increment}/{increment}", &Input::snippet("", &now, 3));
        assert_eq!(once, "3/3");
        assert_eq!(next, 4);
        let (empty_sel, _) = expand("[{selection}]", &Input::snippet("", &now, 0));
        assert_eq!(empty_sel, "[]");
    }
}
