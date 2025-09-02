use codex_tui::memories_panel::MemoriesPanel;
use codex_tui::memory::MemoryLogger;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::widgets::WidgetRef;
use tempfile::tempdir;

#[test]
fn panel_renders() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".codex")).unwrap();
    let mut panel = MemoriesPanel::new(dir.path().to_path_buf()).unwrap();
    panel.add_pref("Use ripgrep for search").unwrap();
    panel.add_pref("Avoid force pushes").unwrap();
    panel.refresh().unwrap();

    let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
    terminal
        .draw(|f| (&panel).render_ref(f.area(), f.buffer_mut()))
        .unwrap();
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn preamble_preview() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".codex")).unwrap();
    let repo = dir.path().to_path_buf();
    let store = codex_memory::factory::open_repo_store(&repo, None).unwrap();
    let ts = "2025-01-01T00:00:00.000Z".to_string();
    let pref = codex_memory::types::MemoryItem {
        id: "1".into(),
        created_at: ts.clone(),
        updated_at: ts.clone(),
        schema_version: 1,
        source: "test".into(),
        scope: codex_memory::types::Scope::Repo,
        status: codex_memory::types::Status::Active,
        kind: codex_memory::types::Kind::Pref,
        content: "Respect editorconfig".into(),
        tags: vec![],
        relevance_hints: codex_memory::types::RelevanceHints {
            files: vec![],
            crates: vec![],
            languages: vec![],
            commands: vec![],
        },
        counters: codex_memory::types::Counters {
            seen_count: 0,
            used_count: 0,
            last_used_at: None,
        },
        expiry: None,
    };
    store.add(pref).unwrap();
    let mut fact = store.get("1").unwrap().unwrap();
    fact.id = "2".into();
    fact.kind = codex_memory::types::Kind::Fact;
    fact.content = "Tests use cargo test".into();
    store.add(fact).unwrap();
    let logger = MemoryLogger::new(repo);
    let pre = logger.build_durable_preamble(512).unwrap();
    insta::assert_snapshot!(pre);
}
