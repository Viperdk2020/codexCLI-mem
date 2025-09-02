use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

fn sample_line(id: &str, content: &str) -> String {
    format!(
        r#"{{"id":"{id}","created_at":"2025-01-01T00:00:00Z","updated_at":"2025-01-01T00:00:00Z","schema_version":1,"source":"test","scope":"Repo","status":"Active","kind":"Note","content":"{content}","tags":[],"relevance_hints":{{"files":[],"crates":[],"languages":[],"commands":[]}},"counters":{{"seen_count":0,"used_count":0,"last_used_at":null}},"expiry":null}}"#
    )
}

#[test]
fn memory_compact_removes_duplicates() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let input = dir.path().join("mem.jsonl");
    let output = dir.path().join("out.jsonl");
    let data = [
        sample_line("1", "one"),
        sample_line("2", "two"),
        sample_line("1", "one"),
    ]
    .join("\n");
    fs::write(&input, data + "\n")?;

    Command::cargo_bin("codex")?
        .args([
            "memory",
            "compact",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Read 3 entries, wrote 2 entries"));

    let out_data = fs::read_to_string(&output)?;
    assert_eq!(out_data.lines().count(), 2);
    Ok(())
}

#[test]
fn memory_migrate_imports_entries() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let jsonl = dir.path().join("mem.jsonl");
    let sqlite = dir.path().join("mem.sqlite");
    let data = [sample_line("1", "one"), sample_line("2", "two")].join("\n");
    fs::write(&jsonl, data + "\n")?;

    Command::cargo_bin("codex")?
        .args([
            "memory",
            "migrate",
            "--jsonl",
            jsonl.to_str().unwrap(),
            "--sqlite",
            sqlite.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Migrated 2 entries"));

    assert!(sqlite.exists());
    Ok(())
}
