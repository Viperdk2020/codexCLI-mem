use chrono::Utc;
use serde_json::json;
use std::fs::OpenOptions;
use std::fs::create_dir_all;
use std::io::BufRead;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

use codex_memory::factory;
use codex_memory::recall::RecallContext;
use codex_memory::recall::{self};
use codex_memory::types::Counters;
use codex_memory::types::Kind;
use codex_memory::types::MemoryItem;
use codex_memory::types::RelevanceHints;
use codex_memory::types::Scope;
use codex_memory::types::Status;

pub struct MemoryLogger {
    repo_root: PathBuf,
    memory_dir: PathBuf,
    memory_file: PathBuf,
    session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolInvocation {
    pub server: String,
    pub tool: String,
    pub arguments: Option<serde_json::Value>,
    pub duration: Duration,
    pub success: bool,
    pub result: Option<serde_json::Value>,
}

impl MemoryLogger {
    pub fn new(start_path: PathBuf) -> Self {
        let repo_root = detect_repo_root(&start_path).unwrap_or(start_path);
        let memory_dir = repo_root.join(".codex").join("memory");
        let memory_file = memory_dir.join("memory.jsonl");
        let _ = create_dir_all(&memory_dir);
        Self {
            repo_root,
            memory_dir,
            memory_file,
            session_id: None,
        }
    }

    pub fn set_session_id(&mut self, session_id: Uuid) {
        self.session_id = Some(session_id.to_string());
    }

    fn write_line(&self, value: &serde_json::Value) {
        if let Err(e) = create_dir_all(&self.memory_dir) {
            tracing::debug!("tui memory: create_dir_all failed: {e}");
            return;
        }
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.memory_file)
        {
            Ok(mut f) => {
                if let Ok(s) = serde_json::to_string(value) {
                    let _ = writeln!(f, "{s}");
                }
            }
            Err(e) => tracing::debug!("tui memory: open append failed: {e}"),
        }
    }

    pub fn log_exec(&self, command: &[String], exit_code: i32, duration: Duration, output: &str) {
        let id = Uuid::new_v4().to_string();
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let preview = truncate_multiline(output, 160, 20);
        let value = json!({
            "id": id,
            "ts": ts,
            "repo": self.repo_root.to_string_lossy(),
            "type": "exec",
            "content": shlex::try_join(command.iter().map(|s| s.as_str())).unwrap_or_else(|_| command.join(" ")),
            "tags": ["exec"],
            "files": [],
            "session_id": self.session_id,
            "source": "codex-tui",
            "metadata": {
                "exit_code": exit_code,
                "duration_ms": duration.as_millis() as u64,
                "output_preview": preview,
            }
        });
        self.write_line(&value);
    }

    pub fn log_tool_call(&self, inv: ToolInvocation) {
        let id = Uuid::new_v4().to_string();
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let args_str = inv
            .arguments
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();
        let content = if args_str.is_empty() {
            format!("{}.{}()", inv.server, inv.tool)
        } else {
            format!("{}.{}({})", inv.server, inv.tool, args_str)
        };
        let value = json!({
            "id": id,
            "ts": ts,
            "repo": self.repo_root.to_string_lossy(),
            "type": "tool",
            "content": content,
            "tags": ["tool"],
            "files": [],
            "session_id": self.session_id,
            "source": "codex-tui",
            "metadata": {
                "server": inv.server,
                "tool": inv.tool,
                "success": inv.success,
                "duration_ms": inv.duration.as_millis() as u64,
                "result": inv.result,
            }
        });
        self.write_line(&value);
    }

    pub fn log_patch_apply(
        &self,
        success: bool,
        auto_approved: bool,
        duration: Duration,
        stdout: &str,
        stderr: &str,
        files: &[String],
    ) {
        let id = Uuid::new_v4().to_string();
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let preview = if success { stdout } else { stderr };
        let value = json!({
            "id": id,
            "ts": ts,
            "repo": self.repo_root.to_string_lossy(),
            "type": "change",
            "content": format!("apply_patch(auto_approved={})", auto_approved),
            "tags": ["apply_patch"],
            "files": files,
            "session_id": self.session_id,
            "source": "codex-tui",
            "metadata": {
                "success": success,
                "auto_approved": auto_approved,
                "duration_ms": duration.as_millis() as u64,
                "output_preview": truncate_multiline(preview, 160, 20),
            }
        });
        self.write_line(&value);
    }

