use super::*;

#[cfg(feature = "sqlite")]
use rusqlite::Connection;
#[cfg(feature = "sqlite")]
use rusqlite::OptionalExtension;
#[cfg(feature = "sqlite")]
use rusqlite::params;

#[cfg(feature = "sqlite")]
fn init_db(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS memory_items (
            id TEXT PRIMARY KEY,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            schema_version INTEGER NOT NULL,
            source TEXT NOT NULL,
            scope TEXT NOT NULL,
            status TEXT NOT NULL,
            kind TEXT NOT NULL,
            content TEXT NOT NULL,
            tags_json TEXT NOT NULL,
            relevance_hints_json TEXT NOT NULL,
            counters_json TEXT NOT NULL,
            expiry_json TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_memory_scope ON memory_items(scope);
        CREATE INDEX IF NOT EXISTS idx_memory_status ON memory_items(status);
        CREATE INDEX IF NOT EXISTS idx_memory_updated ON memory_items(updated_at);
        "#,
    )?;
    Ok(())
}

#[cfg(feature = "sqlite")]
fn scope_as_str(s: &Scope) -> &'static str {
    match s {
        Scope::Global => "global",
        Scope::Repo => "repo",
        Scope::Dir => "dir",
    }
}

#[cfg(feature = "sqlite")]
fn status_as_str(s: &Status) -> &'static str {
    match s {
        Status::Active => "active",
        Status::Archived => "archived",
    }
}

#[cfg(feature = "sqlite")]
fn parse_scope(s: &str) -> anyhow::Result<Scope> {
    match s {
        "global" => Ok(Scope::Global),
        "repo" => Ok(Scope::Repo),
        "dir" => Ok(Scope::Dir),
        other => anyhow::bail!("unknown scope: {other}"),
    }
}

#[cfg(feature = "sqlite")]
fn parse_status(s: &str) -> anyhow::Result<Status> {
    match s {
        "active" => Ok(Status::Active),
        "archived" => Ok(Status::Archived),
        other => anyhow::bail!("unknown status: {other}"),
    }
}

#[cfg(feature = "sqlite")]
fn kind_as_str(k: &crate::types::Kind) -> &'static str {
    use crate::types::Kind::*;
    match k {
        Pref => "pref",
        Fact => "fact",
        Profile => "profile",
        Instruction => "instruction",
        Note => "note",
    }
}

#[cfg(feature = "sqlite")]
fn parse_kind(s: &str) -> anyhow::Result<crate::types::Kind> {
    use crate::types::Kind::*;
    match s {
        "pref" => Ok(Pref),
        "fact" => Ok(Fact),
        "profile" => Ok(Profile),
        "instruction" => Ok(Instruction),
        "note" => Ok(Note),
        other => anyhow::bail!("unknown kind: {other}"),
    }
}

#[cfg(feature = "sqlite")]
fn open_conn(path: &std::path::Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(path)?;
    init_db(&conn)?;
    Ok(conn)
}

#[cfg(feature = "sqlite")]
fn item_to_cols(
    item: &MemoryItem,
) -> anyhow::Result<(
    &str,
    &str,
    &str,
    i64,
    &str,
    String,
    String,
    String,
    &str,
    String,
    String,
    String,
    Option<String>,
)> {
    Ok((
        &item.id,
        &item.created_at,
        &item.updated_at,
        i64::from(item.schema_version),
        &item.source,
        scope_as_str(&item.scope).to_string(),
        status_as_str(&item.status).to_string(),
        kind_as_str(&item.kind).to_string(),
        &item.content,
        serde_json::to_string(&item.tags)?,
        serde_json::to_string(&item.relevance_hints)?,
        serde_json::to_string(&item.counters)?,
        item.expiry
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    ))
}

