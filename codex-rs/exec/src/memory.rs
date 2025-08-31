use chrono::Utc;
use serde_json::json;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

/// Minimal per-repo memory logger that writes JSONL entries to
/// `<repo>/.codex/memory/memory.jsonl`.
pub(crate) struct MemoryLogger {
    repo_root: PathBuf,
    memory_dir: PathBuf,
    memory_file: PathBuf,
    index_file: PathBuf,
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
        let index_file = memory_dir.join("index.json");
        // Best-effort create, ignore errors here; we'll handle on write.
        let _ = create_dir_all(&memory_dir);
        Self {
            repo_root,
            memory_dir,
            memory_file,
            index_file,
        }
    }

    fn write_line(&self, value: &serde_json::Value) {
        if let Err(e) = create_dir_all(&self.memory_dir) {
            tracing::debug!("memory: create_dir_all failed: {e}");
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
            Err(e) => {
                tracing::debug!("memory: open append failed: {e}");
            }
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
            "content": format!("{}", shlex::try_join(command.iter().map(|s| s.as_str())).unwrap_or_else(|_| command.join(" "))),
            "tags": ["exec"],
            "files": [],
            "session_id": null,
            "source": "codex-rs",
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
            "session_id": null,
            "source": "codex-rs",
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
            "session_id": null,
            "source": "codex-rs",
            "metadata": {
                "success": success,
                "auto_approved": auto_approved,
                "duration_ms": duration.as_millis() as u64,
                "output_preview": truncate_multiline(preview, 160, 20),
            }
        });
        self.write_line(&value);
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

