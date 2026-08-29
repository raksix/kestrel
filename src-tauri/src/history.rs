//! Capture history.
//!
//! SQLite rather than ShareX's `History.xml`, because the library has to filter
//! and search across thousands of rows and re-parsing an XML document to do it
//! does not scale. The schema is versioned through SQLite's own `user_version`
//! so an older database upgrades in place instead of being discarded.

use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Bump when the schema changes, and add a matching arm to `migrate`.
const SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    /// Unix seconds. Stored as an integer so ordering is a numeric compare.
    pub created_at: i64,
    pub filename: String,
    pub path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub window_title: Option<String>,
    pub url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub deletion_url: Option<String>,
    pub destination: Option<String>,
    /// Recognised text, so the library can search what a screenshot *said*.
    pub ocr_text: Option<String>,
}

/// The subset of an entry that exists at capture time.
#[derive(Debug, Clone, Default)]
pub struct NewEntry {
    pub filename: String,
    pub path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub window_title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    /// Matched against filename, window title, URL and recognised text.
    pub text: Option<String>,
    pub uploaded_only: bool,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("no history directory is available")]
    NoDirectory,
}

pub type Result<T> = std::result::Result<T, HistoryError>;

pub struct History(pub Mutex<Connection>);

impl History {
    /// Open the on-disk history, falling back to an in-memory database.
    ///
    /// A capture must never fail because history could not be written, so a
    /// broken or unwritable file degrades to an ephemeral database with a
    /// warning rather than taking the app down.
    pub fn open() -> Self {
        match Self::open_at_config_dir() {
            Ok(history) => history,
            Err(err) => {
                tracing::error!(%err, "history unavailable, continuing without persistence");
                let connection = Connection::open_in_memory().expect("in-memory sqlite");
                let _ = migrate(&connection);
                History(Mutex::new(connection))
            }
        }
    }

    fn open_at_config_dir() -> Result<Self> {
        let dir = crate::settings::config_dir().map_err(|_| HistoryError::NoDirectory)?;
        std::fs::create_dir_all(&dir).map_err(|_| HistoryError::NoDirectory)?;
        let connection = Connection::open(dir.join("history.sqlite"))?;
        migrate(&connection)?;
        Ok(History(Mutex::new(connection)))
    }

    #[cfg(test)]
    fn in_memory() -> Self {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        migrate(&connection).expect("schema");
        History(Mutex::new(connection))
    }