    // --- Durable items API ---
    pub fn add_summary(&self, text: &str) -> anyhow::Result<()> {
        let id = Uuid::new_v4().to_string();
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let value = json!({
            "id": id,
            "ts": ts,
            "repo": self.repo_root.to_string_lossy(),
            "type": "summary",
            "content": text,
            "tags": ["summary"],
            "files": [],
            "session_id": self.session_id,
            "source": "codex-tui",
            "metadata": {}
        });
        self.write_line(&value);
        Ok(())
    }

    pub fn log_decision(&self, text: &str, tags: &[&str]) {
        let id = Uuid::new_v4().to_string();
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let value = json!({
            "id": id,
            "ts": ts,
            "repo": self.repo_root.to_string_lossy(),
            "type": "decision",
            "content": text,
            "tags": tags,
            "files": [],
            "session_id": self.session_id,
            "source": "codex-tui",
            "metadata": {}
        });
        self.write_line(&value);
    }

    // Build a short preamble string from durable memory items (prefs/summaries).
    pub fn build_durable_preamble(&self, max_len: usize) -> Option<String> {
        let store = factory::open_repo_store(&self.repo_root, None).ok()?;
        let ctx = RecallContext {
            repo_root: Some(self.repo_root.clone()),
            dir: None,
            current_file: None,
            crate_name: None,
            language: None,
            command: None,
            now_rfc3339: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            item_cap: 16,
            token_cap: max_len * 2,
        };
        let Ok(items) = recall::recall(store.as_ref(), "", &ctx) else {
            return None;
        };
        let mut prefs: Vec<(String, Vec<String>)> = Vec::new();
        let mut summaries: Vec<(String, Vec<String>)> = Vec::new();
        for it in items {
            match it.kind {
                Kind::Pref => prefs.push((it.content, it.tags)),
                Kind::Fact => summaries.push((it.content, it.tags)),
                _ => {}
            }
        }
        if prefs.is_empty() && summaries.is_empty() {
            return None;
        }
        let dedupe = |items: Vec<(String, Vec<String>)>, cap: usize| -> Vec<String> {
            use std::collections::BTreeMap;
            let mut map: BTreeMap<String, (Vec<String>, usize)> = BTreeMap::new();
            for (c, tags) in items {
                let key = c.to_ascii_lowercase();
                let e = map.entry(key).or_insert((Vec::new(), 0));
                for t in tags {
                    if !e.0.contains(&t) {
                        e.0.push(t);
                    }
                }
                e.1 += 1;
            }
            let mut out: Vec<String> = map
                .into_iter()
                .map(|(k, (tags, cnt))| {
                    if cnt > 1 && !tags.is_empty() {
                        format!("{k} (tags: {} ×{cnt})", tags.join(", "))
                    } else if !tags.is_empty() {
                        format!("{k} (tags: {})", tags.join(", "))
                    } else {
                        k
                    }
                })
                .collect();
            if out.len() > cap {
                out.truncate(cap);
            }
            out
        };
        let prefs_out = dedupe(prefs, 8);
        let summaries_out = dedupe(summaries, 6);
        let mut parts: Vec<String> = Vec::new();
        if !prefs_out.is_empty() {
            parts.push(format!(
                "Project preferences:\n- {}",
                prefs_out.join("\n- ")
            ));
        }
        if !summaries_out.is_empty() {
            parts.push(format!("Project facts:\n- {}", summaries_out.join("\n- ")));
        }
        let mut s = parts.join("\n\n");
        if s.len() > max_len {
            s.truncate(max_len);
            s.push_str("\n…");
        }
        Some(format!(
            "Context: The following project memory may be helpful.\n{s}\nPlease follow these preferences and consider these facts."
        ))
    }

    // Minimal durable ops for TUI slash commands.
    pub fn add_pref(&self, text: &str) -> anyhow::Result<()> {
        if sqlite_enabled() {
            #[cfg(feature = "memory-sqlite")]
            {
                let id = Uuid::new_v4().to_string();
                let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let item = MemoryItem {
                    id,
                    created_at: ts.clone(),
                    updated_at: ts,
                    schema_version: 1,
                    source: "codex-tui".to_string(),
                    scope: Scope::Repo,
                    status: Status::Active,
                    kind: Kind::Pref,
                    content: text.to_string(),
                    tags: vec!["pref".to_string()],
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
                };
                let store = factory::open_repo_store(&self.repo_root, None)?;
                store.add(item)?;
                return Ok(());
            }
        }
        let id = Uuid::new_v4().to_string();
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let value = json!({
            "id": id,
            "ts": ts,
            "repo": self.repo_root.to_string_lossy(),
            "type": "pref",
            "content": text,
            "tags": ["pref"],
            "files": [],
            "session_id": self.session_id,
            "source": "codex-tui",
            "metadata": {}
        });
        self.write_line(&value);
        Ok(())
    }

