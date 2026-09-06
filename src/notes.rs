use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::db;
use crate::item::{Action, Icon, Item, Kind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub updated: u64,
}

impl Note {
    pub fn to_item(&self) -> Item {
        let preview = self
            .body
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("Empty note")
            .chars()
            .take(80)
            .collect::<String>();
        Item {
            id: format!("note:{}", self.id),
            title: self.title.clone(),
            subtitle: preview,
            keywords: format!("note notes {}", self.body),
            kind: Kind::Note,
            icon: Icon::Name(
                if self.pinned {
                    "starred"
                } else {
                    "text-x-generic"
                }
                .into(),
            ),
            action: Action::OpenNote {
                id: self.id.clone(),
            },
        }
    }
}

pub fn load() -> Vec<Note> {
    db::notes_load().unwrap_or_default()
}

pub fn get(id: &str) -> Option<Note> {
    db::note_get(id)
}

pub fn upsert(note: Note) {
    let _ = db::note_upsert(&note);
}

pub fn create(title: &str) -> Note {
    let now = now();
    let title = {
        let t = title.trim();
        if t.is_empty() { "Untitled" } else { t }
    };
    let note = Note {
        id: format!("{now:x}"),
        title: title.to_string(),
        body: String::new(),
        pinned: false,
        updated: now,
    };
    upsert(note.clone());
    note
}

pub fn save_body(id: &str, title: &str, body: &str) -> Option<Note> {
    let mut note = get(id)?;
    note.title = if title.trim().is_empty() {
        note.title
    } else {
        title.trim().to_string()
    };
    note.body = body.to_string();
    note.updated = now();
    let _ = db::note_upsert(&note);
    Some(note)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::Note;

    #[test]
    fn item_uses_first_line_as_preview() {
        let note = Note {
            id: "1".into(),
            title: "Ship checklist".into(),
            body: "\nBuy milk\nCall bank".into(),
            pinned: false,
            updated: 1,
        };
        assert_eq!(note.to_item().subtitle, "Buy milk");
    }
}
