use crate::types::MemoryItem;
use crate::types::Scope;
use crate::types::Status;

pub trait MemoryStore: Send + Sync {
    fn add(&self, item: MemoryItem) -> anyhow::Result<()>;
    fn update(&self, item: &MemoryItem) -> anyhow::Result<()>;
    fn delete(&self, id: &str) -> anyhow::Result<()>;
    fn get(&self, id: &str) -> anyhow::Result<Option<MemoryItem>>;
    fn list(&self, scope: Option<Scope>, status: Option<Status>)
    -> anyhow::Result<Vec<MemoryItem>>;
    fn archive(&self, id: &str, archived: bool) -> anyhow::Result<()>;
    fn export(&self, out: &mut dyn std::io::Write) -> anyhow::Result<()>;
    fn import(&self, input: &mut dyn std::io::Read) -> anyhow::Result<usize>;
    fn stats(&self) -> anyhow::Result<serde_json::Value>;
}

pub mod jsonl;
