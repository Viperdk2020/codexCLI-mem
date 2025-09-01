use crate::store::MemoryStore;
use crate::store::jsonl::JsonlMemoryStore;

#[cfg(feature = "sqlite")]
use crate::store::sqlite::SqliteMemoryStore;

/// Backend selection for memory persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Jsonl,
    #[cfg(feature = "sqlite")]
    Sqlite,
}

/// Choose backend using env `CODEX_MEMORY_BACKEND` if present: `sqlite` or `jsonl`.
/// Defaults to JSONL; if `sqlite` is requested but not compiled in, falls back to JSONL.
pub fn choose_backend_from_env() -> Backend {
    let v = std::env::var("CODEX_MEMORY_BACKEND").unwrap_or_default();
    match v.as_str() {
        #[cfg(feature = "sqlite")]
        "sqlite" | "SQLITE" => Backend::Sqlite,
        _ => Backend::Jsonl,
    }
}

/// Build a store for a repo‑scoped path inside `<repo>/.codex/memory/`.
/// Paths can be overridden via env:
/// - `CODEX_MEMORY_REPO_DB` for SQLite file path
/// - `CODEX_MEMORY_REPO_JSONL` for JSONL file path
pub fn open_repo_store(
    repo_root: &std::path::Path,
    backend: Option<Backend>,
) -> anyhow::Result<Box<dyn MemoryStore>> {
    let base = repo_root.join(".codex").join("memory");
    let be = backend.unwrap_or_else(choose_backend_from_env);
    Ok(match be {
        Backend::Jsonl => {
            let path = std::env::var("CODEX_MEMORY_REPO_JSONL")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| base.join("memory.jsonl"));
            Box::new(JsonlMemoryStore::new(path))
        }
        #[cfg(feature = "sqlite")]
        Backend::Sqlite => {
            let path = std::env::var("CODEX_MEMORY_REPO_DB")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| base.join("memory.db"));
            Box::new(SqliteMemoryStore::new(path))
        }
    })
}

/// Build a store for global scope under `~/.codex/memory/`.
/// Paths can be overridden via env:
/// - `CODEX_MEMORY_HOME_DB` for SQLite file path
/// - `CODEX_MEMORY_HOME_JSONL` for JSONL file path
pub fn open_global_store(
    home_dir: &std::path::Path,
    backend: Option<Backend>,
) -> anyhow::Result<Box<dyn MemoryStore>> {
    let base = home_dir.join(".codex").join("memory");
    let be = backend.unwrap_or_else(choose_backend_from_env);
    Ok(match be {
        Backend::Jsonl => {
            let path = std::env::var("CODEX_MEMORY_HOME_JSONL")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| base.join("memory.jsonl"));
            Box::new(JsonlMemoryStore::new(path))
        }
        #[cfg(feature = "sqlite")]
        Backend::Sqlite => {
            let path = std::env::var("CODEX_MEMORY_HOME_DB")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| base.join("memory.db"));
            Box::new(SqliteMemoryStore::new(path))
        }
    })
}
