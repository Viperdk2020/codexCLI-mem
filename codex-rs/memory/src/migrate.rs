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
    use crate::store::MemoryStore;
    use crate::store::sqlite::SqliteMemoryStore;
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

/// Compact a JSONL file by removing duplicate entries based on the `id` field.
///
/// - `input_path`: source JSONL file
/// - `output_path`: destination JSONL file (may be the same as `input_path`)
///
/// Returns a tuple of `(read_count, written_count)`.
pub fn compact_jsonl(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
) -> anyhow::Result<(usize, usize)> {
    use crate::types::MemoryItem;
    use std::collections::HashSet;
    use std::io::BufRead as _;
    use std::io::BufReader;
    use std::io::BufWriter;
    use std::io::Write as _;

    let infile = std::fs::File::open(input_path)?;
    let reader = BufReader::new(infile);

    let tmp_path = if output_path == input_path {
        let mut p = output_path.to_path_buf();
        p.set_extension("jsonl.tmp");
        p
    } else {
        output_path.to_path_buf()
    };
    if let Some(parent) = tmp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let outfile = std::fs::File::create(&tmp_path)?;
    let mut writer = BufWriter::new(outfile);

    let mut seen = HashSet::new();
    let mut read = 0usize;
    let mut written = 0usize;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        read += 1;
        if let Ok(item) = serde_json::from_str::<MemoryItem>(trimmed)
            && seen.insert(item.id) {
                writer.write_all(trimmed.as_bytes())?;
                writer.write_all(b"\n")?;
                written += 1;
            }
    }
    writer.flush()?;
    if output_path == input_path {
        std::fs::rename(tmp_path, output_path)?;
    }
    Ok((read, written))
}