    pub fn list_durable(&self, limit: usize) -> Vec<DurableItem> {
        if sqlite_enabled() {
            #[cfg(feature = "memory-sqlite")]
            {
                let Ok(store) = factory::open_repo_store(&self.repo_root, None) else {
                    return vec![];
                };
                let Ok(mut items) = store.list(Some(Scope::Repo), Some(Status::Active)) else {
                    return vec![];
                };
                // Only preferences and summaries (facts)
                items.retain(|i| matches!(i.kind, Kind::Pref | Kind::Fact));
                items.truncate(limit);
                return items
                    .into_iter()
                    .map(|i| DurableItem {
                        id: i.id,
                        r#type: match i.kind {
                            Kind::Pref => "pref".to_string(),
                            _ => "summary".to_string(),
                        },
                        content: i.content,
                    })
                    .collect();
            }
        }
        let Ok(file) = std::fs::File::open(&self.memory_file) else {
            return vec![];
        };
        let reader = std::io::BufReader::new(file);
        let mut items: Vec<DurableItem> = reader
            .lines()
            .flatten()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
            .filter_map(|v| DurableItem::from_value(&v))
            .collect();
        if items.len() > limit {
            items.drain(0..items.len().saturating_sub(limit));
        }
        items
    }

    pub fn search_durable(&self, query: &str, limit: usize) -> Vec<DurableItem> {
        if sqlite_enabled() {
            #[cfg(feature = "memory-sqlite")]
            {
                let q = query.to_ascii_lowercase();
                return self
                    .list_durable(usize::MAX)
                    .into_iter()
                    .filter(|i| i.content.to_ascii_lowercase().contains(&q))
                    .take(limit)
                    .collect();
            }
        }
        let q = query.to_ascii_lowercase();
        self.list_durable(usize::MAX)
            .into_iter()
            .filter(|i| i.content.to_ascii_lowercase().contains(&q))
            .take(limit)
            .collect()
    }

    pub fn list_durable_tagged(&self, limit: usize, tag: &str) -> Vec<DurableItem> {
        if sqlite_enabled() {
            #[cfg(feature = "memory-sqlite")]
            {
                let t = tag.to_ascii_lowercase();
                let Ok(store) = factory::open_repo_store(&self.repo_root, None) else {
                    return vec![];
                };
                let Ok(mut items) = store.list(Some(Scope::Repo), Some(Status::Active)) else {
                    return vec![];
                };
                items.retain(|i| {
                    i.tags.iter().any(|x| x.eq_ignore_ascii_case(&t))
                        && matches!(i.kind, Kind::Pref | Kind::Fact)
                });
                items.truncate(limit);
                return items
                    .into_iter()
                    .map(|i| DurableItem {
                        id: i.id,
                        r#type: match i.kind {
                            Kind::Pref => "pref".to_string(),
                            _ => "summary".to_string(),
                        },
                        content: i.content,
                    })
                    .collect();
            }
        }
        let t = tag.to_ascii_lowercase();
        let Ok(file) = std::fs::File::open(&self.memory_file) else {
            return vec![];
        };
        let reader = std::io::BufReader::new(file);
        let mut items: Vec<DurableItem> = reader
            .lines()
            .flatten()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
            .filter_map(|v| {
                let has_tag = v
                    .get("tags")
                    .and_then(|x| x.as_array())
                    .map(|arr| {
                        arr.iter().any(|t0| {
                            t0.as_str()
                                .map(|s| s.eq_ignore_ascii_case(&t))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false);
                if has_tag {
                    DurableItem::from_value(&v)
                } else {
                    None
                }
            })
            .collect();
        if items.len() > limit {
            items.drain(0..items.len().saturating_sub(limit));
        }
        items
    }

    pub fn delete_by_prefix(&self, prefix: &str) -> bool {
        if sqlite_enabled() {
            #[cfg(feature = "memory-sqlite")]
            {
                if prefix.is_empty() {
                    return false;
                }
                if let Ok(store) = factory::open_repo_store(&self.repo_root, None)
                    && let Ok(items) = store.list(Some(Scope::Repo), None)
                {
                    let mut changed = false;
                    for it in items {
                        if (matches!(it.kind, Kind::Pref | Kind::Fact)) && it.id.starts_with(prefix)
                        {
                            let _ = store.delete(&it.id);
                            changed = true;
                        }
                    }
                    return changed;
                }
                return false;
            }
        }
        if prefix.is_empty() {
            return false;
        }
        let Ok(file) = std::fs::File::open(&self.memory_file) else {
            return false;
        };
        let reader = std::io::BufReader::new(&file);
        let lines: Vec<String> = reader.lines().flatten().collect();
        let mut changed = false;
        let mut out: Vec<String> = Vec::with_capacity(lines.len());
        for line in lines {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
                let is_durable = t == "pref" || t == "summary";
                let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                if is_durable && id.starts_with(prefix) {
                    changed = true;
                    continue;
                }
            }
            out.push(line);
        }
        if changed
            && let Ok(mut f) = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&self.memory_file)
        {
            let _ = writeln!(f, "{}", out.join("\n"));
        }
        changed
    }
}

