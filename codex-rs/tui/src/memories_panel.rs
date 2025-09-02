use chrono::Utc;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use uuid::Uuid;

use codex_memory::factory;
use codex_memory::types::Counters;
use codex_memory::types::Kind;
use codex_memory::types::MemoryItem;
use codex_memory::types::RelevanceHints;
use codex_memory::types::Scope;
use codex_memory::types::Status;

/// Simple panel showing stored memories and exposing minimal CRUD ops.
pub struct MemoriesPanel {
    repo_root: std::path::PathBuf,
    items: Vec<MemoryItem>,
}

impl MemoriesPanel {
    pub fn new(repo_root: std::path::PathBuf) -> anyhow::Result<Self> {
        let mut panel = Self {
            repo_root,
            items: Vec::new(),
        };
        panel.refresh()?;
        Ok(panel)
    }

    /// Reload items from the repo store.
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let store = factory::open_repo_store(&self.repo_root, None)?;
        self.items = store.list(Some(Scope::Repo), Some(Status::Active))?;
        Ok(())
    }

    /// Add a new preference memory entry.
    pub fn add_pref(&mut self, text: &str) -> anyhow::Result<()> {
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let item = MemoryItem {
            id: Uuid::new_v4().to_string(),
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
        self.refresh()
    }

    /// Delete a memory item by id.
    pub fn delete(&mut self, id: &str) -> anyhow::Result<()> {
        let store = factory::open_repo_store(&self.repo_root, None)?;
        store.delete(id)?;
        self.refresh()
    }
}

impl WidgetRef for &MemoriesPanel {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(self.items.len() + 1);
        lines.push(Line::raw("Memories:"));
        for it in &self.items {
            lines.push(Line::raw(format!("- {}", it.content)));
        }
        let para = Paragraph::new(lines);
        para.render(area, buf);
    }
}
