use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::cell::RefCell;
#[cfg(not(test))]
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::ai::{ChatMessage, Thread};
use crate::calc;
use crate::clipboard::{self, Entry as Clip};
use crate::layout::{NamedLayout, Slot};
use crate::memory::Fact;
use crate::notes::Note;
use crate::paths;
use crate::quicklinks::Link;
use crate::snippets::Snippet;
use crate::usage::{self, Record};
use crate::voice::Entry as Dictation;

const SCHEMA_VERSION: &str = "1";
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS clips (
    id TEXT PRIMARY KEY NOT NULL,
    text TEXT NOT NULL,
    copied_at INTEGER NOT NULL,
    pinned INTEGER NOT NULL DEFAULT 0,
    label TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    pinned INTEGER NOT NULL DEFAULT 0,
    updated INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS snippets (
    keyword TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    text TEXT NOT NULL,
    increment INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS aliases (
    id TEXT PRIMARY KEY NOT NULL,
    alias TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS favorites (
    id TEXT PRIMARY KEY NOT NULL,
    pos INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS calc_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    expr TEXT NOT NULL,
    result TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS usage (
    id TEXT PRIMARY KEY NOT NULL,
    count INTEGER NOT NULL,
    last INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS choices (
    query TEXT NOT NULL,
    item_id TEXT NOT NULL,
    count INTEGER NOT NULL,
    last INTEGER NOT NULL,
    PRIMARY KEY(query, item_id)
);
CREATE INDEX IF NOT EXISTS choices_query ON choices(query);
CREATE TABLE IF NOT EXISTS quicklinks (
    name TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    target TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS layouts (
    name TEXT PRIMARY KEY NOT NULL,
    slots TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS quit_keep (
    class TEXT PRIMARY KEY NOT NULL
);
CREATE TABLE IF NOT EXISTS dictation (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    text TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ai_threads (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    created INTEGER NOT NULL,
    updated INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ai_messages (
    id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL,
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS memory (
    id TEXT PRIMARY KEY NOT NULL,
    text TEXT NOT NULL,
    at INTEGER NOT NULL
);
";

const MAX_DICTATION: i64 = 100;
const MAX_MEMORY: i64 = 50;

struct Db {
    conn: Connection,
}

#[cfg(test)]
thread_local! {
    static STATE: RefCell<Option<Db>> = const { RefCell::new(None) };
}

#[cfg(not(test))]
static STATE: Mutex<Option<Db>> = Mutex::new(None);

fn mutate_state<R>(f: impl FnOnce(&mut Option<Db>) -> R) -> R {
    #[cfg(test)]
    {
        STATE.with(|cell| f(&mut cell.borrow_mut()))
    }
    #[cfg(not(test))]
    {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut state)
    }
}

/// Open the user database at `~/.local/share/flint/flint.db`.
/// Tests must call [`open_path`] so they never touch the real file.
pub fn open() -> rusqlite::Result<()> {
    mutate_state(|state| {
        if state.is_some() {
            return Ok(());
        }
        if cfg!(test) {
            return Ok(());
        }
        let data = paths::data_dir();
        let config = paths::config_dir();
        paths::ensure_dir(&data);
        paths::ensure_dir(&config);
        let path = data.join("flint.db");
        open_at_locked(state, &path, &data, &config)
    })
}

/// Open a specific database file. JSON import looks in the parent directory.
#[cfg(test)]
pub fn open_path(path: &Path) -> rusqlite::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    paths::ensure_dir(dir);
    mutate_state(|state| open_at_locked(state, path, dir, dir))
}

fn open_at_locked(
    state: &mut Option<Db>,
    path: &Path,
    data_dir: &Path,
    config_dir: &Path,
) -> rusqlite::Result<()> {
    *state = None;
    let mut conn = Connection::open(path)?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(5_000));
    let _ = conn.pragma_update(None, "foreign_keys", "ON");
    {
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA)?;
        if meta_get(&tx, "schema_version")?.is_none() {
            meta_set(&tx, "schema_version", SCHEMA_VERSION)?;
        }
        if meta_get(&tx, "imported")?.as_deref() != Some("1") {
            import_all(&tx, data_dir, config_dir)?;
            seed_defaults(&tx, config_dir)?;
            trim_clips_conn(&tx)?;
            meta_set(&tx, "imported", "1")?;
        }
        tx.commit()?;
    }
    paths::tighten_private_file(path);
    *state = Some(Db { conn });
    retire_all(data_dir, config_dir);
    Ok(())
}

fn with_conn<T>(f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Option<T> {
    mutate_state(|state| {
        if state.is_none() {
            if cfg!(test) {
                return None;
            }
            let data = paths::data_dir();
            let config = paths::config_dir();
            paths::ensure_dir(&data);
            paths::ensure_dir(&config);
            let path = data.join("flint.db");
            if open_at_locked(state, &path, &data, &config).is_err() {
                return None;
            }
        }
        let conn = &state.as_ref()?.conn;
        f(conn).ok()
    })
}

fn meta_get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM meta WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
}

fn meta_set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn bak_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn retire(path: &Path) {
    if !path.exists() {
        return;
    }
    let bak = bak_path(path);
    if bak.exists() {
        return;
    }
    let _ = fs::rename(path, bak);
}

fn retire_all(data_dir: &Path, config_dir: &Path) {
    for name in [
        "clipboard.json",
        "notes.json",
        "usage.json",
        "favorites.json",
        "calc-history.json",
    ] {
        retire(&data_dir.join(name));
    }
    for name in [
        "snippets.json",
        "aliases.json",
        "quicklinks.json",
        "layouts.json",
        "quit-keep.json",
    ] {
        retire(&config_dir.join(name));
    }
}

fn import_all(conn: &Connection, data_dir: &Path, config_dir: &Path) -> rusqlite::Result<()> {
    import_clips(conn, &data_dir.join("clipboard.json"))?;
    import_notes(conn, &data_dir.join("notes.json"))?;
    import_usage(conn, &data_dir.join("usage.json"))?;
    import_favorites(conn, &data_dir.join("favorites.json"))?;
    import_calc(conn, &data_dir.join("calc-history.json"))?;
    import_snippets(conn, &config_dir.join("snippets.json"))?;
    import_aliases(conn, &config_dir.join("aliases.json"))?;
    import_quicklinks(conn, &config_dir.join("quicklinks.json"))?;
    import_layouts(conn, &config_dir.join("layouts.json"))?;
    import_quit_keep(conn, &config_dir.join("quit-keep.json"))?;
    Ok(())
}

fn json_present(path: &Path) -> bool {
    path.exists() || bak_path(path).exists()
}

fn seed_defaults(conn: &Connection, config_dir: &Path) -> rusqlite::Result<()> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM snippets", [], |r| r.get(0))?;
    if n == 0 && !json_present(&config_dir.join("snippets.json")) {
        for snippet in crate::snippets::default_snippets() {
            snippet_upsert_conn(conn, &snippet)?;
        }
    }
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM quicklinks", [], |r| r.get(0))?;
    if n == 0 && !json_present(&config_dir.join("quicklinks.json")) {
        for link in crate::quicklinks::default_links() {
            quicklink_upsert_conn(conn, &link)?;
        }
    }
    Ok(())
}

fn import_clips(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    let entries = if let Some(store) = read_json::<clipboard::Store>(path) {
        store.entries
    } else if let Some(entries) = read_json::<Vec<Clip>>(path) {
        entries
    } else {
        return Ok(());
    };
    for entry in entries {
        clip_upsert_conn(conn, &entry)?;
    }
    Ok(())
}

fn import_notes(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    let Some(notes) = read_json::<Vec<Note>>(path) else {
        return Ok(());
    };
    for note in notes {
        note_upsert_conn(conn, &note)?;
    }
    Ok(())
}

fn import_usage(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    if let Some(map) = read_json::<usage::Map>(path) {
        for (id, rec) in map {
            usage_put_conn(conn, &id, rec.count, rec.last)?;
        }
        return Ok(());
    }
    if let Some(old) = read_json::<HashMap<String, u32>>(path) {
        for (id, count) in old {
            usage_put_conn(conn, &id, count, 0)?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct FavoritesFile {
    #[serde(default)]
    ids: Vec<String>,
}

fn import_favorites(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let ids = if let Ok(file) = serde_json::from_str::<FavoritesFile>(&text) {
        file.ids
    } else {
        serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
    };
    for (i, id) in ids.into_iter().enumerate() {
        if id.is_empty() {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO favorites (id, pos) VALUES (?1, ?2)",
            params![id, i as i64],
        )?;
    }
    Ok(())
}

fn import_calc(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    let Some(history) = read_json::<calc::History>(path) else {
        return Ok(());
    };
    for entry in history.entries.into_iter().rev() {
        calc_insert_conn(conn, &entry.expr, &entry.result, entry.at)?;
    }
    Ok(())
}

fn import_snippets(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    let Some(snippets) = read_json::<Vec<Snippet>>(path) else {
        return Ok(());
    };
    for snippet in snippets {
        snippet_upsert_conn(conn, &snippet)?;
    }
    Ok(())
}

fn import_aliases(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    let Some(map) = read_json::<HashMap<String, String>>(path) else {
        return Ok(());
    };
    for (id, name) in map {
        alias_set_conn(conn, &id, Some(&name))?;
    }
    Ok(())
}

fn import_quicklinks(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    let Some(links) = read_json::<Vec<Link>>(path) else {
        return Ok(());
    };
    for link in links {
        quicklink_upsert_conn(conn, &link)?;
    }
    Ok(())
}

fn import_layouts(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    let Some(layouts) = read_json::<Vec<NamedLayout>>(path) else {
        return Ok(());
    };
    for layout in layouts {
        layout_upsert_conn(conn, &layout)?;
    }
    Ok(())
}

fn import_quit_keep(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    let Some(classes) = read_json::<Vec<String>>(path) else {
        return Ok(());
    };
    for class in classes {
        let class = class.trim();
        if class.is_empty() {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO quit_keep (class) VALUES (?1)",
            params![class],
        )?;
    }
    Ok(())
}

fn clip_upsert_conn(conn: &Connection, entry: &Clip) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO clips (id, text, copied_at, pinned, label) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            text = excluded.text,
            copied_at = excluded.copied_at,
            pinned = excluded.pinned,
            label = excluded.label",
        params![
            entry.id,
            entry.text,
            entry.copied_at as i64,
            i64::from(entry.pinned),
            entry.label
        ],
    )?;
    Ok(())
}

fn trim_clips_conn(conn: &Connection) -> rusqlite::Result<()> {
    let mut entries = clips_load_conn(conn)?;
    let dropped = clipboard::sort_and_trim_entries(&mut entries);
    for id in dropped {
        conn.execute("DELETE FROM clips WHERE id = ?1", params![id])?;
    }
    Ok(())
}

fn clips_load_conn(conn: &Connection) -> rusqlite::Result<Vec<Clip>> {
    let mut stmt = conn.prepare(
        "SELECT id, text, copied_at, pinned, label FROM clips
         ORDER BY pinned DESC, copied_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Clip {
            id: row.get(0)?,
            text: row.get(1)?,
            copied_at: row.get::<_, i64>(2)? as u64,
            pinned: row.get::<_, i64>(3)? != 0,
            label: row.get(4)?,
        })
    })?;
    rows.collect()
}

fn note_upsert_conn(conn: &Connection, note: &Note) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO notes (id, title, body, pinned, updated) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            body = excluded.body,
            pinned = excluded.pinned,
            updated = excluded.updated",
        params![
            note.id,
            note.title,
            note.body,
            i64::from(note.pinned),
            note.updated as i64
        ],
    )?;
    Ok(())
}

fn snippet_upsert_conn(conn: &Connection, snippet: &Snippet) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO snippets (keyword, title, text, increment) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(keyword) DO UPDATE SET
            title = excluded.title,
            text = excluded.text,
            increment = excluded.increment",
        params![
            snippet.keyword,
            snippet.title,
            snippet.text,
            snippet.increment as i64
        ],
    )?;
    Ok(())
}

