/// Migrate a JSONL file into a SQLite database file.
///
/// - `jsonl_path`: source JSONL (line‑delimited `MemoryItem`)
/// - `sqlite_path`: destination SQLite DB (created if missing)
///
/// Returns the count of imported rows.
#[cfg(feature = "sqlite")]
pub fn migrate_jsonl_to_sqlite(
    jsonl_path: &std::path::Path,
    sqlite_path: &std::path::Path,
) -> anyhow::Result<usize> {
    use crate::store::sqlite::SqliteMemoryStore;
    use crate::store::MemoryStore;
    use std::io::Read as _;

    let mut data = String::new();
    std::fs::File::open(jsonl_path)?.read_to_string(&mut data)?;

    let store = SqliteMemoryStore::new(sqlite_path);
    let mut cursor = std::io::Cursor::new(data);
    store.import(&mut cursor)
}

#[cfg(not(feature = "sqlite"))]
pub fn migrate_jsonl_to_sqlite(
    _jsonl_path: &std::path::Path,
    _sqlite_path: &std::path::Path,
) -> anyhow::Result<usize> {
    anyhow::bail!("sqlite backend not compiled; enable with `--features codex-memory/sqlite`");
}