#[derive(Clone)]
pub(crate) struct DurableItem {
    pub id: String,
    pub r#type: String,
    pub content: String,
}
impl DurableItem {
    fn from_value(v: &serde_json::Value) -> Option<Self> {
        let t = v.get("type")?.as_str()?;
        if t != "pref" && t != "summary" {
            return None;
        }
        Some(DurableItem {
            id: v.get("id")?.as_str()?.to_string(),
            r#type: t.to_string(),
            content: v.get("content")?.as_str()?.to_string(),
        })
    }
}

fn truncate_multiline(text: &str, max_chars: usize, max_lines: usize) -> String {
    let mut s: String = text.lines().take(max_lines).collect::<Vec<_>>().join("\n");
    if s.len() > max_chars {
        s.truncate(max_chars);
        s.push('…');
    }
    s
}

fn detect_repo_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.canonicalize().unwrap_or(start.to_path_buf());
    for _ in 0..64 {
        if cur.join(".git").exists() || cur.join(".codex").exists() {
            return Some(cur);
        }
        if let Some(parent) = cur.parent() {
            cur = parent.to_path_buf();
        } else {
            break;
        }
    }
    None
}

fn sqlite_enabled() -> bool {
    #[cfg(feature = "memory-sqlite")]
    {
        match std::env::var("CODEX_MEMORY_BACKEND") {
            Ok(v) if v.eq_ignore_ascii_case("sqlite") => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_jsonl<P: AsRef<Path>>(path: P, lines: &[serde_json::Value]) {
        let mut s = String::new();
        for v in lines {
            s.push_str(&serde_json::to_string(v).unwrap());
            s.push('\n');
        }
        fs::write(path, s).unwrap();
    }

    #[test]
    fn preamble_dedupes_merges_and_caps() {
        let dir = tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        fs::create_dir_all(repo.join(".codex").join("memory")).unwrap();
        let store = factory::open_repo_store(&repo, None).unwrap();
        let ts = "2025-01-01T00:00:00.000Z".to_string();
        let item = MemoryItem {
            id: "1".into(),
            created_at: ts.clone(),
            updated_at: ts.clone(),
            schema_version: 1,
            source: "test".into(),
            scope: Scope::Repo,
            status: Status::Active,
            kind: Kind::Pref,
            content: "prefer ruff".into(),
            tags: vec!["python".into(), "style".into()],
            relevance_hints: RelevanceHints { files: vec![], crates: vec![], languages: vec![], commands: vec![] },
            counters: Counters { seen_count: 0, used_count: 0, last_used_at: None },
            expiry: None,
        };
        store.add(item).unwrap();
        let mut item2 = store.get("1").unwrap().unwrap();
        item2.id = "2".into();
        item2.content = "Prefer Ruff".into();
        item2.tags = vec!["style".into()];
        store.add(item2).unwrap();
        let mut fact = store.get("1").unwrap().unwrap();
        fact.id = "3".into();
        fact.kind = Kind::Fact;
        fact.content = "uses pytest".into();
        fact.tags = vec!["python".into()];
        store.add(fact).unwrap();
        let mut fact2 = store.get("3").unwrap().unwrap();
        fact2.id = "4".into();
        fact2.content = "Uses PyTest".into();
        fact2.tags = vec!["ci".into()];
        store.add(fact2).unwrap();

        let mut logger = MemoryLogger::new(repo.clone());
        logger.session_id = Some("test".into());
        let pre = logger.build_durable_preamble(512).expect("preamble");

        // Expect deduped entries each appearing once and mention of tags.
        assert!(pre.to_ascii_lowercase().contains("project preferences"));
        assert!(pre.to_ascii_lowercase().contains("project facts"));
        // The two Ruff prefs should merge into one line (case-insensitive dedupe) and include tags
        assert!(pre.to_ascii_lowercase().contains("prefer ruff"));
        // The two pytest summaries should merge
        assert!(pre.to_ascii_lowercase().contains("uses pytest"));
    }
}