fn usage_put_conn(conn: &Connection, id: &str, count: u32, last: u64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO usage (id, count, last) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET count = excluded.count, last = excluded.last",
        params![id, count as i64, last as i64],
    )?;
    Ok(())
}

fn alias_set_conn(conn: &Connection, id: &str, alias: Option<&str>) -> rusqlite::Result<()> {
    match alias {
        None | Some("") => {
            conn.execute("DELETE FROM aliases WHERE id = ?1", params![id])?;
        }
        Some(alias) => {
            conn.execute(
                "INSERT INTO aliases (id, alias) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET alias = excluded.alias",
                params![id, alias],
            )?;
        }
    }
    Ok(())
}

fn calc_insert_conn(conn: &Connection, expr: &str, result: &str, at: u64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM calc_history WHERE expr = ?1 AND result = ?2",
        params![expr, result],
    )?;
    conn.execute(
        "INSERT INTO calc_history (expr, result, at) VALUES (?1, ?2, ?3)",
        params![expr, result, at as i64],
    )?;
    let extra: i64 = conn.query_row("SELECT COUNT(*) FROM calc_history", [], |row| row.get(0))?;
    if extra > calc::MAX_HISTORY as i64 {
        conn.execute(
            "DELETE FROM calc_history WHERE id IN (
                SELECT id FROM (
                    SELECT id FROM calc_history ORDER BY at ASC, id ASC LIMIT ?1
                )
            )",
            params![extra - calc::MAX_HISTORY as i64],
        )?;
    }
    Ok(())
}

