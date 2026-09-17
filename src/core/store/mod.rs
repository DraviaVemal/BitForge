use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::core::workspace::{FORGE_SOURCE_DIR, STATS_DB_FILE};

#[derive(Debug, Clone, Serialize)]
pub struct BuildRecord {
    pub id: i64,
    pub target: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub elapsed_secs: Option<i64>,
    pub report_dir: Option<String>,
    pub log_path: Option<String>,
}

pub fn stats_db_path(root: &Path) -> PathBuf {
    root.join(FORGE_SOURCE_DIR).join(STATS_DB_FILE)
}

pub fn ensure_database(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let connection =
        Connection::open(path).with_context(|| format!("failed to create {}", path.display()))?;
    connection
        .execute_batch("PRAGMA user_version = 1;")
        .with_context(|| format!("failed to initialize {}", path.display()))?;
    Ok(())
}

pub fn init_schema(db_path: &Path) -> Result<()> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS builds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                target TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                elapsed_secs INTEGER,
                report_dir TEXT,
                log_path TEXT
            );
            CREATE TABLE IF NOT EXISTS metadata_cache (
                kind TEXT NOT NULL,
                signature TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (kind, signature)
            );
            CREATE TABLE IF NOT EXISTS revisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event TEXT NOT NULL,
                commit_sha TEXT,
                branch TEXT,
                dirty INTEGER NOT NULL,
                changed_count INTEGER NOT NULL,
                changed_files TEXT NOT NULL,
                recorded_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS activities (
                id INTEGER PRIMARY KEY,
                label TEXT NOT NULL,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                log TEXT
            );",
        )
        .context("failed to create builds table")?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct RevisionRecord {
    pub id: i64,
    pub event: String,
    pub commit_sha: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub changed_count: i64,
    pub changed_files: String,
    pub recorded_at: i64,
}

