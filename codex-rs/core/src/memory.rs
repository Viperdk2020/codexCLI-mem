//! Local-first "Memory" scaffolding inspired by ChatGPT-style memories.
//! Phase 1: deterministic, JSONL-backed store with CRUD and simple recall.

use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    Global,
    Repo,
    Dir,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Pref,
    Fact,
    Instruction,
    Profile,
    Note,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelevanceHints {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub crates: Vec<String>,
    #[serde(default)]
    pub langs: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Counters {
    pub seen_count: u64,
    pub used_count: u64,
    pub last_used_at: Option<String>, // RFC3339
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: String,
    pub created_at: String, // RFC3339
    pub updated_at: String, // RFC3339
    pub scope: MemoryScope,
    pub status: MemoryStatus,
    pub r#type: MemoryType,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub relevance_hints: RelevanceHints,
    #[serde(default)]
    pub counters: Counters,
    pub expiry: Option<String>, // RFC3339
}

impl MemoryItem {
    pub fn new(scope: MemoryScope, r#type: MemoryType, content: String) -> Self {
        let now = now_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: now.clone(),
            updated_at: now,
            scope,
            status: MemoryStatus::Active,
            r#type,
            content,
            tags: vec![],
            relevance_hints: RelevanceHints::default(),
            counters: Counters::default(),
            expiry: None,
        }
    }
}

fn now_rfc3339() -> String {
    // Use time crate via std time formatting fallback to RFC3339
    // Keep dependencies minimal by using chrono-like formatting through time::OffsetDateTime if present.
    // Here we rely on std + humantime for simplicity-free build: format seconds precision.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // RFC3339-like UTC with seconds only.
    chrono_like_utc(ts).to_string()
}

fn chrono_like_utc(secs: u64) -> String {
    // Simple UTC seconds -> RFC3339 (YYYY-MM-DDTHH:MM:SSZ) without extra deps.
    // This is not DST-sensitive and good enough for logs.
    use std::fmt::Write as _;
    const SECS_PER_MIN: i64 = 60;
    const SECS_PER_HOUR: i64 = 3600;
    const SECS_PER_DAY: i64 = 86400;

    let t = secs as i64;
    let days = t / SECS_PER_DAY;
    let rem = t % SECS_PER_DAY;
    let hours = rem / SECS_PER_HOUR;
    let rem = rem % SECS_PER_HOUR;
    let mins = rem / SECS_PER_MIN;
    let secs = rem % SECS_PER_MIN;

    // Convert days since epoch to date using a simple algorithm (Unix epoch 1970-01-01).
    // For accuracy and brevity, fall back to chrono-like formatting via time crate if present later.
    let (y, m, d) = days_to_ymd(days);
    let mut s = String::with_capacity(20);
    let _ = write!(s, "{y:04}-{m:02}-{d:02}T{hours:02}:{mins:02}:{secs:02}Z");
    s
}

fn days_to_ymd(days_since_epoch: i64) -> (i32, u32, u32) {
    // Algorithm from Howard Hinnant's civil calendar conversions.
    let z = days_since_epoch + 719468; // shift to civil from 1970-01-01
    let era = (z >= 0).then_some(z).unwrap_or(z - 146096) / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = (yoe as i32) + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100 + yoe / 400); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = mp + if mp < 10 { 3 } else { -9 }; // [1, 12]
    (y + (m <= 2) as i32, m as u32, d as u32)
}

#[derive(Debug, Clone, Default)]
pub struct RecallContext<'a> {
    pub prompt: &'a str,
    pub current_file: Option<&'a str>,
    pub current_crate: Option<&'a str>,
    pub lang: Option<&'a str>,
    pub want: usize, // how many items to recall
}

pub trait MemoryStore: Send + Sync {
    fn add(&self, item: MemoryItem) -> std::io::Result<()>;
    fn update(&self, item: &MemoryItem) -> std::io::Result<()>;
    fn remove(&self, id: &str) -> std::io::Result<()>;
    fn list(&self, scope_filter: Option<MemoryScope>) -> std::io::Result<Vec<MemoryItem>>;
    fn recall(&self, ctx: &RecallContext) -> std::io::Result<Vec<MemoryItem>>;
}

/// JSONL-backed memory store. Each line encodes one `MemoryItem`.
#[derive(Debug, Clone)]
pub struct JsonlMemoryStore {
    path: PathBuf,
}

impl JsonlMemoryStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    fn read_all(&self) -> std::io::Result<Vec<MemoryItem>> {
        let data = match std::fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let mut items = Vec::new();
        for line in data.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(item) = serde_json::from_str::<MemoryItem>(line) {
                items.push(item);
            }
        }
        Ok(items)
    }

    fn write_all(&self, items: &[MemoryItem]) -> std::io::Result<()> {
        let mut out = String::new();
        for it in items {
            let line = serde_json::to_string(it)
                .map_err(|e| std::io::Error::other(format!("serialize memory: {e}")))?;
            out.push_str(&line);
            out.push('\n');
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, out)
    }
}

impl MemoryStore for JsonlMemoryStore {
    fn add(&self, item: MemoryItem) -> std::io::Result<()> {
        // append-only add
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut line = serde_json::to_string(&item)
            .map_err(|e| std::io::Error::other(format!("serialize memory: {e}")))?;
        line.push('\n');
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(line.as_bytes())?;
        f.flush()
    }