fn quicklink_upsert_conn(conn: &Connection, link: &Link) -> rusqlite::Result<()> {
    let tags = serde_json::to_string(&link.tags).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO quicklinks (name, title, target, tags) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(name) DO UPDATE SET
            title = excluded.title,
            target = excluded.target,
            tags = excluded.tags",
        params![link.name, link.title, link.target, tags],
    )?;
    Ok(())
}

fn layout_upsert_conn(conn: &Connection, layout: &NamedLayout) -> rusqlite::Result<()> {
    let slots = serde_json::to_string(&layout.slots).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO layouts (name, slots) VALUES (?1, ?2)
         ON CONFLICT(name) DO UPDATE SET slots = excluded.slots",
        params![layout.name, slots],
    )?;
    Ok(())
}

pub fn clips_load() -> Option<Vec<Clip>> {
    with_conn(clips_load_conn)
}

pub fn clip_upsert(entry: &Clip) -> Option<()> {
    with_conn(|conn| clip_upsert_conn(conn, entry))
}

pub fn clip_delete(id: &str) -> Option<()> {
    with_conn(|conn| {
        conn.execute("DELETE FROM clips WHERE id = ?1", params![id])?;
        Ok(())
    })
}

pub fn notes_load() -> Option<Vec<Note>> {
    with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, title, body, pinned, updated FROM notes
             ORDER BY pinned DESC, updated DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Note {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                pinned: row.get::<_, i64>(3)? != 0,
                updated: row.get::<_, i64>(4)? as u64,
            })
        })?;
        rows.collect()
    })
}