#[cfg(feature = "sqlite")]
fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryItem> {
    use rusqlite::types::Type;
    let conv_err = |idx: usize, msg: String| -> rusqlite::Error {
        rusqlite::Error::FromSqlConversionFailure(
            idx,
            Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg)),
        )
    };
    let scope_s: String = row.get(5)?;
    let status_s: String = row.get(6)?;
    let kind_s: String = row.get(7)?;
    let tags_s: String = row.get(9)?;
    let hints_s: String = row.get(10)?;
    let counters_s: String = row.get(11)?;
    let expiry_s: Option<String> = row.get(12)?;

    let parse_json = |idx: usize, s: &str| -> rusqlite::Result<serde_json::Value> {
        serde_json::from_str(s)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e)))
    };

    Ok(MemoryItem {
        id: row.get::<_, String>(0)?,
        created_at: row.get::<_, String>(1)?,
        updated_at: row.get::<_, String>(2)?,
        schema_version: u16::try_from(row.get::<_, i64>(3)?).unwrap_or(1),
        source: row.get::<_, String>(4)?,
        scope: parse_scope(&scope_s)
            .map_err(|_| conv_err(5, format!("invalid scope: {}", scope_s)))?,
        status: parse_status(&status_s)
            .map_err(|_| conv_err(6, format!("invalid status: {}", status_s)))?,
        kind: parse_kind(&kind_s).map_err(|_| conv_err(7, format!("invalid kind: {}", kind_s)))?,
        content: row.get::<_, String>(8)?,
        tags: serde_json::from_value(parse_json(9, &tags_s)?)
            .map_err(|e| conv_err(9, format!("tags decode: {e}")))?,
        relevance_hints: serde_json::from_value(parse_json(10, &hints_s)?)
            .map_err(|e| conv_err(10, format!("hints decode: {e}")))?,
        counters: serde_json::from_value(parse_json(11, &counters_s)?)
            .map_err(|e| conv_err(11, format!("counters decode: {e}")))?,
        expiry: match expiry_s {
            Some(s) => Some(
                serde_json::from_value(parse_json(12, &s)?)
                    .map_err(|e| conv_err(12, format!("expiry decode: {e}")))?,
            ),
            None => None,
        },
    })
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone)]
pub struct SqliteMemoryStore {
    path: std::path::PathBuf,
}

#[cfg(feature = "sqlite")]
impl SqliteMemoryStore {
    pub fn new<P: AsRef<std::path::Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

#[cfg(feature = "sqlite")]
impl MemoryStore for SqliteMemoryStore {
    fn add(&self, item: MemoryItem) -> anyhow::Result<()> {
        let conn = open_conn(&self.path)?;
        let cols = item_to_cols(&item)?;
        conn.execute(
            "INSERT INTO memory_items (
                    id, created_at, updated_at, schema_version, source,
                    scope, status, kind, content,
                    tags_json, relevance_hints_json, counters_json, expiry_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                cols.0, cols.1, cols.2, cols.3, cols.4, cols.5, cols.6, cols.7, cols.8, cols.9,
                cols.10, cols.11, cols.12
            ],
        )?;
        Ok(())
    }

    fn update(&self, item: &MemoryItem) -> anyhow::Result<()> {
        let conn = open_conn(&self.path)?;
        let cols = item_to_cols(item)?;
        let n = conn.execute(
            "UPDATE memory_items SET
                created_at=?2, updated_at=?3, schema_version=?4, source=?5,
                scope=?6, status=?7, kind=?8, content=?9,
                tags_json=?10, relevance_hints_json=?11, counters_json=?12, expiry_json=?13
             WHERE id=?1",
            params![
                cols.0, cols.1, cols.2, cols.3, cols.4, cols.5, cols.6, cols.7, cols.8, cols.9,
                cols.10, cols.11, cols.12
            ],
        )?;
        if n == 0 {
            let missing_id = &item.id;
            anyhow::bail!("update: id not found: {missing_id}");
        }
        Ok(())
    }

    fn delete(&self, id: &str) -> anyhow::Result<()> {
        let conn = open_conn(&self.path)?;
        conn.execute("DELETE FROM memory_items WHERE id=?1", params![id])?;
        Ok(())
    }

    fn get(&self, id: &str) -> anyhow::Result<Option<MemoryItem>> {
        let conn = open_conn(&self.path)?;
        let row = conn
            .query_row(
                "SELECT id, created_at, updated_at, schema_version, source,
                        scope, status, kind, content,
                        tags_json, relevance_hints_json, counters_json, expiry_json
                 FROM memory_items WHERE id=?1",
                params![id],
                row_to_item,
            )
            .optional()?;
        Ok(row)
    }