    pub fn insert(&self, entry: &NewEntry, created_at: i64) -> Result<i64> {
        let connection = self.0.lock().expect("history mutex poisoned");
        connection.execute(
            "INSERT INTO captures (created_at, filename, path, width, height, window_title)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                created_at,
                entry.filename,
                entry.path,
                entry.width,
                entry.height,
                entry.window_title,
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    /// Attach the result of an upload to an existing capture.
    pub fn record_upload(
        &self,
        id: i64,
        url: &str,
        thumbnail_url: Option<&str>,
        deletion_url: Option<&str>,
        destination: &str,
    ) -> Result<()> {
        let connection = self.0.lock().expect("history mutex poisoned");
        connection.execute(
            "UPDATE captures
                SET url = ?2, thumbnail_url = ?3, deletion_url = ?4, destination = ?5
              WHERE id = ?1",
            params![id, url, thumbnail_url, deletion_url, destination],
        )?;
        Ok(())
    }

    /// Attach recognised text to a capture.
    ///
    /// Test-only until the OCR tool exists to call it. The column and the
    /// search that reads it are real and covered; shipping a public writer with
    /// no caller would just be dead weight.
    #[cfg(test)]
    fn record_ocr(&self, id: i64, text: &str) -> Result<()> {
        let connection = self.0.lock().expect("history mutex poisoned");
        connection.execute(
            "UPDATE captures SET ocr_text = ?2 WHERE id = ?1",
            params![id, text],
        )?;
        Ok(())
    }

    pub fn list(&self, query: &Query) -> Result<Vec<Entry>> {
        let connection = self.0.lock().expect("history mutex poisoned");

        // Built as one statement with bound parameters. Interpolating the
        // search text would be an injection hole in the user's own database.
        let mut sql = String::from(
            "SELECT id, created_at, filename, path, width, height, window_title,
                    url, thumbnail_url, deletion_url, destination, ocr_text
               FROM captures WHERE 1 = 1",
        );
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(text) = query.text.as_deref().filter(|t| !t.trim().is_empty()) {
            sql.push_str(
                " AND (filename LIKE ?1 OR IFNULL(window_title, '') LIKE ?1
                       OR IFNULL(url, '') LIKE ?1 OR IFNULL(ocr_text, '') LIKE ?1)",
            );
            bound.push(Box::new(format!("%{text}%")));
        }
        if query.uploaded_only {
            sql.push_str(" AND url IS NOT NULL");
        }

        sql.push_str(" ORDER BY created_at DESC, id DESC");
        sql.push_str(&format!(
            " LIMIT {} OFFSET {}",
            query.limit.unwrap_or(500).min(2000),
            query.offset.unwrap_or(0)
        ));

        let mut statement = connection.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
        let rows = statement.query_map(refs.as_slice(), row_to_entry)?;
        rows.collect::<rusqlite::Result<Vec<Entry>>>()
            .map_err(Into::into)
    }

    pub fn get(&self, id: i64) -> Result<Option<Entry>> {
        let connection = self.0.lock().expect("history mutex poisoned");
        let entry = connection
            .query_row(
                "SELECT id, created_at, filename, path, width, height, window_title,
                        url, thumbnail_url, deletion_url, destination, ocr_text
                   FROM captures WHERE id = ?1",
                params![id],
                row_to_entry,
            )
            .optional()?;
        Ok(entry)
    }

    /// Forget an entry. The file on disk is the caller's business — deleting
    /// the user's screenshot because they tidied a list would be a surprise.
    pub fn remove(&self, id: i64) -> Result<()> {
        let connection = self.0.lock().expect("history mutex poisoned");
        connection.execute("DELETE FROM captures WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let connection = self.0.lock().expect("history mutex poisoned");
        connection.execute("DELETE FROM captures", [])?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let connection = self.0.lock().expect("history mutex poisoned");
        Ok(connection.query_row("SELECT COUNT(*) FROM captures", [], |row| row.get(0))?)
    }
}

impl Default for History {
    fn default() -> Self {
        Self::open()
    }
}

/// The history row for the most recent capture, so an upload that happens
/// afterwards can attach its URL to the right entry.
#[derive(Default)]
pub struct LastEntryId(pub Mutex<Option<i64>>);

impl LastEntryId {
    pub fn set(&self, id: i64) {
        *self.0.lock().expect("last entry mutex poisoned") = Some(id);
    }

    pub fn get(&self) -> Option<i64> {
        *self.0.lock().expect("last entry mutex poisoned")
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        created_at: row.get(1)?,
        filename: row.get(2)?,
        path: row.get(3)?,
        width: row.get(4)?,
        height: row.get(5)?,
        window_title: row.get(6)?,
        url: row.get(7)?,
        thumbnail_url: row.get(8)?,
        deletion_url: row.get(9)?,
        destination: row.get(10)?,
        ocr_text: row.get(11)?,
    })
}

fn migrate(connection: &Connection) -> Result<()> {
    let version: i32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if version < 1 {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS captures (
                 id            INTEGER PRIMARY KEY AUTOINCREMENT,
                 created_at    INTEGER NOT NULL,
                 filename      TEXT    NOT NULL,
                 path          TEXT,
                 width         INTEGER NOT NULL DEFAULT 0,
                 height        INTEGER NOT NULL DEFAULT 0,
                 window_title  TEXT,
                 url           TEXT,
                 thumbnail_url TEXT,
                 deletion_url  TEXT,
                 destination   TEXT,
                 ocr_text      TEXT
             );
             CREATE INDEX IF NOT EXISTS captures_created_at
                 ON captures (created_at DESC);",
        )?;
    }

    if version != SCHEMA_VERSION {
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> NewEntry {
        NewEntry {
            filename: name.to_string(),
            path: Some(format!("/tmp/{name}")),
            width: 800,
            height: 600,
            window_title: Some("Kestrel".into()),
        }
    }

    #[test]
    fn an_inserted_capture_comes_back() {
        let history = History::in_memory();
        let id = history.insert(&entry("a.png"), 1000).unwrap();

        let found = history.get(id).unwrap().expect("should exist");
        assert_eq!(found.filename, "a.png");
        assert_eq!((found.width, found.height), (800, 600));
        assert_eq!(found.url, None);
    }

    #[test]
    fn entries_come_back_newest_first() {
        let history = History::in_memory();
        history.insert(&entry("old.png"), 100).unwrap();
        history.insert(&entry("new.png"), 900).unwrap();

        let names: Vec<String> = history
            .list(&Query::default())
            .unwrap()
            .into_iter()
            .map(|e| e.filename)
            .collect();
        assert_eq!(names, ["new.png", "old.png"]);
    }

    #[test]
    fn entries_captured_in_the_same_second_still_have_a_stable_order() {
        // Two captures a few hundred milliseconds apart share a timestamp, and
        // a list that reshuffles between renders looks broken.
        let history = History::in_memory();
        let first = history.insert(&entry("first.png"), 500).unwrap();
        let second = history.insert(&entry("second.png"), 500).unwrap();

        let ids: Vec<i64> = history
            .list(&Query::default())
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, [second, first]);
    }