pub fn note_get(id: &str) -> Option<Note> {
    with_conn(|conn| {
        conn.query_row(
            "SELECT id, title, body, pinned, updated FROM notes WHERE id = ?1",
            params![id],
            |row| {
                Ok(Note {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    pinned: row.get::<_, i64>(3)? != 0,
                    updated: row.get::<_, i64>(4)? as u64,
                })
            },
        )
        .optional()
    })
    .flatten()
}

pub fn note_upsert(note: &Note) -> Option<()> {
    with_conn(|conn| note_upsert_conn(conn, note))
}

pub fn snippets_load() -> Option<Vec<Snippet>> {
    with_conn(|conn| {
        let mut stmt =
            conn.prepare("SELECT keyword, title, text, increment FROM snippets ORDER BY keyword")?;
        let rows = stmt.query_map([], |row| {
            Ok(Snippet {
                keyword: row.get(0)?,
                title: row.get(1)?,
                text: row.get(2)?,
                increment: row.get::<_, i64>(3)? as u32,
            })
        })?;
        rows.collect()
    })
}

pub fn snippet_get(keyword: &str) -> Option<Snippet> {
    with_conn(|conn| {
        conn.query_row(
            "SELECT keyword, title, text, increment FROM snippets WHERE keyword = ?1",
            params![keyword],
            |row| {
                Ok(Snippet {
                    keyword: row.get(0)?,
                    title: row.get(1)?,
                    text: row.get(2)?,
                    increment: row.get::<_, i64>(3)? as u32,
                })
            },
        )
        .optional()
    })
    .flatten()
}

pub fn snippet_upsert(snippet: &Snippet) -> Option<()> {
    with_conn(|conn| snippet_upsert_conn(conn, snippet))
}

pub fn snippet_set_increment(keyword: &str, increment: u32) -> Option<()> {
    with_conn(|conn| {
        conn.execute(
            "UPDATE snippets SET increment = ?1 WHERE keyword = ?2",
            params![increment as i64, keyword],
        )?;
        Ok(())
    })
}

pub fn aliases_load() -> Option<HashMap<String, String>> {
    with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT id, alias FROM aliases")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    })
}

pub fn alias_set(id: &str, alias: Option<&str>) -> Option<()> {
    with_conn(|conn| alias_set_conn(conn, id, alias))
}