    fn list(
        &self,
        scope: Option<Scope>,
        status: Option<Status>,
    ) -> anyhow::Result<Vec<MemoryItem>> {
        let conn = open_conn(&self.path)?;
        let base = "SELECT id, created_at, updated_at, schema_version, source,
                    scope, status, kind, content,
                    tags_json, relevance_hints_json, counters_json, expiry_json
             FROM memory_items";
        let (sql, params_any): (String, Vec<String>) = match (scope, status) {
            (None, None) => (format!("{base} ORDER BY updated_at DESC"), vec![]),
            (Some(sc), None) => (
                format!("{base} WHERE scope = ?1 ORDER BY updated_at DESC"),
                vec![scope_as_str(&sc).to_string()],
            ),
            (None, Some(st)) => (
                format!("{base} WHERE status = ?1 ORDER BY updated_at DESC"),
                vec![status_as_str(&st).to_string()],
            ),
            (Some(sc), Some(st)) => (
                format!("{base} WHERE scope = ?1 AND status = ?2 ORDER BY updated_at DESC"),
                vec![
                    scope_as_str(&sc).to_string(),
                    status_as_str(&st).to_string(),
                ],
            ),
        };

        let mut stmt = conn.prepare(&sql)?;
        let mut rows = if params_any.is_empty() {
            stmt.query([])?
        } else if params_any.len() == 1 {
            stmt.query(params![params_any[0]])?
        } else {
            stmt.query(params![params_any[0], params_any[1]])?
        };

        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_item(row)?);
        }
        Ok(out)
    }

    fn archive(&self, id: &str, archived: bool) -> anyhow::Result<()> {
        let conn = open_conn(&self.path)?;
        let st = if archived { "archived" } else { "active" };
        let n = conn.execute(
            "UPDATE memory_items SET status=?2 WHERE id=?1",
            params![id, st],
        )?;
        if n == 0 {
            anyhow::bail!("archive: id not found: {id}");
        }
        Ok(())
    }

    fn export(&self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        let conn = open_conn(&self.path)?;
        let mut stmt = conn.prepare(
            "SELECT id, created_at, updated_at, schema_version, source,
                    scope, status, kind, content,
                    tags_json, relevance_hints_json, counters_json, expiry_json
             FROM memory_items ORDER BY updated_at DESC",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let item = row_to_item(row)?;
            let line = serde_json::to_string(&item)?;
            use std::io::Write as _;
            out.write_all(line.as_bytes())?;
            out.write_all(b"\n")?;
        }
        Ok(())
    }

    fn import(&self, input: &mut dyn std::io::Read) -> anyhow::Result<usize> {
        let mut data = String::new();
        use std::io::Read as _;
        input.read_to_string(&mut data)?;
        let mut conn = open_conn(&self.path)?;
        let tx = conn.transaction()?;
        let mut count = 0usize;
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let item: MemoryItem = serde_json::from_str(line)?;
            let cols = item_to_cols(&item)?;
            tx.execute(
                "INSERT INTO memory_items (
                        id, created_at, updated_at, schema_version, source,
                        scope, status, kind, content,
                        tags_json, relevance_hints_json, counters_json, expiry_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(id) DO UPDATE SET
                        created_at=excluded.created_at,
                        updated_at=excluded.updated_at,
                        schema_version=excluded.schema_version,
                        source=excluded.source,
                        scope=excluded.scope,
                        status=excluded.status,
                        kind=excluded.kind,
                        content=excluded.content,
                        tags_json=excluded.tags_json,
                        relevance_hints_json=excluded.relevance_hints_json,
                        counters_json=excluded.counters_json,
                        expiry_json=excluded.expiry_json",
                params![
                    cols.0, cols.1, cols.2, cols.3, cols.4, cols.5, cols.6, cols.7, cols.8, cols.9,
                    cols.10, cols.11, cols.12
                ],
            )?;
            count += 1;
        }
        tx.commit()?;
        Ok(count)
    }

    fn stats(&self) -> anyhow::Result<serde_json::Value> {
        let conn = open_conn(&self.path)?;
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM memory_items", [], |r| r.get(0))?;
        let active: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_items WHERE status='active'",
            [],
            |r| r.get(0),
        )?;
        let archived: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_items WHERE status='archived'",
            [],
            |r| r.get(0),
        )?;
        let by_scope = {
            let mut m = serde_json::Map::new();
            for sc in ["global", "repo", "dir"] {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM memory_items WHERE scope=?1",
                    params![sc],
                    |r| r.get(0),
                )?;
                m.insert(sc.to_string(), serde_json::json!(n));
            }
            serde_json::Value::Object(m)
        };
        Ok(serde_json::json!({
            "total": total,
            "active": active,
            "archived": archived,
            "by_scope": by_scope,
        }))
    }
}
