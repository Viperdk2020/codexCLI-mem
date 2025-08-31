use chrono::Utc;
use serde_json::json;
use std::fs::create_dir_all;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct MemoryEntry {
    pub id: String,
    pub r#type: String,
    pub content: String,
}

#[derive(Clone)]
pub struct MemoryLogger {
    repo_root: PathBuf,
    memory_dir: PathBuf,
    pub memory_file: PathBuf,
    session_id: Option<String>,
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

    pub fn set_session_id(&mut self, id: Uuid) {
        self.session_id = Some(id.to_string());
    }

    fn write_line(&self, value: &serde_json::Value) {
        if let Err(e) = create_dir_all(&self.memory_dir) {
            tracing::debug!("gui memory: create_dir_all failed: {e}");
            return;
        }
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.memory_file)
        {
            Ok(mut f) => {
                if let Ok(s) = serde_json::to_string(value) {
                    let _ = writeln!(f, "{}", s);
                }
            }
            Err(e) => tracing::debug!("gui memory: open append failed: {e}"),
        }
    }

    pub fn add_pref(&self, text: &str) -> anyhow::Result<()> {
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
            "source": "codex-gui",
            "metadata": {}
        });
        self.write_line(&value);
        Ok(())
    }

    pub fn build_durable_preamble(&self, max_len: usize) -> Option<String> {
        let path = self.memory_file.as_path();
        let Ok(file) = File::open(path) else {
            return None;
        };
        let reader = std::io::BufReader::new(file);
        let mut prefs: Vec<(String, Vec<String>)> = Vec::new();
        let mut summaries: Vec<(String, Vec<String>)> = Vec::new();
        for line in reader.lines().flatten() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
                if t == "pref" || t == "summary" {
                    let c = v
                        .get("content")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tags: Vec<String> = v
                        .get("tags")
                        .and_then(|x| x.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    if t == "pref" {
                        prefs.push((c, tags));
                    } else {
                        summaries.push((c, tags));
                    }
                }
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
        Some(format!("Context: The following project memory may be helpful.\n{}\nPlease follow these preferences and consider these facts.", s))
    }
}

pub fn read_memory_items(path: &Path, limit: usize) -> (Vec<MemoryEntry>, Vec<MemoryEntry>) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (vec![], vec![]),
    };
    let reader = std::io::BufReader::new(file);
    let mut durable = Vec::new();
    let mut recent = Vec::new();
    for line in reader.lines().flatten() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            let t = v
                .get("type")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let id = v
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let content = v
                .get("content")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            match t.as_str() {
                "pref" | "summary" | "decision" => durable.push(MemoryEntry {
                    id,
                    r#type: t,
                    content,
                }),
                "exec" | "tool" | "change" => recent.push(MemoryEntry {
                    id,
                    r#type: t,
                    content,
                }),
                _ => {}
            }
        }
    }
    if durable.len() > limit {
        durable.drain(0..durable.len().saturating_sub(limit));
    }
    if recent.len() > limit {
        recent.drain(0..recent.len().saturating_sub(limit));
    }
    (durable, recent)
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