#[allow(clippy::too_many_arguments)]
pub fn record_revision(
    root: &Path,
    event: &str,
    commit_sha: Option<&str>,
    branch: Option<&str>,
    dirty: bool,
    changed_count: i64,
    changed_files: &str,
    recorded_at: i64,
) -> Result<()> {
    let connection = Connection::open(stats_db_path(root))?;
    connection.execute(
        "INSERT INTO revisions
            (event, commit_sha, branch, dirty, changed_count, changed_files, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![event, commit_sha, branch, dirty as i64, changed_count, changed_files, recorded_at],
    )?;
    Ok(())
}

pub fn list_revisions(root: &Path, limit: i64) -> Result<Vec<RevisionRecord>> {
    let connection = Connection::open(stats_db_path(root))?;
    let mut statement = connection.prepare(
        "SELECT id, event, commit_sha, branch, dirty, changed_count, changed_files, recorded_at
         FROM revisions ORDER BY id DESC LIMIT ?1",
    )?;
    let records = statement
        .query_map(params![limit], |row| {
            Ok(RevisionRecord {
                id: row.get(0)?,
                event: row.get(1)?,
                commit_sha: row.get(2)?,
                branch: row.get(3)?,
                dirty: row.get::<_, i64>(4)? != 0,
                changed_count: row.get(5)?,
                changed_files: row.get(6)?,
                recorded_at: row.get(7)?,
            })
        })?
        .filter_map(Result::ok)
        .collect();
    Ok(records)
}

pub fn cached_metadata(root: &Path, kind: &str, signature: &str) -> Result<Option<String>> {
    let connection = Connection::open(stats_db_path(root))?;
    let mut statement = connection
        .prepare("SELECT payload FROM metadata_cache WHERE kind = ?1 AND signature = ?2")?;
    let mut rows = statement.query_map(params![kind, signature], |row| row.get::<_, String>(0))?;
    Ok(rows.next().and_then(Result::ok))
}

pub fn store_metadata(
    root: &Path,
    kind: &str,
    signature: &str,
    payload: &str,
    created_at: i64,
) -> Result<()> {
    let connection = Connection::open(stats_db_path(root))?;
    connection.execute(
        "DELETE FROM metadata_cache WHERE kind = ?1 AND signature <> ?2",
        params![kind, signature],
    )?;
    connection.execute(
        "INSERT INTO metadata_cache (kind, signature, payload, created_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(kind, signature) DO UPDATE SET payload = ?3, created_at = ?4",
        params![kind, signature, payload, created_at],
    )?;
    Ok(())
}

pub fn insert_build(
    root: &Path,
    target: &str,
    started_at: i64,
    report_dir: &str,
    log_path: &str,
) -> Result<i64> {
    let connection = Connection::open(stats_db_path(root))?;
    connection.execute(
        "INSERT INTO builds (target, status, started_at, report_dir, log_path)
         VALUES (?1, 'running', ?2, ?3, ?4)",
        params![target, started_at, report_dir, log_path],
    )?;
    Ok(connection.last_insert_rowid())
}

pub fn finish_build(
    root: &Path,
    id: i64,
    status: &str,
    finished_at: i64,
    elapsed_secs: i64,
) -> Result<()> {
    let connection = Connection::open(stats_db_path(root))?;
    connection.execute(
        "UPDATE builds SET status = ?1, finished_at = ?2, elapsed_secs = ?3 WHERE id = ?4",
        params![status, finished_at, elapsed_secs, id],
    )?;
    Ok(())
}

pub fn list_builds(root: &Path, limit: i64) -> Result<Vec<BuildRecord>> {
    let connection = Connection::open(stats_db_path(root))?;
    let mut statement = connection.prepare(
        "SELECT id, target, status, started_at, finished_at, elapsed_secs, report_dir, log_path
         FROM builds ORDER BY id DESC LIMIT ?1",
    )?;
    let records = statement
        .query_map(params![limit], row_to_record)?
        .filter_map(Result::ok)
        .collect();
    Ok(records)
}

pub fn get_build(root: &Path, id: i64) -> Result<Option<BuildRecord>> {
    let connection = Connection::open(stats_db_path(root))?;
    let mut statement = connection.prepare(
        "SELECT id, target, status, started_at, finished_at, elapsed_secs, report_dir, log_path
         FROM builds WHERE id = ?1",
    )?;
    let mut rows = statement.query_map(params![id], row_to_record)?;
    Ok(rows.next().and_then(Result::ok))
}

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<BuildRecord> {
    Ok(BuildRecord {
        id: row.get(0)?,
        target: row.get(1)?,
        status: row.get(2)?,
        started_at: row.get(3)?,
        finished_at: row.get(4)?,
        elapsed_secs: row.get(5)?,
        report_dir: row.get(6)?,
        log_path: row.get(7)?,
    })
}

#[derive(Debug, Clone)]
pub struct ActivityRecord {
    pub id: i64,
    pub label: String,
    pub kind: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub log: Option<String>,
}

pub fn record_activity_start(
    root: &Path,
    id: i64,
    label: &str,
    kind: &str,
    status: &str,
    started_at: i64,
) -> Result<()> {
    let connection = Connection::open(stats_db_path(root))?;
    connection.execute(
        "INSERT OR REPLACE INTO activities (id, label, kind, status, started_at, finished_at, log)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)",
        params![id, label, kind, status, started_at],
    )?;
    Ok(())
}

pub fn record_activity_finish(
    root: &Path,
    id: i64,
    status: &str,
    finished_at: i64,
    log: Option<&str>,
) -> Result<()> {
    let connection = Connection::open(stats_db_path(root))?;
    connection.execute(
        "UPDATE activities SET status = ?1, finished_at = ?2, log = ?3 WHERE id = ?4",
        params![status, finished_at, log, id],
    )?;
    Ok(())
}

pub fn mark_running_cancelled(root: &Path, finished_at: i64) -> Result<()> {
    let connection = Connection::open(stats_db_path(root))?;
    connection.execute(
        "UPDATE activities
         SET status = 'cancelled', finished_at = COALESCE(finished_at, ?1)
         WHERE status = 'running'",
        params![finished_at],
    )?;
    Ok(())
}

pub fn list_activities(root: &Path, limit: i64) -> Result<Vec<ActivityRecord>> {
    let connection = Connection::open(stats_db_path(root))?;
    let mut statement = connection.prepare(
        "SELECT id, label, kind, status, started_at, finished_at, log FROM (
            SELECT id, label, kind, status, started_at, finished_at, log
            FROM activities ORDER BY id DESC LIMIT ?1
         ) ORDER BY id ASC",
    )?;
    let records = statement
        .query_map(params![limit], |row| {
            Ok(ActivityRecord {
                id: row.get(0)?,
                label: row.get(1)?,
                kind: row.get(2)?,
                status: row.get(3)?,
                started_at: row.get(4)?,
                finished_at: row.get(5)?,
                log: row.get(6)?,
            })
        })?
        .filter_map(Result::ok)
        .collect();
    Ok(records)
}
