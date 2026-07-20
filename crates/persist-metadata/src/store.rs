use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use persist_core::{PersistError, Result};
use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub session_id: u32,
    pub name: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub cwd: Option<String>,
    pub shell: Option<String>,
    pub exit_code: Option<i32>,
    pub note: Option<String>,
    pub pinned: bool,
    pub locked: bool,
    pub env_snapshot: Option<String>,
    pub holder_instance: Option<String>,
    pub holder_generation: Option<i64>,
}

pub struct MetadataStore {
    conn: Connection,
}

impl MetadataStore {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| PersistError::Io {
                operation: "create metadata db parent directory",
                source,
            })?;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(
                |source| PersistError::Io {
                    operation: "set metadata db parent directory permission",
                    source,
                },
            )?;
        }

        let conn = Connection::open(db_path).map_err(|source| PersistError::MetadataOpen {
            path: db_path.to_path_buf(),
            message: source.to_string(),
        })?;
        std::fs::set_permissions(db_path, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| PersistError::Io {
                operation: "set metadata db permission",
                source,
            },
        )?;

        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn =
            Connection::open_in_memory().map_err(|source| PersistError::MetadataOperation {
                operation: "open in-memory metadata database",
                message: source.to_string(),
            })?;

        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_version (
                    version INTEGER NOT NULL
                );
                INSERT INTO schema_version (version)
                SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM schema_version);

                CREATE TABLE IF NOT EXISTS sessions (
                    session_id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'created',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    closed_at TEXT,
                    cwd TEXT,
                    shell TEXT,
                    exit_code INTEGER
                );",
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "migrate metadata schema",
                message: source.to_string(),
            })?;

        // Migration 2: add note column
        let version: i32 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if version < 2 {
            self.conn
                .execute_batch(
                    "ALTER TABLE sessions ADD COLUMN note TEXT;
                     INSERT INTO schema_version (version) VALUES (2);",
                )
                .map_err(|source| PersistError::MetadataOperation {
                    operation: "migrate to schema version 2 (add note column)",
                    message: source.to_string(),
                })?;
        }

        if version < 3 {
            self.conn
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS session_tags (
                        session_id INTEGER NOT NULL,
                        tag TEXT NOT NULL,
                        PRIMARY KEY (session_id, tag),
                        FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
                    );
                    INSERT INTO schema_version (version) VALUES (3);",
                )
                .map_err(|source| PersistError::MetadataOperation {
                    operation: "migrate to schema version 3 (add session_tags table)",
                    message: source.to_string(),
                })?;
        }

        if version < 4 {
            self.conn
                .execute_batch(
                    "ALTER TABLE sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
                     INSERT INTO schema_version (version) VALUES (4);",
                )
                .map_err(|source| PersistError::MetadataOperation {
                    operation: "migrate to schema version 4 (add pinned column)",
                    message: source.to_string(),
                })?;
        }

        if version < 5 {
            self.conn
                .execute_batch(
                    "ALTER TABLE sessions ADD COLUMN locked INTEGER NOT NULL DEFAULT 0;
                     INSERT INTO schema_version (version) VALUES (5);",
                )
                .map_err(|source| PersistError::MetadataOperation {
                    operation: "migrate to schema version 5 (add locked column)",
                    message: source.to_string(),
                })?;
        }

        if version < 6 {
            self.conn
                .execute_batch(
                    "ALTER TABLE sessions ADD COLUMN env_snapshot TEXT;
                     INSERT INTO schema_version (version) VALUES (6);",
                )
                .map_err(|source| PersistError::MetadataOperation {
                    operation: "migrate to schema version 6 (add env snapshot column)",
                    message: source.to_string(),
                })?;
        }

        if version < 7 {
            self.conn
                .execute_batch(
                    "BEGIN IMMEDIATE;
                     ALTER TABLE sessions ADD COLUMN holder_instance TEXT;
                     ALTER TABLE sessions ADD COLUMN holder_generation INTEGER;
                     INSERT INTO schema_version (version) VALUES (7);
                     COMMIT;",
                )
                .map_err(|source| PersistError::MetadataOperation {
                    operation: "migrate to schema version 7 (add holder reconciliation fields)",
                    message: source.to_string(),
                })?;
        }

        Ok(())
    }

    pub fn create_session(
        &mut self,
        session_id: u32,
        name: &str,
        cwd: Option<&str>,
        shell: Option<&str>,
    ) -> Result<()> {
        let now = iso_now();
        self.conn
            .execute(
                "INSERT INTO sessions (session_id, name, status, created_at, updated_at, cwd, shell)
                 VALUES (?1, ?2, 'running', ?3, ?3, ?4, ?5)",
                rusqlite::params![session_id, name, now, cwd, shell],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "create session metadata",
                message: source.to_string(),
            })?;
        Ok(())
    }

    pub fn next_session_id(&self) -> Result<u32> {
        let highest: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(session_id), 0) FROM sessions",
                [],
                |row| row.get(0),
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "find next session id",
                message: source.to_string(),
            })?;
        let highest = u32::try_from(highest)
            .map_err(|_| PersistError::invalid_argument("session id is out of range"))?;
        highest
            .checked_add(1)
            .ok_or_else(|| PersistError::invalid_argument("session id space exhausted"))
    }

    pub fn get_session(&self, session_id: u32) -> Result<Option<SessionRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT session_id, name, status, created_at, updated_at,
                        closed_at, cwd, shell, exit_code, note, pinned, locked, env_snapshot,
                        holder_instance, holder_generation
                 FROM sessions WHERE session_id = ?1",
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "prepare get session",
                message: source.to_string(),
            })?;

        let mut rows = stmt
            .query(rusqlite::params![session_id])
            .map_err(|source| PersistError::MetadataOperation {
                operation: "query get session",
                message: source.to_string(),
            })?;

        match rows
            .next()
            .map_err(|source| PersistError::MetadataOperation {
                operation: "fetch get session",
                message: source.to_string(),
            })? {
            Some(row) => Ok(Some(SessionRecord {
                session_id: row.get(0).map_err(|e| PersistError::Io {
                    operation: "read session_id",
                    source: std::io::Error::other(e.to_string()),
                })?,
                name: row.get(1).map_err(|e| PersistError::Io {
                    operation: "read name",
                    source: std::io::Error::other(e.to_string()),
                })?,
                status: row.get(2).map_err(|e| PersistError::Io {
                    operation: "read status",
                    source: std::io::Error::other(e.to_string()),
                })?,
                created_at: row.get(3).map_err(|e| PersistError::Io {
                    operation: "read created_at",
                    source: std::io::Error::other(e.to_string()),
                })?,
                updated_at: row.get(4).map_err(|e| PersistError::Io {
                    operation: "read updated_at",
                    source: std::io::Error::other(e.to_string()),
                })?,
                closed_at: row.get(5).map_err(|e| PersistError::Io {
                    operation: "read closed_at",
                    source: std::io::Error::other(e.to_string()),
                })?,
                cwd: row.get(6).map_err(|e| PersistError::Io {
                    operation: "read cwd",
                    source: std::io::Error::other(e.to_string()),
                })?,
                shell: row.get(7).map_err(|e| PersistError::Io {
                    operation: "read shell",
                    source: std::io::Error::other(e.to_string()),
                })?,
                exit_code: row.get(8).map_err(|e| PersistError::Io {
                    operation: "read exit_code",
                    source: std::io::Error::other(e.to_string()),
                })?,
                note: row.get(9).map_err(|e| PersistError::Io {
                    operation: "read note",
                    source: std::io::Error::other(e.to_string()),
                })?,
                pinned: row.get(10).map_err(|e| PersistError::Io {
                    operation: "read pinned",
                    source: std::io::Error::other(e.to_string()),
                })?,
                locked: row.get(11).map_err(|e| PersistError::Io {
                    operation: "read locked",
                    source: std::io::Error::other(e.to_string()),
                })?,
                env_snapshot: row.get(12).map_err(|e| PersistError::Io {
                    operation: "read env_snapshot",
                    source: std::io::Error::other(e.to_string()),
                })?,
                holder_instance: row.get(13).map_err(|e| PersistError::Io {
                    operation: "read holder_instance",
                    source: std::io::Error::other(e.to_string()),
                })?,
                holder_generation: row.get(14).map_err(|e| PersistError::Io {
                    operation: "read holder_generation",
                    source: std::io::Error::other(e.to_string()),
                })?,
            })),
            None => Ok(None),
        }
    }

    pub fn update_status(&mut self, session_id: u32, status: &str) -> Result<()> {
        let now = iso_now();
        self.conn
            .execute(
                "UPDATE sessions SET status = ?1, updated_at = ?2 WHERE session_id = ?3",
                rusqlite::params![status, now, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "update session status",
                message: source.to_string(),
            })?;
        Ok(())
    }

    pub fn reconcile_running(
        &mut self,
        session_id: u32,
        holder_instance: &str,
        holder_generation: u64,
    ) -> Result<()> {
        validate_holder_state(holder_instance, holder_generation)?;
        let holder_generation = holder_generation as i64;
        let now = iso_now();
        let affected = self
            .conn
            .execute(
                "UPDATE sessions SET status = 'running', closed_at = NULL, exit_code = NULL,
                        holder_instance = ?1, holder_generation = ?2, updated_at = ?3
                 WHERE session_id = ?4",
                rusqlite::params![holder_instance, holder_generation, now, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "reconcile running holder session",
                message: source.to_string(),
            })?;
        require_session_updated(affected)
    }

    pub fn reconcile_exited(
        &mut self,
        session_id: u32,
        exit_code: i32,
        cwd: Option<&str>,
        holder_instance: &str,
        holder_generation: u64,
    ) -> Result<()> {
        self.reconcile_exited_with_context(
            session_id,
            exit_code,
            cwd,
            None,
            holder_instance,
            holder_generation,
        )
    }

    pub fn reconcile_exited_with_context(
        &mut self,
        session_id: u32,
        exit_code: i32,
        cwd: Option<&str>,
        env_snapshot: Option<&str>,
        holder_instance: &str,
        holder_generation: u64,
    ) -> Result<()> {
        validate_holder_state(holder_instance, holder_generation)?;
        let holder_generation = holder_generation as i64;
        let now = iso_now();
        let affected = self
            .conn
            .execute(
                "UPDATE sessions SET status = 'closed', closed_at = COALESCE(closed_at, ?1),
                        exit_code = ?2, cwd = COALESCE(?3, cwd),
                        env_snapshot = COALESCE(?4, env_snapshot), holder_instance = ?5,
                        holder_generation = ?6, updated_at = ?1 WHERE session_id = ?7",
                rusqlite::params![
                    now,
                    exit_code,
                    cwd,
                    env_snapshot,
                    holder_instance,
                    holder_generation,
                    session_id
                ],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "reconcile exited holder session",
                message: source.to_string(),
            })?;
        require_session_updated(affected)
    }

    pub fn mark_lost(
        &mut self,
        session_id: u32,
        holder_instance: &str,
        holder_generation: u64,
    ) -> Result<()> {
        validate_holder_state(holder_instance, holder_generation)?;
        let holder_generation = holder_generation as i64;
        let now = iso_now();
        self.conn
            .execute(
                "UPDATE sessions SET status = 'lost', holder_instance = ?1,
                        holder_generation = ?2, updated_at = ?3
                 WHERE session_id = ?4 AND status IN ('running', 'attached', 'detached')",
                rusqlite::params![holder_instance, holder_generation, now, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "mark missing holder session lost",
                message: source.to_string(),
            })?;
        Ok(())
    }

    pub fn close_session(&mut self, session_id: u32, exit_code: i32) -> Result<()> {
        let now = iso_now();
        self.conn
            .execute(
                "UPDATE sessions SET status = 'closed', closed_at = ?1, exit_code = ?2,
                        updated_at = ?1
                 WHERE session_id = ?3",
                rusqlite::params![now, exit_code, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "close session",
                message: source.to_string(),
            })?;
        Ok(())
    }

    pub fn close_session_with_context(
        &mut self,
        session_id: u32,
        exit_code: i32,
        cwd: Option<&str>,
        env_snapshot: Option<&str>,
    ) -> Result<()> {
        let now = iso_now();
        self.conn
            .execute(
                "UPDATE sessions SET status = 'closed', closed_at = ?1, exit_code = ?2,
                        cwd = COALESCE(?3, cwd), env_snapshot = COALESCE(?4, env_snapshot),
                        updated_at = ?1 WHERE session_id = ?5",
                rusqlite::params![now, exit_code, cwd, env_snapshot, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "close session with recovery context",
                message: source.to_string(),
            })?;
        Ok(())
    }

    pub fn reopen_session(&mut self, session_id: u32) -> Result<()> {
        let now = iso_now();
        let affected = self
            .conn
            .execute(
                "UPDATE sessions SET status = 'running', closed_at = NULL, exit_code = NULL,
                        updated_at = ?1 WHERE session_id = ?2",
                rusqlite::params![now, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "reopen closed session",
                message: source.to_string(),
            })?;
        if affected == 0 {
            return Err(PersistError::invalid_argument("session not found"));
        }
        Ok(())
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT session_id, name, status, created_at, updated_at,
                        closed_at, cwd, shell, exit_code, note, pinned, locked, env_snapshot,
                        holder_instance, holder_generation
                 FROM sessions ORDER BY session_id",
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "prepare list sessions",
                message: source.to_string(),
            })?;

        let rows = stmt
            .query_map([], |row| {
                Ok(SessionRecord {
                    session_id: row.get(0)?,
                    name: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    closed_at: row.get(5)?,
                    cwd: row.get(6)?,
                    shell: row.get(7)?,
                    exit_code: row.get(8)?,
                    note: row.get(9)?,
                    pinned: row.get(10)?,
                    locked: row.get(11)?,
                    env_snapshot: row.get(12)?,
                    holder_instance: row.get(13)?,
                    holder_generation: row.get(14)?,
                })
            })
            .map_err(|source| PersistError::MetadataOperation {
                operation: "query list sessions",
                message: source.to_string(),
            })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row.map_err(|source| PersistError::Io {
                operation: "read session row",
                source: std::io::Error::other(source.to_string()),
            })?);
        }
        Ok(sessions)
    }

    pub fn rename_session(&mut self, session_id: u32, name: &str) -> Result<()> {
        let now = iso_now();
        let affected = self
            .conn
            .execute(
                "UPDATE sessions SET name = ?1, updated_at = ?2 WHERE session_id = ?3",
                rusqlite::params![name, now, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "rename session",
                message: source.to_string(),
            })?;
        if affected == 0 {
            return Err(PersistError::invalid_argument("session not found"));
        }
        Ok(())
    }

    pub fn set_session_note(&mut self, session_id: u32, note: Option<&str>) -> Result<()> {
        let now = iso_now();
        let affected = self
            .conn
            .execute(
                "UPDATE sessions SET note = ?1, updated_at = ?2 WHERE session_id = ?3",
                rusqlite::params![note, now, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "set session note",
                message: source.to_string(),
            })?;
        if affected == 0 {
            return Err(PersistError::invalid_argument("session not found"));
        }
        Ok(())
    }

    pub fn set_session_pinned(&mut self, session_id: u32, pinned: bool) -> Result<()> {
        let now = iso_now();
        let affected = self
            .conn
            .execute(
                "UPDATE sessions SET pinned = ?1, updated_at = ?2 WHERE session_id = ?3",
                rusqlite::params![pinned as i32, now, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "set session pinned",
                message: source.to_string(),
            })?;
        if affected == 0 {
            return Err(PersistError::invalid_argument("session not found"));
        }
        Ok(())
    }

    pub fn set_session_locked(&mut self, session_id: u32, locked: bool) -> Result<()> {
        let now = iso_now();
        let affected = self
            .conn
            .execute(
                "UPDATE sessions SET locked = ?1, updated_at = ?2 WHERE session_id = ?3",
                rusqlite::params![locked as i32, now, session_id],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "set session locked",
                message: source.to_string(),
            })?;
        if affected == 0 {
            return Err(PersistError::invalid_argument("session not found"));
        }
        Ok(())
    }

    pub fn add_session_tag(&mut self, session_id: u32, tag: &str) -> Result<()> {
        if tag.trim().is_empty() || tag.contains(' ') {
            return Err(PersistError::invalid_argument(
                "tag must be non-empty and contain no spaces",
            ));
        }
        self.conn
            .execute(
                "INSERT OR IGNORE INTO session_tags (session_id, tag) VALUES (?1, ?2)",
                rusqlite::params![session_id, tag],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "add session tag",
                message: source.to_string(),
            })?;
        Ok(())
    }

    pub fn remove_session_tag(&mut self, session_id: u32, tag: &str) -> Result<()> {
        let affected = self
            .conn
            .execute(
                "DELETE FROM session_tags WHERE session_id = ?1 AND tag = ?2",
                rusqlite::params![session_id, tag],
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "remove session tag",
                message: source.to_string(),
            })?;
        if affected == 0 {
            return Err(PersistError::invalid_argument("tag not found on session"));
        }
        Ok(())
    }

    pub fn list_session_tags(&self, session_id: u32) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM session_tags WHERE session_id = ?1 ORDER BY tag")
            .map_err(|source| PersistError::MetadataOperation {
                operation: "prepare list session tags",
                message: source.to_string(),
            })?;

        let rows = stmt
            .query_map(rusqlite::params![session_id], |row| row.get::<_, String>(0))
            .map_err(|source| PersistError::MetadataOperation {
                operation: "query list session tags",
                message: source.to_string(),
            })?;

        let mut tags = Vec::new();
        for row in rows {
            tags.push(row.map_err(|source| PersistError::Io {
                operation: "read tag",
                source: std::io::Error::other(source.to_string()),
            })?);
        }
        Ok(tags)
    }

    pub fn find_sessions_by_tag(&self, tag: &str) -> Result<Vec<u32>> {
        let mut stmt = self
            .conn
            .prepare("SELECT session_id FROM session_tags WHERE tag = ?1 ORDER BY session_id")
            .map_err(|source| PersistError::MetadataOperation {
                operation: "prepare find sessions by tag",
                message: source.to_string(),
            })?;

        let rows = stmt
            .query_map(rusqlite::params![tag], |row| row.get::<_, u32>(0))
            .map_err(|source| PersistError::MetadataOperation {
                operation: "query find sessions by tag",
                message: source.to_string(),
            })?;

        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(|source| PersistError::Io {
                operation: "read session_id",
                source: std::io::Error::other(source.to_string()),
            })?);
        }
        Ok(ids)
    }

    pub fn session_has_tag(&self, session_id: u32, tag: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM session_tags WHERE session_id = ?1 AND tag = ?2")
            .map_err(|source| PersistError::MetadataOperation {
                operation: "prepare check session tag",
                message: source.to_string(),
            })?;

        let count: i32 = stmt
            .query_row(rusqlite::params![session_id, tag], |row| row.get(0))
            .map_err(|source| PersistError::MetadataOperation {
                operation: "query check session tag",
                message: source.to_string(),
            })?;

        Ok(count > 0)
    }

    pub fn list_sessions_by_status(&self, status: &str) -> Result<Vec<SessionRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT session_id, name, status, created_at, updated_at,
                        closed_at, cwd, shell, exit_code, note, pinned, locked, env_snapshot,
                        holder_instance, holder_generation
                 FROM sessions WHERE status = ?1 ORDER BY session_id",
            )
            .map_err(|source| PersistError::MetadataOperation {
                operation: "prepare list sessions by status",
                message: source.to_string(),
            })?;

        let rows = stmt
            .query_map(rusqlite::params![status], |row| {
                Ok(SessionRecord {
                    session_id: row.get(0)?,
                    name: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    closed_at: row.get(5)?,
                    cwd: row.get(6)?,
                    shell: row.get(7)?,
                    exit_code: row.get(8)?,
                    note: row.get(9)?,
                    pinned: row.get(10)?,
                    locked: row.get(11)?,
                    env_snapshot: row.get(12)?,
                    holder_instance: row.get(13)?,
                    holder_generation: row.get(14)?,
                })
            })
            .map_err(|source| PersistError::MetadataOperation {
                operation: "query list sessions by status",
                message: source.to_string(),
            })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row.map_err(|source| PersistError::Io {
                operation: "read session row",
                source: std::io::Error::other(source.to_string()),
            })?);
        }
        Ok(sessions)
    }
}

