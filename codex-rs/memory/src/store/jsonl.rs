use super::*;

pub struct JsonlMemoryStore;

impl MemoryStore for JsonlMemoryStore {
    fn add(&self, _item: MemoryItem) -> anyhow::Result<()> {
        todo!()
    }
    fn update(&self, _item: &MemoryItem) -> anyhow::Result<()> {
        todo!()
    }
    fn delete(&self, _id: &str) -> anyhow::Result<()> {
        todo!()
    }
    fn get(&self, _id: &str) -> anyhow::Result<Option<MemoryItem>> {
        todo!()
    }
    fn list(
        &self,
        _scope: Option<Scope>,
        _status: Option<Status>,
    ) -> anyhow::Result<Vec<MemoryItem>> {
        todo!()
    }
    fn archive(&self, _id: &str, _archived: bool) -> anyhow::Result<()> {
        todo!()
    }
    fn export(&self, _out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        todo!()
    }
    fn import(&self, _input: &mut dyn std::io::Read) -> anyhow::Result<usize> {
        todo!()
    }
    fn stats(&self) -> anyhow::Result<serde_json::Value> {
        todo!()
    }
}
