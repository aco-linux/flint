use crate::item::{Action, Icon, Item, Kind};

const LANGS: &[(&str, &str)] = &[
    ("en", "English"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
    ("ru", "Russian"),
    ("ar", "Arabic"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("sv", "Swedish"),
    ("tr", "Turkish"),
    ("hi", "Hindi"),
    ("id", "Indonesian"),
    ("vi", "Vietnamese"),
    ("th", "Thai"),
    ("uk", "Ukrainian"),
    ("cs", "Czech"),
    ("ro", "Romanian"),
    ("hu", "Hungarian"),
    ("el", "Greek"),
    ("he", "Hebrew"),
    ("da", "Danish"),
    ("fi", "Finnish"),
    ("no", "Norwegian"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Translate {
        from: Option<String>,
        to: String,
        text: String,
    },
    Define {
        word: String,
    },
}

pub fn parse(query: &str) -> Option<Job> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let lower = q.to_ascii_lowercase();
    if let Some(rest) = strip_define(&lower, q) {
        let word = rest.trim();
        if word.is_empty() {
            return None;
        }
        return Some(Job::Define {
            word: word.to_string(),
        });
    }
    if let Some((from, to, text)) = parse_pair(q) {
        return Some(Job::Translate {
            from: Some(from),
            to,
            text,
        });
    }
    if let Some((to, text)) = parse_prefixed(q) {
        return Some(Job::Translate {
            from: None,
            to,
            text,
        });
    }
    None
}

pub fn item(query: &str) -> Option<Item> {
    let job = parse(query)?;
    Some(match job {
        Job::Translate { from, to, text } => {
            let dest = lang_name(&to);
            let title = match from.as_deref() {
                Some(src) => format!("Translate {} → {dest}", lang_name(src)),
                None => format!("Translate to {dest}"),
            };
            Item {
                id: format!("tr:{to}:{text}"),
                title,
                subtitle: format!("{text}  ·  Ask AI"),
                keywords: "translate translation language".into(),
                kind: Kind::Ai,
                icon: Icon::Name("preferences-desktop-locale".into()),
                action: Action::AskAi {
                    prompt: translate_prompt(from.as_deref(), &to, &text),
                },
            }
        }
        Job::Define { word } => Item {
            id: format!("define:{word}"),
            title: format!("Define “{word}”"),
            subtitle: "Ask AI · output only the definition".into(),
            keywords: "define dictionary meaning lookup".into(),
            kind: Kind::Ai,
            icon: Icon::Name("accessories-dictionary".into()),
            action: Action::AskAi {
                prompt: define_prompt(&word),
            },
        },
    })
}

fn strip_define<'a>(lower: &str, original: &'a str) -> Option<&'a str> {
    for prefix in ["define ", "definition of ", "meaning of ", "dictionary "] {
        if lower.starts_with(prefix) {
            return Some(&original[prefix.len()..]);
        }
    }
    None
}

fn parse_prefixed(query: &str) -> Option<(String, String)> {
    let lower = query.to_ascii_lowercase();
    let rest =
        after_prefix(query, &lower, "translate ").or_else(|| after_prefix(query, &lower, "tr "))?;
    let rest = rest.trim();
    let (code, text) = rest.split_once(char::is_whitespace)?;
    let code = code.trim_start_matches("to ").trim();
    let to = lang_code(code)?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some((to, text.to_string()))
}

fn parse_pair(query: &str) -> Option<(String, String, String)> {
    let (head, text) = query.split_once(char::is_whitespace)?;
    if !head.contains(':') || head.contains("://") {
        return None;
    }
    let (from, to) = head.split_once(':')?;
    let from = lang_code(from)?;
    let to = lang_code(to)?;
    let text = text.trim();
    if text.is_empty() || from == to {
        return None;
    }
    Some((from, to, text.to_string()))
}

fn after_prefix<'a>(original: &'a str, lower: &str, prefix: &str) -> Option<&'a str> {
    if lower.starts_with(prefix) {
        Some(&original[prefix.len()..])
    } else {
        None
    }
}

fn lang_code(raw: &str) -> Option<String> {
    let key = raw.trim().to_ascii_lowercase();
    LANGS
        .iter()
        .find(|(code, name)| *code == key || name.eq_ignore_ascii_case(&key))
        .map(|(code, _)| (*code).to_string())
}

fn lang_name(code: &str) -> &str {
    LANGS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
        .unwrap_or(code)
}

fn translate_prompt(from: Option<&str>, to: &str, text: &str) -> String {
    let dest = lang_name(to);
    match from {
        Some(src) => format!(
            "Translate from {} to {dest}. Output only the translation, nothing else.\n\n{text}",
            lang_name(src)
        ),
        None => format!(
            "Translate the following text to {dest}. Output only the translation, nothing else.\n\n{text}"
        ),
    }
}

fn define_prompt(word: &str) -> String {
    format!("Define “{word}” in one or two short sentences. Output only the definition.")
}

#[cfg(test)]
mod tests {
    use super::{Job, parse, translate_prompt};

    #[test]
    fn tr_and_translate_prefixes() {
        assert_eq!(
            parse("tr fr hello"),
            Some(Job::Translate {
                from: None,
                to: "fr".into(),
                text: "hello".into(),
            })
        );
        assert_eq!(
            parse("translate es buenos días"),
            Some(Job::Translate {
                from: None,
                to: "es".into(),
                text: "buenos días".into(),
            })
        );
        assert_eq!(
            parse("translate French thanks"),
            Some(Job::Translate {
                from: None,
                to: "fr".into(),
                text: "thanks".into(),
            })
        );
    }

    #[test]
    fn language_pair() {
        assert_eq!(
            parse("en:de thanks"),
            Some(Job::Translate {
                from: Some("en".into()),
                to: "de".into(),
                text: "thanks".into(),
            })
        );
        let prompt = translate_prompt(Some("en"), "de", "thanks");
        assert!(prompt.contains("English"));
        assert!(prompt.contains("German"));
        assert!(prompt.contains("Output only the translation"));
        assert!(prompt.contains("thanks"));
    }

    #[test]
    fn define_word() {
        assert_eq!(
            parse("define widget"),
            Some(Job::Define {
                word: "widget".into(),
            })
        );
        assert_eq!(
            parse("definition of widget").map(|job| matches!(job, Job::Define { .. })),
            Some(true)
        );
    }

    #[test]
    fn ignores_lookalikes() {
        assert!(parse("try firefox").is_none());
        assert!(parse("tree").is_none());
        assert!(parse("tr").is_none());
        assert!(parse("tr fr").is_none());
        assert!(parse("en:de").is_none());
        assert!(parse("https://example.com").is_none());
        assert!(parse("file:readme").is_none());
        assert!(parse("translator").is_none());
        assert!(parse("define").is_none());
    }
}