pub fn favorites_load() -> Option<Vec<String>> {
    with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM favorites ORDER BY pos ASC")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    })
}

pub fn favorite_add(id: &str) -> Option<()> {
    with_conn(|conn| {
        conn.execute("DELETE FROM favorites WHERE id = ?1", params![id])?;
        conn.execute(
            "INSERT INTO favorites (id, pos)
             VALUES (?1, COALESCE((SELECT MIN(pos) FROM favorites), 0) - 1)",
            params![id],
        )?;
        Ok(())
    })
}

pub fn favorite_remove(id: &str) -> Option<()> {
    with_conn(|conn| {
        conn.execute("DELETE FROM favorites WHERE id = ?1", params![id])?;
        Ok(())
    })
}

pub fn calc_load() -> Option<Vec<calc::Entry>> {
    with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT expr, result, at FROM calc_history ORDER BY at DESC, id DESC LIMIT 50",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(calc::Entry {
                expr: row.get(0)?,
                result: row.get(1)?,
                at: row.get::<_, i64>(2)? as u64,
            })
        })?;
        rows.collect()
    })
}

pub fn calc_push(expr: &str, result: &str) -> Option<()> {
    with_conn(|conn| calc_insert_conn(conn, expr, result, now_secs()))
}

pub fn usage_load() -> Option<usage::Map> {
    with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT id, count, last FROM usage")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Record {
                    count: row.get::<_, i64>(1)? as u32,
                    last: row.get::<_, i64>(2)? as u64,
                },
            ))
        })?;
        rows.collect()
    })
}

pub fn usage_bump(id: &str, now: u64) -> Option<()> {
    with_conn(|conn| {
        conn.execute(
            "INSERT INTO usage (id, count, last) VALUES (?1, 1, ?2)
             ON CONFLICT(id) DO UPDATE SET
                count = usage.count + 1,
                last = excluded.last",
            params![id, now as i64],
        )?;
        Ok(())
    })
}

pub fn choices_load() -> Option<crate::choices::Map> {
    with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT query, item_id, count, last FROM choices")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                Record {
                    count: row.get::<_, i64>(2)? as u32,
                    last: row.get::<_, i64>(3)? as u64,
                },
            ))
        })?;
        let mut map = crate::choices::Map::new();
        for row in rows {
            let (query, item_id, rec) = row?;
            map.entry(query).or_default().insert(item_id, rec);
        }
        Ok(map)
    })
}

pub fn choices_record(query: &str, item_id: &str, now: u64, prefixes: &[String]) -> Option<()> {
    with_conn(|conn| {
        choices_bump_full(conn, query, item_id, now)?;
        for prefix in prefixes {
            if prefix.is_empty() || prefix == query {
                continue;
            }
            choices_bump_prefix(conn, prefix, item_id, now)?;
        }
        Ok(())
    })
}

fn choices_bump_full(
    conn: &Connection,
    query: &str,
    item_id: &str,
    now: u64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO choices (query, item_id, count, last) VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(query, item_id) DO UPDATE SET
            count = choices.count + 1,
            last = excluded.last",
        params![query, item_id, now as i64],
    )?;
    Ok(())
}

fn choices_bump_prefix(
    conn: &Connection,
    query: &str,
    item_id: &str,
    now: u64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO choices (query, item_id, count, last) VALUES (?1, ?2, 0, ?3)
         ON CONFLICT(query, item_id) DO UPDATE SET last = excluded.last",
        params![query, item_id, now as i64],
    )?;
    Ok(())
}

pub fn choices_clear() -> Option<()> {
    with_conn(|conn| {
        conn.execute("DELETE FROM choices", [])?;
        Ok(())
    })
}

pub fn quicklinks_load() -> Option<Vec<Link>> {
    with_conn(|conn| {
        let mut stmt =
            conn.prepare("SELECT name, title, target, tags FROM quicklinks ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            let tags: String = row.get(3)?;
            Ok(Link {
                name: row.get(0)?,
                title: row.get(1)?,
                target: row.get(2)?,
                tags: serde_json::from_str(&tags).unwrap_or_default(),
            })
        })?;
        rows.collect()
    })
}

pub fn quicklink_upsert(link: &Link) -> Option<()> {
    with_conn(|conn| quicklink_upsert_conn(conn, link))
}

