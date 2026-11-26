use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::models::{BuildRecord, Workspace};
use crate::utils::path::data_dir;

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open() -> Result<Self> {
        let directory = data_dir()?;
        std::fs::create_dir_all(&directory)?;
        let path = directory.join("voidforge.sqlite3");
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(include_str!("../migrations/001_init.sql"))?;
        ensure_workspace_columns(&conn)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

fn ensure_workspace_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(workspaces)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if !columns.iter().any(|name| name == "remotes") {
        conn.execute_batch(
            "ALTER TABLE workspaces ADD COLUMN remotes TEXT NOT NULL DEFAULT '[]';",
        )?;
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
        params![key, value],
    )?;
    Ok(())
}

pub fn workspaces(conn: &Connection) -> Result<Vec<Workspace>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, path, COALESCE(branch,''), COALESCE(remotes,'[]'), is_valid, created_at
         FROM workspaces ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        let remote_json: String = row.get(4)?;
        let remotes: Vec<String> = serde_json::from_str(&remote_json).unwrap_or_default();
        Ok(Workspace {
            id: row.get(0)?,
            name: row.get(1)?,
            path: PathBuf::from(row.get::<_, String>(2)?),
            branch: row.get(3)?,
            remotes,
            valid: row.get::<_, i64>(5)? != 0,
            created_at: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn insert_workspace(
    conn: &Connection,
    name: &str,
    path: &str,
    branch: &str,
    remotes: &[String],
    valid: bool,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO workspaces (name, path, branch, remotes, is_valid, last_opened_at)
         VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)",
        params![
            name,
            path,
            branch,
            serde_json::to_string(remotes)?,
            valid as i64
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_workspace(
    conn: &Connection,
    id: i64,
    branch: &str,
    remotes: &[String],
    valid: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE workspaces SET branch = ?2, remotes = ?3, is_valid = ?4,
         last_opened_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id, branch, serde_json::to_string(remotes)?, valid as i64],
    )?;
    Ok(())
}

pub fn delete_workspace(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn builds(conn: &Connection, limit: usize) -> Result<Vec<BuildRecord>> {
    let mut stmt = conn.prepare(
        "SELECT b.id, COALESCE(w.name,'unknown'), b.package_name, b.operation, b.status,
                b.started_at, COALESCE(b.finished_at,''), COALESCE(b.duration_seconds,0),
                COALESCE(b.log_path,'')
         FROM builds b LEFT JOIN workspaces w ON w.id = b.workspace_id
         ORDER BY b.id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(BuildRecord {
            id: row.get(0)?,
            workspace: row.get(1)?,
            package: row.get(2)?,
            operation: row.get(3)?,
            status: row.get(4)?,
            started_at: row.get(5)?,
            finished_at: {
                let value: String = row.get(6)?;
                if value.is_empty() {
                    None
                } else {
                    Some(value)
                }
            },
            duration_seconds: Some(row.get(7)?),
            log_path: {
                let value: String = row.get(8)?;
                if value.is_empty() {
                    None
                } else {
                    Some(value)
                }
            },
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn record_build_start(
    conn: &Connection,
    workspace_id: Option<i64>,
    package: &str,
    operation: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO builds (workspace_id, package_name, operation, status) VALUES (?1, ?2, ?3, 'running')",
        params![workspace_id, package, operation],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Remove a build row when the process could not be started, so the history
/// never shows a build stuck in `running` forever.
pub fn delete_build(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM builds WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn record_build_finish(
    conn: &Connection,
    id: i64,
    status: &str,
    log_path: &str,
    duration: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE builds SET status = ?2, finished_at = CURRENT_TIMESTAMP,
         duration_seconds = ?3, log_path = ?4 WHERE id = ?1",
        params![id, status, duration, log_path],
    )?;
    Ok(())
}

pub fn record_git(
    conn: &Connection,
    workspace_id: Option<i64>,
    operation: &str,
    detail: &str,
    status: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO git_operations (workspace_id, operation, detail, status) VALUES (?1, ?2, ?3, ?4)",
        params![workspace_id, operation, detail, status],
    )?;
    Ok(())
}

pub fn record_activity(conn: &Connection, kind: &str, summary: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO recent_activity (kind, summary) VALUES (?1, ?2)",
        params![kind, summary],
    )?;
    Ok(())
}
