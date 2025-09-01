use codex_memory::recall::RecallContext;
use codex_memory::recall::recall;
use codex_memory::store::MemoryStore;
use codex_memory::types::Counters;
use codex_memory::types::Kind;
use codex_memory::types::MemoryItem;
use codex_memory::types::RelevanceHints;
use codex_memory::types::Scope;
use codex_memory::types::Status;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
struct TestStore {
    items: Mutex<HashMap<String, MemoryItem>>,
}

impl TestStore {
    fn new(items: Vec<MemoryItem>) -> Self {
        let map = items.into_iter().map(|i| (i.id.clone(), i)).collect();
        Self {
            items: Mutex::new(map),
        }
    }
}

impl MemoryStore for TestStore {
    fn add(&self, item: MemoryItem) -> anyhow::Result<()> {
        self.items.lock().unwrap().insert(item.id.clone(), item);
        Ok(())
    }

    fn update(&self, item: &MemoryItem) -> anyhow::Result<()> {
        self.items
            .lock()
            .unwrap()
            .insert(item.id.clone(), item.clone());
        Ok(())
    }

    fn delete(&self, _id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn get(&self, id: &str) -> anyhow::Result<Option<MemoryItem>> {
        Ok(self.items.lock().unwrap().get(id).cloned())
    }

    fn list(
        &self,
        _scope: Option<Scope>,
        status: Option<Status>,
    ) -> anyhow::Result<Vec<MemoryItem>> {
        let items = self.items.lock().unwrap();
        Ok(items
            .values()
            .filter(|i| match status.as_ref() {
                Some(s) => i.status == *s,
                None => true,
            })
            .cloned()
            .collect())
    }

    fn archive(&self, _id: &str, _archived: bool) -> anyhow::Result<()> {
        Ok(())
    }

    fn export(&self, _out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        Ok(())
    }

    fn import(&self, _input: &mut dyn std::io::Read) -> anyhow::Result<usize> {
        Ok(0)
    }

    fn stats(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }
}

fn item(id: &str, content: &str, lang: &str) -> MemoryItem {
    MemoryItem {
        id: id.to_string(),
        created_at: "2024-01-01T00:00:00Z".into(),
        updated_at: "2024-01-01T00:00:00Z".into(),
        schema_version: 1,
        source: "test".into(),
        scope: Scope::Global,
        status: Status::Active,
        kind: Kind::Fact,
        content: content.into(),
        tags: vec![],
        relevance_hints: RelevanceHints {
            files: vec![],
            crates: vec![],
            languages: vec![lang.into()],
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
fn ranks_and_updates_counters() {
    let a = item("1", "use cargo build for rust", "rust");
    let b = item("2", "cargo test runs tests", "rust");
    let c = item("3", "npm install packages", "javascript");
    let store = TestStore::new(vec![a.clone(), b.clone(), c.clone()]);
    let now = "2024-01-10T00:00:00Z".to_string();
    let ctx = RecallContext {
        repo_root: None,
        dir: None,
        current_file: None,
        crate_name: None,
        language: Some("rust".into()),
        command: None,
        now_rfc3339: now.clone(),
        item_cap: 2,
        token_cap: 50,
    };
    let out = recall(&store, "cargo build rust", &ctx).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].id, "1");
    assert_eq!(out[1].id, "2");
    let a_upd = store.get("1").unwrap().unwrap();
    assert_eq!(a_upd.counters.used_count, 1);
    assert_eq!(a_upd.counters.last_used_at.as_deref(), Some(now.as_str()));
    let b_upd = store.get("2").unwrap().unwrap();
    assert_eq!(b_upd.counters.used_count, 1);
    assert_eq!(b_upd.counters.last_used_at.as_deref(), Some(now.as_str()));
    let c_upd = store.get("3").unwrap().unwrap();
    assert_eq!(c_upd.counters.used_count, 0);
    assert_eq!(c_upd.counters.last_used_at, None);
}
