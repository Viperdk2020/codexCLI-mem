use super::*;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// Simple JSONL-backed memory store used for both durable history and recall.
pub struct JsonlMemoryStore {
    path: PathBuf,
}

impl JsonlMemoryStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    fn read_all(&self) -> anyhow::Result<Vec<MemoryItem>> {
        let data = match std::fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        let mut items = Vec::new();
        for line in data.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(it) = serde_json::from_str::<MemoryItem>(line) {
                items.push(it);
            }
        }
        Ok(items)
    }

    fn write_all(&self, items: &[MemoryItem]) -> anyhow::Result<()> {
        let mut out = String::new();
        for it in items {
            let line = serde_json::to_string(it)?;
            out.push_str(&line);
            out.push('\n');
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, out)?;
        Ok(())
    }
}

impl MemoryStore for JsonlMemoryStore {
    fn add(&self, item: MemoryItem) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut line = serde_json::to_string(&item)?;
        line.push('\n');
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(line.as_bytes())?;
        f.flush()?;
        Ok(())
    }

    fn update(&self, item: &MemoryItem) -> anyhow::Result<()> {
        let mut items = self.read_all()?;
        for it in &mut items {
            if it.id == item.id {
                *it = item.clone();
            }
        }
        self.write_all(&items)
    }

    fn delete(&self, id: &str) -> anyhow::Result<()> {
        let items = self.read_all()?;
        let items: Vec<_> = items.into_iter().filter(|i| i.id != id).collect();
        self.write_all(&items)
    }

    fn get(&self, id: &str) -> anyhow::Result<Option<MemoryItem>> {
        let items = self.read_all()?;
        Ok(items.into_iter().find(|i| i.id == id))
    }

    fn list(
        &self,
        scope: Option<Scope>,
        status: Option<Status>,
    ) -> anyhow::Result<Vec<MemoryItem>> {
        let mut items = self.read_all()?;
        if let Some(sc) = scope {
            items.retain(|i| i.scope == sc);
        }
        if let Some(st) = status {
            items.retain(|i| i.status == st);
        }
        Ok(items)
    }

    fn archive(&self, id: &str, archived: bool) -> anyhow::Result<()> {
        let mut items = self.read_all()?;
        for it in &mut items {
            if it.id == id {
                it.status = if archived {
                    Status::Archived
                } else {
                    Status::Active
                };
            }
        }
        self.write_all(&items)
    }

    fn export(&self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        let items = self.read_all()?;
        for it in items {
            let line = serde_json::to_string(&it)?;
            out.write_all(line.as_bytes())?;
            out.write_all(b"\n")?;
        }
        Ok(())
    }

    fn import(&self, input: &mut dyn std::io::Read) -> anyhow::Result<usize> {
        let mut buf = String::new();
        std::io::Read::read_to_string(input, &mut buf)?;
        let mut items = self.read_all()?;
        let mut count = 0usize;
        for line in buf.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(it) = serde_json::from_str::<MemoryItem>(line) {
                items.push(it);
                count += 1;
            }
        }
        self.write_all(&items)?;
        Ok(count)
    }

    fn stats(&self) -> anyhow::Result<serde_json::Value> {
        let items = self.read_all()?;
        let total = items.len();
        let active = items.iter().filter(|i| i.status == Status::Active).count();
        Ok(serde_json::json!({"total": total, "active": active}))
    }
}