pub fn layouts_load() -> Option<Vec<NamedLayout>> {
    with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT name, slots FROM layouts ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            let slots: String = row.get(1)?;
            let slots: Vec<Slot> = serde_json::from_str(&slots).unwrap_or_default();
            Ok(NamedLayout {
                name: row.get(0)?,
                slots,
            })
        })?;
        rows.collect()
    })
}

pub fn layout_upsert(layout: &NamedLayout) -> Option<()> {
    with_conn(|conn| layout_upsert_conn(conn, layout))
}

pub fn quit_keep_load() -> Option<Vec<String>> {
    with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT class FROM quit_keep ORDER BY class")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect()
    })
}

fn dictation_insert_conn(conn: &Connection, text: &str, at: u64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO dictation (text, at) VALUES (?1, ?2)",
        params![text, at as i64],
    )?;
    let extra: i64 = conn.query_row("SELECT COUNT(*) FROM dictation", [], |row| row.get(0))?;
    if extra > MAX_DICTATION {
        conn.execute(
            "DELETE FROM dictation WHERE id IN (
                SELECT id FROM (
                    SELECT id FROM dictation ORDER BY at ASC, id ASC LIMIT ?1
                )
            )",
            params![extra - MAX_DICTATION],
        )?;
    }
    Ok(())
}

pub fn dictation_push(text: &str) -> Option<()> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    with_conn(|conn| dictation_insert_conn(conn, text, now_secs()))
}

pub fn dictation_load() -> Option<Vec<Dictation>> {
    with_conn(|conn| {
        let mut stmt =
            conn.prepare("SELECT id, text, at FROM dictation ORDER BY at DESC, id DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![MAX_DICTATION], |row| {
            Ok(Dictation {
                id: row.get(0)?,
                text: row.get(1)?,
                at: row.get::<_, i64>(2)? as u64,
            })
        })?;
        rows.collect()
    })
}

pub fn dictation_last() -> Option<String> {
    with_conn(|conn| {
        conn.query_row(
            "SELECT text FROM dictation ORDER BY at DESC, id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
    })
    .flatten()
}

pub fn dictation_get(id: i64) -> Option<Dictation> {
    with_conn(|conn| {
        conn.query_row(
            "SELECT id, text, at FROM dictation WHERE id = ?1",
            params![id],
            |row| {
                Ok(Dictation {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    at: row.get::<_, i64>(2)? as u64,
                })
            },
        )
        .optional()
    })
    .flatten()
}

fn thread_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Thread> {
    Ok(Thread {
        id: row.get(0)?,
        title: row.get(1)?,
        created: row.get::<_, i64>(2)? as u64,
        updated: row.get::<_, i64>(3)? as u64,
    })
}

pub fn ai_thread_insert(thread: &Thread) -> Option<()> {
    with_conn(|conn| {
        conn.execute(
            "INSERT INTO ai_threads (id, title, created, updated) VALUES (?1, ?2, ?3, ?4)",
            params![
                thread.id,
                thread.title,
                thread.created as i64,
                thread.updated as i64
            ],
        )?;
        Ok(())
    })
}

pub fn ai_thread_touch(id: &str, title: Option<&str>, updated: u64) -> Option<()> {
    with_conn(|conn| {
        if let Some(title) = title.filter(|t| !t.is_empty()) {
            conn.execute(
                "UPDATE ai_threads SET title = ?1, updated = ?2 WHERE id = ?3",
                params![title, updated as i64, id],
            )?;
        } else {
            conn.execute(
                "UPDATE ai_threads SET updated = ?1 WHERE id = ?2",
                params![updated as i64, id],
            )?;
        }
        Ok(())
    })
}

pub fn ai_thread_get(id: &str) -> Option<Thread> {
    with_conn(|conn| {
        conn.query_row(
            "SELECT id, title, created, updated FROM ai_threads WHERE id = ?1",
            params![id],
            thread_from_row,
        )
        .optional()
    })
    .flatten()
}

pub fn ai_threads_search(query: &str) -> Option<Vec<Thread>> {
    with_conn(|conn| {
        let q = query.trim();
        if q.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT id, title, created, updated FROM ai_threads
                 ORDER BY updated DESC LIMIT 20",
            )?;
            let rows = stmt.query_map([], thread_from_row)?;
            rows.collect()
        } else {
            let pat = format!("%{q}%");
            let mut stmt = conn.prepare(
                "SELECT DISTINCT t.id, t.title, t.created, t.updated
                 FROM ai_threads t
                 LEFT JOIN ai_messages m ON m.thread_id = t.id
                 WHERE t.title LIKE ?1 OR m.text LIKE ?1
                 ORDER BY t.updated DESC LIMIT 20",
            )?;
            let rows = stmt.query_map(params![pat], thread_from_row)?;
            rows.collect()
        }
    })
}

