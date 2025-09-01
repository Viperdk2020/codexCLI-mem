use crate::types::MemoryItem;

pub struct RecallContext {
    pub repo_root: Option<std::path::PathBuf>,
    pub dir: Option<std::path::PathBuf>,
    pub current_file: Option<String>,
    pub crate_name: Option<String>,
    pub language: Option<String>,
    pub command: Option<String>,
    pub now_rfc3339: String,
    pub item_cap: usize,
    pub token_cap: usize,
}

pub fn recall(
    _store: &dyn crate::store::MemoryStore,
    _prompt: &str,
    _ctx: &RecallContext,
) -> anyhow::Result<Vec<MemoryItem>> {
    todo!()
}
