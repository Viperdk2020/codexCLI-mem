use codex_memory::factory::Backend;
use codex_memory::factory::open_repo_store;
use codex_memory::recall::RecallContext;
use codex_memory::recall::recall;

fn backends() -> Vec<Backend> {
    #[cfg(feature = "sqlite")]
    {
        vec![Backend::Jsonl, Backend::Sqlite]
    }
    #[cfg(not(feature = "sqlite"))]
    {
        vec![Backend::Jsonl]
    }
}

fn sample_ctx() -> RecallContext {
    RecallContext {
        repo_root: None,
        dir: None,
        current_file: None,
        crate_name: None,
        language: None,
        command: None,
        now_rfc3339: "2024-01-01T00:00:00Z".to_string(),
        item_cap: 10,
        token_cap: 1000,
    }
}

#[test]
fn recall_unimplemented_panics() {
    for be in backends() {
        let repo = tempfile::tempdir().unwrap();
        let store = open_repo_store(repo.path(), Some(be)).unwrap();
        let ctx = sample_ctx();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            recall(store.as_ref(), "", &ctx)
        }));
        assert!(res.is_err());
    }
}
