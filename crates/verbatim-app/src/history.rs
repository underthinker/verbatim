//! Dictation history store (ARCHITECTURE.md 4.8, UX.md 7): SQLite holding
//! raw+polished pairs with the target app id and a timestamp. Retention is
//! pruned on write (default 7 days from config); retention `0` means "off" -
//! nothing is written. Clear-all is a single delete + VACUUM.
//!
//! The DB lives under the *data* dir (ENGINEERING.md 5.2) and never leaves the
//! machine (security posture). The connection is behind a `Mutex` so the bus
//! recorder task and the webview query commands can share one handle.

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Serialize;

/// One recorded dictation, for the History surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub app_id: String,
    pub raw: String,
    /// Polished text when polish ran, else `null` (raw-only).
    pub polished: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("history db error: {0}")]
    Db(#[from] rusqlite::Error),
}

pub struct History {
    conn: Mutex<Connection>,
}

impl History {
    /// Open (creating if needed) the history DB at `path` and ensure the schema.
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        let conn = Connection::open(path)?;
        Self::from_conn(conn)
    }

    /// An ephemeral in-memory store - the startup fallback when the on-disk DB
    /// cannot be opened, so a bad data dir degrades history rather than blocking
    /// the app. Lost on exit.
    pub fn open_in_memory() -> Result<Self, HistoryError> {
        Self::from_conn(Connection::open_in_memory()?)
    }

    fn from_conn(conn: Connection) -> Result<Self, HistoryError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                app_id     TEXT    NOT NULL,
                raw        TEXT    NOT NULL,
                polished   TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS entries_created_at ON entries(created_at);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Record a completed dictation, then prune expired rows. Retention `0`
    /// means history is off: nothing is written. A poisoned lock (a prior panic
    /// while holding it) is treated as a lost write, not a crash.
    pub fn record(
        &self,
        app_id: &str,
        raw: &str,
        polished: Option<&str>,
        retention_days: u32,
    ) -> Result<(), HistoryError> {
        if retention_days == 0 {
            return Ok(());
        }
        let Ok(conn) = self.conn.lock() else {
            tracing::warn!("history lock poisoned; skipping record");
            return Ok(());
        };
        conn.execute(
            "INSERT INTO entries (app_id, raw, polished, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![app_id, raw, polished, now()],
        )?;
        prune(&conn, retention_days)?;
        Ok(())
    }

    /// The most recent `limit` entries, newest first.
    pub fn list(&self, limit: u32) -> Result<Vec<HistoryEntry>, HistoryError> {
        let Ok(conn) = self.conn.lock() else {
            return Ok(Vec::new());
        };
        let mut stmt = conn.prepare(
            "SELECT id, app_id, raw, polished, created_at
             FROM entries ORDER BY created_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                app_id: row.get(1)?,
                raw: row.get(2)?,
                polished: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Delete every entry and reclaim the space (VACUUM).
    pub fn clear(&self) -> Result<(), HistoryError> {
        let Ok(conn) = self.conn.lock() else {
            return Ok(());
        };
        conn.execute("DELETE FROM entries", [])?;
        conn.execute_batch("VACUUM")?;
        Ok(())
    }
}

/// Delete rows older than `retention_days`.
fn prune(conn: &Connection, retention_days: u32) -> Result<(), rusqlite::Error> {
    let cutoff = now() - i64::from(retention_days) * 86_400;
    conn.execute("DELETE FROM entries WHERE created_at < ?1", [cutoff])?;
    Ok(())
}

/// Current unix time in seconds. Before the epoch is impossible on a sane
/// clock; clamp to 0 rather than panic.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn history() -> History {
        History::from_conn(Connection::open_in_memory().expect("open mem")).expect("schema")
    }

    /// Insert a row directly at a chosen timestamp (to exercise pruning).
    fn insert_at(h: &History, app: &str, raw: &str, created_at: i64) {
        let conn = h.conn.lock().expect("lock");
        conn.execute(
            "INSERT INTO entries (app_id, raw, polished, created_at) VALUES (?1, ?2, NULL, ?3)",
            rusqlite::params![app, raw, created_at],
        )
        .expect("insert");
    }

    #[test]
    fn record_then_list_newest_first() {
        let h = history();
        h.record("com.app.a", "first", None, 7).expect("rec1");
        h.record("com.app.b", "second", Some("Second."), 7)
            .expect("rec2");
        let list = h.list(10).expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].raw, "second");
        assert_eq!(list[0].polished.as_deref(), Some("Second."));
        assert_eq!(list[1].raw, "first");
        assert_eq!(list[1].polished, None);
    }

    #[test]
    fn retention_off_writes_nothing() {
        let h = history();
        h.record("com.app", "ignored", None, 0).expect("rec");
        assert!(h.list(10).expect("list").is_empty());
    }

    #[test]
    fn record_prunes_expired_rows() {
        let h = history();
        // An old row from 10 days ago; a 7-day retention record prunes it.
        insert_at(&h, "com.app", "stale", now() - 10 * 86_400);
        h.record("com.app", "fresh", None, 7).expect("rec");
        let list = h.list(10).expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].raw, "fresh");
    }

    #[test]
    fn clear_removes_everything() {
        let h = history();
        h.record("com.app", "one", None, 7).expect("rec");
        h.clear().expect("clear");
        assert!(h.list(10).expect("list").is_empty());
    }
}