pub fn ai_message_insert(msg: &ChatMessage) -> Option<()> {
    with_conn(|conn| {
        conn.execute(
            "INSERT INTO ai_messages (id, thread_id, role, text, at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![msg.id, msg.thread_id, msg.role, msg.text, msg.at as i64],
        )?;
        Ok(())
    })
}

pub fn ai_messages(thread_id: &str) -> Option<Vec<ChatMessage>> {
    with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, thread_id, role, text, at FROM ai_messages
             WHERE thread_id = ?1 ORDER BY at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![thread_id], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                role: row.get(2)?,
                text: row.get(3)?,
                at: row.get::<_, i64>(4)? as u64,
            })
        })?;
        rows.collect()
    })
}

pub fn memory_remember(text: &str) -> Option<Fact> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    with_conn(|conn| {
        if let Some(existing) = conn
            .query_row(
                "SELECT id, text, at FROM memory WHERE text = ?1",
                params![text],
                |row| {
                    Ok(Fact {
                        id: row.get(0)?,
                        text: row.get(1)?,
                        at: row.get::<_, i64>(2)? as u64,
                    })
                },
            )
            .optional()?
        {
            let now = now_secs();
            conn.execute(
                "UPDATE memory SET at = ?1 WHERE id = ?2",
                params![now as i64, existing.id],
            )?;
            return Ok(Fact {
                at: now,
                ..existing
            });
        }
        let now = now_secs();
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let fact = Fact {
            id: format!("{ns:x}"),
            text: text.to_string(),
            at: now,
        };
        conn.execute(
            "INSERT INTO memory (id, text, at) VALUES (?1, ?2, ?3)",
            params![fact.id, fact.text, fact.at as i64],
        )?;
        let extra: i64 = conn.query_row("SELECT COUNT(*) FROM memory", [], |row| row.get(0))?;
        if extra > MAX_MEMORY {
            conn.execute(
                "DELETE FROM memory WHERE id IN (
                    SELECT id FROM (
                        SELECT id FROM memory ORDER BY at ASC, id ASC LIMIT ?1
                    )
                )",
                params![extra - MAX_MEMORY],
            )?;
        }
        Ok(fact)
    })
}

pub fn memory_forget(query: &str) -> Option<usize> {
    let query = query.trim();
    if query.is_empty() {
        return Some(0);
    }
    with_conn(|conn| {
        let pat = format!("%{query}%");
        let n = conn.execute("DELETE FROM memory WHERE text LIKE ?1", params![pat])?;
        Ok(n)
    })
}

pub fn memory_load() -> Option<Vec<Fact>> {
    with_conn(|conn| {
        let mut stmt =
            conn.prepare("SELECT id, text, at FROM memory ORDER BY at DESC, id DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![MAX_MEMORY], |row| {
            Ok(Fact {
                id: row.get(0)?,
                text: row.get(1)?,
                at: row.get::<_, i64>(2)? as u64,
            })
        })?;
        rows.collect()
    })
}

#[cfg(test)]
pub(crate) fn reset() {
    mutate_state(|state| *state = None);
}

