use crate::types::Kind;
use crate::types::MemoryItem;
use crate::types::Scope;
use crate::types::Status;

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
    store: &dyn crate::store::MemoryStore,
    _prompt: &str,
    ctx: &RecallContext,
) -> anyhow::Result<Vec<MemoryItem>> {
    // Basic implementation: list active repo-scoped memories and return up to
    // `item_cap` items without exceeding `token_cap` characters.
    let mut items = store.list(Some(Scope::Repo), Some(Status::Active))?;
    // Prefer preferences and facts first; stable sort by updated_at desc.
    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    items.retain(|it| matches!(it.kind, Kind::Pref | Kind::Fact));
    if items.len() > ctx.item_cap {
        items.truncate(ctx.item_cap);
    }
    // Rough token cap approximation: 1 token ~ 1 char.
    let mut total = 0usize;
    let mut out = Vec::new();
    for it in items {
        let len = it.content.len();
        if !out.is_empty() && total + len > ctx.token_cap {
            break;
        }
        total += len;
        out.push(it);
    }
    Ok(out)
}