    fn update(&self, item: &MemoryItem) -> std::io::Result<()> {
        let mut items = self.read_all()?;
        for it in &mut items {
            if it.id == item.id {
                *it = item.clone();
            }
        }
        self.write_all(&items)
    }

    fn remove(&self, id: &str) -> std::io::Result<()> {
        let items = self.read_all()?;
        let items: Vec<_> = items.into_iter().filter(|i| i.id != id).collect();
        self.write_all(&items)
    }

    fn list(&self, scope_filter: Option<MemoryScope>) -> std::io::Result<Vec<MemoryItem>> {
        let mut v = self.read_all()?;
        if let Some(scope) = scope_filter {
            v.retain(|i| i.scope == scope);
        }
        Ok(v)
    }

    fn recall(&self, ctx: &RecallContext) -> std::io::Result<Vec<MemoryItem>> {
        let mut items = self.read_all()?;
        // Only active items
        items.retain(|i| i.status == MemoryStatus::Active);

        // Score map
        let mut scores: HashMap<String, f32> = HashMap::new();
        let tokens = tokenize(ctx.prompt);

        for it in &items {
            let mut s = 0.0f32;
            // Simple token overlap
            s += overlap_score(&tokens, &tokenize(&it.content));
            // Type boost for preferences/instructions
            s += match it.r#type {
                MemoryType::Pref | MemoryType::Instruction => 0.3,
                _ => 0.0,
            };
            // Hints: file/crate/lang
            if let Some(f) = ctx.current_file
                && it.relevance_hints.files.iter().any(|h| f.ends_with(h))
            {
                s += 0.4;
            }
            if let Some(c) = ctx.current_crate
                && it.relevance_hints.crates.iter().any(|h| h == c)
            {
                s += 0.3;
            }
            if let Some(l) = ctx.lang
                && it
                    .relevance_hints
                    .langs
                    .iter()
                    .any(|h| h.eq_ignore_ascii_case(l))
            {
                s += 0.2;
            }
            // Tags light boost when overlapping prompt tokens
            if it
                .tags
                .iter()
                .any(|t| tokens.contains(&t.to_ascii_lowercase()))
            {
                s += 0.1;
            }
            scores.insert(it.id.clone(), s);
        }

        // sort by score desc, updated_at desc (string compare OK for RFC3339)
        items.sort_by(|a, b| {
            let sa = scores.get(&a.id).cloned().unwrap_or(0.0);
            let sb = scores.get(&b.id).cloned().unwrap_or(0.0);
            sb.partial_cmp(&sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });

        let n = ctx.want.max(1).min(8);
        let now = now_rfc3339();
        let mut out: Vec<MemoryItem> = Vec::new();
        let mut changed = false;
        for it in items.iter_mut().take(n) {
            it.counters.used_count = it.counters.used_count.saturating_add(1);
            it.counters.last_used_at = Some(now.clone());
            it.updated_at = now.clone();
            changed = true;
            out.push(it.clone());
        }
        if changed {
            self.write_all(&items)?;
        }
        Ok(out)
    }
}

fn tokenize(s: &str) -> BTreeSet<String> {
    let mut m = BTreeSet::new();
    for w in s.split(|c: char| !c.is_alphanumeric()) {
        if w.is_empty() {
            continue;
        }
        let w = w.to_ascii_lowercase();
        m.insert(w);
    }
    m
}

fn overlap_score(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f32;
    inter / (a.len() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_recall_basic() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonlMemoryStore::new(dir.path().join("mem.jsonl"));
        store
            .add(MemoryItem::new(
                MemoryScope::Global,
                MemoryType::Pref,
                "Use rg for search".to_string(),
            ))
            .unwrap();
        store
            .add(MemoryItem::new(
                MemoryScope::Repo,
                MemoryType::Instruction,
                "Run just fmt before PR".to_string(),
            ))
            .unwrap();

        let ctx = RecallContext {
            prompt: "please search with rg",
            current_file: None,
            current_crate: None,
            lang: None,
            want: 3,
        };
        let out = store.recall(&ctx).unwrap();
        assert!(!out.is_empty());
        assert!(out.iter().any(|i| i.content.contains("rg")));
    }

    #[test]
    fn counters_persist_across_recall() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mem.jsonl");
        let store = JsonlMemoryStore::new(&path);
        store
            .add(MemoryItem::new(
                MemoryScope::Global,
                MemoryType::Pref,
                "Remember me".to_string(),
            ))
            .unwrap();

        let ctx = RecallContext {
            prompt: "remember",
            current_file: None,
            current_crate: None,
            lang: None,
            want: 1,
        };

        // First recall updates counters
        let out1 = store.recall(&ctx).unwrap();
        assert_eq!(out1[0].counters.used_count, 1);
        let last1 = out1[0].counters.last_used_at.clone();
        assert!(last1.is_some());

        // Second recall reads persisted counters
        let store2 = JsonlMemoryStore::new(&path);
        let out2 = store2.recall(&ctx).unwrap();
        assert_eq!(out2[0].counters.used_count, 2);
        let last2 = out2[0].counters.last_used_at.clone();
        assert!(last2.is_some());
        if let (Some(a), Some(b)) = (last1, last2) {
            assert!(b >= a);
        }
    }
}