#[cfg(test)]
pub(crate) fn with_temp<F: FnOnce(&Path)>(f: F) {
    reset();
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("flint-db-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::create_dir_all(&dir);
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            reset();
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(dir.clone());
    f(&dir);
}

#[cfg(test)]
mod tests {
    use super::{clips_load, open_path, usage_load, with_temp};
    use crate::clipboard::Store;
    use crate::usage;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn import_clipboard_json_and_bump_usage() {
        with_temp(|dir| {
            let json = r#"{"entries":[{"id":"abc","text":"hello from json","copied_at":42}]}"#;
            fs::write(dir.join("clipboard.json"), json).expect("write clipboard json");
            open_path(&dir.join("flint.db")).expect("import");
            let store = Store::load();
            assert_eq!(store.entries.len(), 1);
            assert_eq!(store.entries[0].text, "hello from json");
            assert!(dir.join("clipboard.json.bak").exists());
            assert!(!dir.join("clipboard.json").exists());

            let db_path = dir.join("flint.db");
            let before = fs::metadata(&db_path).expect("meta").len();
            usage::bump("app:firefox.desktop");
            let map = usage::load();
            let rec = map.get("app:firefox.desktop").expect("bumped");
            assert_eq!(rec.count, 1);
            usage::bump("app:firefox.desktop");
            let map = usage_load().expect("usage");
            assert_eq!(map.get("app:firefox.desktop").expect("row").count, 2);
            let after = fs::metadata(&db_path).expect("meta").len();
            assert!(
                after < before + 64 * 1024,
                "incremental bump must not rewrite a dump ({before} -> {after})"
            );

            let mode = fs::metadata(&db_path).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "flint.db must be mode 600");
        });
    }

    #[test]
    fn open_twice_is_idempotent() {
        with_temp(|dir| {
            fs::write(
                dir.join("clipboard.json"),
                r#"{"entries":[{"id":"one","text":"first","copied_at":1}]}"#,
            )
            .expect("json");
            open_path(&dir.join("flint.db")).expect("first open");
            assert_eq!(clips_load().expect("clips").len(), 1);
            fs::write(
                dir.join("clipboard.json"),
                r#"{"entries":[{"id":"two","text":"should not import","copied_at":2}]}"#,
            )
            .expect("second json");
            open_path(&dir.join("flint.db")).expect("second open");
            let clips = clips_load().expect("clips");
            assert_eq!(clips.len(), 1);
            assert_eq!(clips[0].text, "first");
            assert!(dir.join("clipboard.json.bak").exists());
        });
    }

    #[test]
    fn dictation_caps_at_100_without_bumping_schema_v1() {
        with_temp(|dir| {
            open_path(&dir.join("flint.db")).expect("open");
            assert_eq!(clips_load().expect("clips").len(), 0);
            for i in 0..105 {
                super::dictation_push(&format!("utterance {i}")).expect("push");
            }
            let rows = super::dictation_load().expect("load");
            assert_eq!(rows.len(), 100);
            assert_eq!(rows[0].text, "utterance 104");
            assert_eq!(rows[99].text, "utterance 5");
            assert_eq!(super::dictation_last().as_deref(), Some("utterance 104"));
            let got = super::dictation_get(rows[0].id).expect("get");
            assert_eq!(got.text, "utterance 104");
            crate::notes::create("still v1");
            assert_eq!(super::notes_load().expect("notes").len(), 1);
            crate::memory::remember("wave5 fact").expect("memory");
            assert_eq!(super::memory_load().expect("mem").len(), 1);
            let thread = crate::ai::Thread {
                id: "t1".into(),
                title: "hello".into(),
                created: 1,
                updated: 2,
            };
            super::ai_thread_insert(&thread).expect("thread");
            super::ai_message_insert(&crate::ai::ChatMessage {
                id: "m1".into(),
                thread_id: "t1".into(),
                role: "user".into(),
                text: "hi".into(),
                at: 3,
            })
            .expect("msg");
            let found = super::ai_threads_search("hi").expect("search");
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].title, "hello");
            assert_eq!(
                meta_get_for_test().as_deref(),
                Some("1"),
                "wave 5 tables must not bump schema_version"
            );
        });
    }

    fn meta_get_for_test() -> Option<String> {
        super::with_conn(|conn| super::meta_get(conn, "schema_version")).flatten()
    }

    #[test]
    fn legacy_json_filenames() {
        assert_eq!(
            crate::snippets::file().file_name().expect("name"),
            "snippets.json"
        );
        assert_eq!(
            crate::quicklinks::file().file_name().expect("name"),
            "quicklinks.json"
        );
        assert_eq!(
            crate::layout::file().file_name().expect("name"),
            "layouts.json"
        );
        assert_eq!(
            crate::quit::file().file_name().expect("name"),
            "quit-keep.json"
        );
    }

    #[test]
    fn choices_roundtrip_in_temp_db() {
        with_temp(|dir| {
            open_path(&dir.join("flint.db")).expect("open");
            let now = 1_800_000_000;
            super::choices_record("sl", "app:slack", now, &["s".into()]).expect("record");
            let map = super::choices_load().expect("load");
            assert_eq!(
                map.get("sl")
                    .expect("sl")
                    .get("app:slack")
                    .expect("row")
                    .count,
                1
            );
            assert_eq!(
                map.get("s")
                    .expect("s")
                    .get("app:slack")
                    .expect("prefix")
                    .count,
                0
            );
            super::choices_clear().expect("clear");
            assert!(super::choices_load().expect("empty").is_empty());
        });
    }
}
