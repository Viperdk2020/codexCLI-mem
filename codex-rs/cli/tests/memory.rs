use chrono::Utc;
use clap::Parser;
use codex_cli::memory::{MemoryCli, MemoryCommand, run};
use codex_memory::{
    factory,
    store::MemoryStore,
    types::{Counters, Kind, MemoryItem, RelevanceHints, Scope, Status},
};
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn parses_recall_for() {
    let cli = MemoryCli::parse_from(["memory", "recall", "--for", "hello"]);
    match cli.cmd {
        MemoryCommand::Recall { query } => assert_eq!(query, "hello"),
        _ => panic!("expected recall"),
    }
}

#[test]
fn sqlite_add_and_list() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let prev = std::env::current_dir()?;
    std::env::set_current_dir(dir.path())?;
    std::fs::create_dir_all(dir.path().join(".codex/memory"))?;
    unsafe { std::env::set_var("CODEX_MEMORY_BACKEND", "sqlite") };
    run(MemoryCli {
        cmd: MemoryCommand::Add {
            content: "hello".into(),
        },
    })?;
    let store = factory::open_repo_store(dir.path(), Some(factory::Backend::Sqlite))?;
    let items = store.list(None, None)?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].content, "hello");
    std::env::set_current_dir(prev)?;
    unsafe { std::env::remove_var("CODEX_MEMORY_BACKEND") };
    Ok(())
}

#[test]
fn migrate_jsonl_to_sqlite() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let jsonl_path = dir.path().join("mem.jsonl");
    let sqlite_path = dir.path().join("mem.db");
    let now = Utc::now().to_rfc3339();
    let item = MemoryItem {
        id: Uuid::new_v4().to_string(),
        created_at: now.clone(),
        updated_at: now,
        schema_version: 1,
        source: "test".into(),
        scope: Scope::Repo,
        status: Status::Active,
        kind: Kind::Note,
        content: "hello".into(),
        tags: Vec::new(),
        relevance_hints: RelevanceHints {
            files: Vec::new(),
            crates: Vec::new(),
            languages: Vec::new(),
            commands: Vec::new(),
        },
        counters: Counters {
            seen_count: 0,
            used_count: 0,
            last_used_at: None,
        },
        expiry: None,
    };
    let line = serde_json::to_string(&item)?;
    std::fs::write(&jsonl_path, format!("{line}\n"))?;
    run(MemoryCli {
        cmd: MemoryCommand::Migrate {
            jsonl: jsonl_path.clone(),
            sqlite: sqlite_path.clone(),
        },
    })?;
    let store = codex_memory::store::sqlite::SqliteMemoryStore::new(sqlite_path);
    let items = store.list(None, None)?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].content, "hello");
    Ok(())
}
