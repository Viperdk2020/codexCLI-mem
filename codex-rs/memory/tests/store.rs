use codex_memory::factory::Backend;
use codex_memory::factory::open_repo_store;
use codex_memory::types::*;

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

fn sample_item(id: &str, scope: Scope, status: Status) -> MemoryItem {
    MemoryItem {
        id: id.to_string(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        schema_version: 1,
        source: "test".to_string(),
        scope,
        status,
        kind: Kind::Note,
        content: format!("content-{id}"),
        tags: vec!["tag".to_string()],
        relevance_hints: RelevanceHints {
            files: vec![],
            crates: vec![],
            languages: vec![],
            commands: vec![],
        },
        counters: Counters {
            seen_count: 0,
            used_count: 0,
            last_used_at: None,
        },
        expiry: None,
    }
}

#[test]
fn store_crud_import_export_stats() {
    for be in backends() {
        let repo = tempfile::tempdir().unwrap();
        let store = open_repo_store(repo.path(), Some(be)).unwrap();

        // create
        let item_a = sample_item("a", Scope::Global, Status::Active);
        store.add(item_a.clone()).unwrap();
        assert_eq!(store.get("a").unwrap().unwrap().content, item_a.content);

        // update
        let mut updated = item_a.clone();
        updated.content = "updated".to_string();
        store.update(&updated).unwrap();
        assert_eq!(store.get("a").unwrap().unwrap().content, "updated");

        // list
        assert_eq!(store.list(None, None).unwrap().len(), 1);

        // archive
        store.archive("a", true).unwrap();
        assert_eq!(store.get("a").unwrap().unwrap().status, Status::Archived);

        // add second item for stats
        let item_b = sample_item("b", Scope::Repo, Status::Active);
        store.add(item_b.clone()).unwrap();

        // stats
        let stats = store.stats().unwrap();
        assert_eq!(stats["total"], 2);
        assert_eq!(stats["active"], 1);
        assert_eq!(stats["archived"], 1);
        assert_eq!(stats["by_scope"]["global"], 1);
        assert_eq!(stats["by_scope"]["repo"], 1);

        // export
        let mut buf = Vec::new();
        store.export(&mut buf).unwrap();

        // import into fresh store
        let repo2 = tempfile::tempdir().unwrap();
        let store2 = open_repo_store(repo2.path(), Some(be)).unwrap();
        store2.import(&mut buf.as_slice()).unwrap();
        assert_eq!(store2.list(None, None).unwrap().len(), 2);
        assert_eq!(store2.get("a").unwrap().unwrap().content, "updated");

        // delete
        store2.delete("a").unwrap();
        assert!(store2.get("a").unwrap().is_none());
    }
}