fn validate_holder_state(instance: &str, generation: u64) -> Result<()> {
    if instance.len() != 32 || !instance.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PersistError::invalid_argument(
            "holder instance must be 16-byte hexadecimal",
        ));
    }
    if generation > i64::MAX as u64 {
        return Err(PersistError::invalid_argument(
            "holder generation exceeds metadata integer range",
        ));
    }
    Ok(())
}

fn require_session_updated(affected: usize) -> Result<()> {
    if affected == 0 {
        return Err(PersistError::invalid_argument("session not found"));
    }
    Ok(())
}

fn iso_now() -> String {
    // Simple ISO 8601 without external dep: format seconds since epoch
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Basic UTC timestamp: seconds since epoch as string
    // For readability, compute a simple YYYY-MM-DDTHH:MM:SSZ
    format_iso(secs)
}

fn format_iso(secs: u64) -> String {
    // Days since epoch
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Year/month/day from days since 1970-01-01
    let mut y = 1970i64;
    let mut remaining = days as i64;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            m = i + 1;
            break;
        }
        remaining -= md;
    }
    if m == 0 {
        m = 12;
    }
    let d = remaining + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_enforces_private_database_permissions() {
        let dir = std::env::temp_dir().join(format!(
            "persist-metadata-perms-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let db_path = dir.join("metadata.db");
        let store = MetadataStore::open(&db_path).expect("open database");

        let parent_mode = std::fs::metadata(&dir)
            .expect("parent metadata")
            .permissions()
            .mode()
            & 0o777;
        let db_mode = std::fs::metadata(&db_path)
            .expect("db metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(parent_mode, 0o700);
        assert_eq!(db_mode, 0o600);

        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_in_memory_creates_schema() {
        let store = MetadataStore::open_in_memory().expect("open in-memory");
        let sessions = store.list_sessions().expect("list sessions");
        assert!(sessions.is_empty());
    }

    #[test]
    fn version_six_database_migrates_holder_fields() {
        let dir = std::env::temp_dir().join(format!(
            "persist-metadata-v6-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("metadata.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (6);
             CREATE TABLE sessions (
                 session_id INTEGER PRIMARY KEY, name TEXT NOT NULL,
                 status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                 closed_at TEXT, cwd TEXT, shell TEXT, exit_code INTEGER, note TEXT,
                 pinned INTEGER NOT NULL DEFAULT 0, locked INTEGER NOT NULL DEFAULT 0,
                 env_snapshot TEXT
             );
             CREATE TABLE session_tags (
                 session_id INTEGER NOT NULL, tag TEXT NOT NULL,
                 PRIMARY KEY (session_id, tag)
             );
             INSERT INTO sessions
                 (session_id, name, status, created_at, updated_at, pinned, locked)
             VALUES (1, 'old', 'running', 'now', 'now', 0, 0);",
        )
        .unwrap();
        drop(conn);

        let mut store = MetadataStore::open(&path).expect("migrate v6 database");
        let old = store.get_session(1).unwrap().unwrap();
        assert!(old.holder_instance.is_none());
        store
            .reconcile_running(1, "00112233445566778899aabbccddeeff", 3)
            .unwrap();
        assert_eq!(
            store.get_session(1).unwrap().unwrap().holder_generation,
            Some(3)
        );
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn create_and_get_session() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");

        store
            .create_session(1, "test-session", Some("/home/user"), Some("/bin/bash"))
            .expect("create session");

        let session = store
            .get_session(1)
            .expect("get session")
            .expect("session exists");
        assert_eq!(session.session_id, 1);
        assert_eq!(session.name, "test-session");
        assert_eq!(session.status, "running");
        assert_eq!(session.cwd.as_deref(), Some("/home/user"));
        assert_eq!(session.shell.as_deref(), Some("/bin/bash"));
        assert!(session.closed_at.is_none());
        assert!(session.exit_code.is_none());
        assert!(session.holder_instance.is_none());
        assert!(session.holder_generation.is_none());
    }

    #[test]
    fn get_nonexistent_session_returns_none() {
        let store = MetadataStore::open_in_memory().expect("open in-memory");
        let session = store.get_session(999).expect("get session");
        assert!(session.is_none());
    }

    #[test]
    fn update_status() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");

        store.create_session(1, "s1", None, None).expect("create");
        store.update_status(1, "closed").expect("update");

        let session = store.get_session(1).expect("get").expect("exists");
        assert_eq!(session.status, "closed");
    }

    #[test]
    fn close_session_sets_exit_code() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");

        store.create_session(1, "s1", None, None).expect("create");
        store.close_session(1, 42).expect("close");

        let session = store.get_session(1).expect("get").expect("exists");
        assert_eq!(session.status, "closed");
        assert_eq!(session.exit_code, Some(42));
        assert!(session.closed_at.is_some());
    }

    #[test]
    fn close_session_context_persists_and_reopens() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store
            .create_session(1, "s1", Some("/initial"), Some("/bin/sh"))
            .expect("create");
        store
            .close_session_with_context(1, 0, Some("/work"), Some(r#"{"LANG":"C"}"#))
            .expect("close with context");

        let closed = store.get_session(1).expect("get").expect("exists");
        assert_eq!(closed.status, "closed");
        assert_eq!(closed.cwd.as_deref(), Some("/work"));
        assert_eq!(closed.env_snapshot.as_deref(), Some(r#"{"LANG":"C"}"#));

        store.reopen_session(1).expect("reopen");
        let reopened = store.get_session(1).expect("get").expect("exists");
        assert_eq!(reopened.status, "running");
        assert!(reopened.closed_at.is_none());
        assert!(reopened.exit_code.is_none());
    }

    #[test]
    fn holder_reconciliation_is_idempotent() {
        const INSTANCE: &str = "00112233445566778899aabbccddeeff";
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store
            .close_session_with_context(1, 0, Some("/old"), Some(r#"{"LANG":"C"}"#))
            .expect("seed recovery context");
        store.reopen_session(1).expect("reopen");

        store
            .reconcile_running(1, INSTANCE, 7)
            .expect("reconcile running");
        store
            .reconcile_running(1, INSTANCE, 7)
            .expect("repeat running");
        let running = store.get_session(1).unwrap().unwrap();
        assert_eq!(running.status, "running");
        assert_eq!(running.holder_instance.as_deref(), Some(INSTANCE));
        assert_eq!(running.holder_generation, Some(7));

        store
            .reconcile_exited(1, 23, Some("/srv/final"), INSTANCE, 8)
            .expect("reconcile exited");
        store
            .reconcile_exited(1, 23, None, INSTANCE, 8)
            .expect("repeat exited");
        let closed = store.get_session(1).unwrap().unwrap();
        assert_eq!(closed.status, "closed");
        assert_eq!(closed.exit_code, Some(23));
        assert_eq!(closed.cwd.as_deref(), Some("/srv/final"));
        assert_eq!(closed.env_snapshot.as_deref(), Some(r#"{"LANG":"C"}"#));
        assert_eq!(closed.holder_generation, Some(8));
    }

    #[test]
    fn mark_lost_only_changes_active_metadata() {
        const INSTANCE: &str = "ffeeddccbbaa99887766554433221100";
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.mark_lost(1, INSTANCE, 9).expect("mark lost");
        store.mark_lost(1, INSTANCE, 9).expect("repeat lost");
        assert_eq!(store.get_session(1).unwrap().unwrap().status, "lost");

        store.create_session(2, "s2", None, None).expect("create");
        store.close_session(2, 0).expect("close");
        store.mark_lost(2, INSTANCE, 9).expect("ignore closed");
        assert_eq!(store.get_session(2).unwrap().unwrap().status, "closed");
    }

    #[test]
    fn holder_reconciliation_rejects_invalid_identity() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        assert!(store.reconcile_running(1, "not-hex", 1).is_err());
        assert!(store
            .reconcile_running(1, "00112233445566778899aabbccddeeff", u64::MAX)
            .is_err());
    }

    #[test]
    fn list_sessions_returns_all() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");

        store.create_session(1, "s1", None, None).expect("create");
        store.create_session(2, "s2", None, None).expect("create");

        let sessions = store.list_sessions().expect("list");
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn list_by_status() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");

        store.create_session(1, "s1", None, None).expect("create");
        store.create_session(2, "s2", None, None).expect("create");
        store.close_session(1, 0).expect("close");

        let running = store.list_sessions_by_status("running").expect("list");
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].session_id, 2);

        let closed = store.list_sessions_by_status("closed").expect("list");
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].session_id, 1);
    }

    #[test]
    fn multiple_sessions_have_unique_ids() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");

        for i in 1..=5 {
            store
                .create_session(i, &format!("s{i}"), None, None)
                .expect("create");
        }

        assert_eq!(store.list_sessions().expect("list").len(), 5);

        // Close middle session
        store.close_session(3, 1).expect("close");
        let closed = store.list_sessions_by_status("closed").expect("list");
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].session_id, 3);
    }

    #[test]
    fn next_session_id_follows_highest_persisted_id() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        assert_eq!(store.next_session_id().expect("next id"), 1);
        store.create_session(2, "s2", None, None).expect("create");
        store.create_session(7, "s7", None, None).expect("create");
        assert_eq!(store.next_session_id().expect("next id"), 8);
    }

    #[test]
    fn note_default_is_none() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        let session = store.get_session(1).expect("get").expect("exists");
        assert!(session.note.is_none());
    }

    #[test]
    fn set_session_note_stores_note() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store
            .set_session_note(1, Some("my note"))
            .expect("set note");
        let session = store.get_session(1).expect("get").expect("exists");
        assert_eq!(session.note.as_deref(), Some("my note"));
    }

    #[test]
    fn set_session_note_to_none_clears_note() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store
            .set_session_note(1, Some("my note"))
            .expect("set note");
        store.set_session_note(1, None).expect("clear note");
        let session = store.get_session(1).expect("get").expect("exists");
        assert!(session.note.is_none());
    }

    #[test]
    fn set_session_note_nonexistent_returns_error() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        let result = store.set_session_note(999, Some("note"));
        assert!(result.is_err());
    }

    #[test]
    fn note_persists_in_list_sessions() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store
            .set_session_note(1, Some("my note"))
            .expect("set note");
        let sessions = store.list_sessions().expect("list");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].note.as_deref(), Some("my note"));
    }

    #[test]
    fn add_session_tag_stores_tag() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.add_session_tag(1, "work").expect("add tag");
        let tags = store.list_session_tags(1).expect("list tags");
        assert_eq!(tags, vec!["work"]);
    }

    #[test]
    fn add_session_tag_rejects_empty_and_spaces() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        assert!(store.add_session_tag(1, "").is_err());
        assert!(store.add_session_tag(1, "has space").is_err());
    }

    #[test]
    fn add_session_tag_duplicate_is_idempotent() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.add_session_tag(1, "work").expect("add tag");
        store.add_session_tag(1, "work").expect("add duplicate tag");
        let tags = store.list_session_tags(1).expect("list tags");
        assert_eq!(tags, vec!["work"]);
    }

    #[test]
    fn remove_session_tag_removes_tag() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.add_session_tag(1, "work").expect("add tag");
        store.add_session_tag(1, "personal").expect("add tag");
        store.remove_session_tag(1, "work").expect("remove tag");
        let tags = store.list_session_tags(1).expect("list tags");
        assert_eq!(tags, vec!["personal"]);
    }

    #[test]
    fn remove_session_tag_nonexistent_returns_error() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        let result = store.remove_session_tag(1, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn list_session_tags_empty_for_no_tags() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        let tags = store.list_session_tags(1).expect("list tags");
        assert!(tags.is_empty());
    }

    #[test]
    fn find_sessions_by_tag_returns_matching() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.create_session(2, "s2", None, None).expect("create");
        store.create_session(3, "s3", None, None).expect("create");
        store.add_session_tag(1, "work").expect("add tag");
        store.add_session_tag(3, "work").expect("add tag");
        store.add_session_tag(2, "personal").expect("add tag");

        let ids = store.find_sessions_by_tag("work").expect("find");
        assert_eq!(ids, vec![1, 3]);

        let personal = store.find_sessions_by_tag("personal").expect("find");
        assert_eq!(personal, vec![2]);
    }

    #[test]
    fn session_has_tag_checks_correctly() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.add_session_tag(1, "work").expect("add tag");
        assert!(store.session_has_tag(1, "work").expect("check"));
        assert!(!store.session_has_tag(1, "personal").expect("check"));
    }

    #[test]
    fn iso_format_is_valid() {
        let ts = format_iso(0);
        assert_eq!(ts, "1970-01-01T00:00:00Z");

        let ts = format_iso(1700000000);
        // 1700000000 = 2023-11-14T22:13:20Z approx
        assert!(ts.contains("T"));
        assert!(ts.ends_with("Z"));
        assert_eq!(ts.len(), 20); // "YYYY-MM-DDTHH:MM:SSZ"
    }

    #[test]
    fn set_session_pinned_sets_pin() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.set_session_pinned(1, true).expect("pin");
        let session = store.get_session(1).expect("get").expect("exists");
        assert!(session.pinned);
    }

    #[test]
    fn set_session_pinned_unpin() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.set_session_pinned(1, true).expect("pin");
        store.set_session_pinned(1, false).expect("unpin");
        let session = store.get_session(1).expect("get").expect("exists");
        assert!(!session.pinned);
    }

    #[test]
    fn set_session_pinned_nonexistent_returns_error() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        let result = store.set_session_pinned(999, true);
        assert!(result.is_err());
    }

    #[test]
    fn new_session_default_not_pinned() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        let session = store.get_session(1).expect("get").expect("exists");
        assert!(!session.pinned);
    }

    #[test]
    fn set_session_locked_persists_state() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.set_session_locked(1, true).expect("lock");
        let session = store.get_session(1).expect("get").expect("exists");
        assert!(session.locked);

        store.set_session_locked(1, false).expect("unlock");
        let session = store.get_session(1).expect("get").expect("exists");
        assert!(!session.locked);
    }

    #[test]
    fn set_session_locked_nonexistent_returns_error() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        assert!(store.set_session_locked(999, true).is_err());
    }

    #[test]
    fn new_session_default_not_locked() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        let session = store.get_session(1).expect("get").expect("exists");
        assert!(!session.locked);
    }

    #[test]
    fn list_sessions_includes_pinned() {
        let mut store = MetadataStore::open_in_memory().expect("open in-memory");
        store.create_session(1, "s1", None, None).expect("create");
        store.create_session(2, "s2", None, None).expect("create");
        store.set_session_pinned(1, true).expect("pin");
        let sessions = store.list_sessions().expect("list");
        let s1 = sessions.iter().find(|s| s.session_id == 1).expect("s1");
        let s2 = sessions.iter().find(|s| s.session_id == 2).expect("s2");
        assert!(s1.pinned);
        assert!(!s2.pinned);
    }
}