    #[test]
    fn an_upload_result_attaches_to_its_capture() {
        let history = History::in_memory();
        let id = history.insert(&entry("a.png"), 1).unwrap();

        history
            .record_upload(
                id,
                "https://e.com/a.png",
                Some("https://e.com/t.png"),
                Some("https://e.com/d"),
                "Test host",
            )
            .unwrap();

        let found = history.get(id).unwrap().unwrap();
        assert_eq!(found.url.as_deref(), Some("https://e.com/a.png"));
        assert_eq!(found.destination.as_deref(), Some("Test host"));
    }

    #[test]
    fn search_matches_filename_title_url_and_recognised_text() {
        let history = History::in_memory();
        let by_name = history.insert(&entry("invoice.png"), 1).unwrap();

        let mut other = entry("b.png");
        other.window_title = Some("Bank statement".into());
        let by_title = history.insert(&other, 2).unwrap();

        let by_url = history.insert(&entry("c.png"), 3).unwrap();
        history
            .record_upload(by_url, "https://imgur.com/receipt", None, None, "Imgur")
            .unwrap();

        let by_ocr = history.insert(&entry("d.png"), 4).unwrap();
        history.record_ocr(by_ocr, "Toplam tutar 250 TL").unwrap();

        let find = |text: &str| -> Vec<i64> {
            history
                .list(&Query {
                    text: Some(text.into()),
                    ..Default::default()
                })
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect()
        };

        assert_eq!(find("invoice"), [by_name]);
        assert_eq!(find("statement"), [by_title]);
        assert_eq!(find("receipt"), [by_url]);
        assert_eq!(find("tutar"), [by_ocr], "searching what a screenshot said");
    }

    #[test]
    fn a_search_term_with_sql_in_it_is_not_executed() {
        let history = History::in_memory();
        history.insert(&entry("a.png"), 1).unwrap();

        let results = history
            .list(&Query {
                text: Some("'; DROP TABLE captures; --".into()),
                ..Default::default()
            })
            .unwrap();

        assert!(results.is_empty(), "no row matches that text");
        assert_eq!(history.count().unwrap(), 1, "and the table survives");
    }

    #[test]
    fn uploaded_only_hides_captures_that_were_never_sent() {
        let history = History::in_memory();
        history.insert(&entry("local.png"), 1).unwrap();
        let sent = history.insert(&entry("sent.png"), 2).unwrap();
        history
            .record_upload(sent, "https://e.com/x", None, None, "Host")
            .unwrap();

        let results = history
            .list(&Query {
                uploaded_only: true,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, sent);
    }

    #[test]
    fn paging_walks_the_list_without_repeating_or_skipping() {
        let history = History::in_memory();
        for i in 0..10 {
            history.insert(&entry(&format!("{i}.png")), i).unwrap();
        }

        let page = |offset: u32| -> Vec<i64> {
            history
                .list(&Query {
                    limit: Some(4),
                    offset: Some(offset),
                    ..Default::default()
                })
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect()
        };

        let mut seen = page(0);
        seen.extend(page(4));
        seen.extend(page(8));

        assert_eq!(seen.len(), 10);
        let mut unique = seen.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 10, "no row appears on two pages");
    }

    #[test]
    fn a_runaway_limit_is_capped() {
        // A UI bug asking for a million rows should not try to load them.
        let history = History::in_memory();
        for i in 0..5 {
            history.insert(&entry(&format!("{i}.png")), i).unwrap();
        }
        let results = history
            .list(&Query {
                limit: Some(u32::MAX),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn removing_an_entry_leaves_the_others() {
        let history = History::in_memory();
        let first = history.insert(&entry("a.png"), 1).unwrap();
        history.insert(&entry("b.png"), 2).unwrap();

        history.remove(first).unwrap();

        assert_eq!(history.count().unwrap(), 1);
        assert!(history.get(first).unwrap().is_none());
    }

    #[test]
    fn clearing_empties_the_table() {
        let history = History::in_memory();
        history.insert(&entry("a.png"), 1).unwrap();
        history.clear().unwrap();
        assert_eq!(history.count().unwrap(), 0);
    }

    #[test]
    fn migrating_twice_is_harmless() {
        let connection = Connection::open_in_memory().unwrap();
        migrate(&connection).unwrap();
        migrate(&connection).unwrap();

        let version: i32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }
}
