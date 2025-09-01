use crate::store::MemoryStore;
use crate::types::MemoryItem;
use crate::types::Status;
use chrono::DateTime;
use chrono::Utc;
use std::collections::BTreeSet;

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
    store: &dyn MemoryStore,
    prompt: &str,
    ctx: &RecallContext,
) -> anyhow::Result<Vec<MemoryItem>> {
    let now = DateTime::parse_from_rfc3339(&ctx.now_rfc3339)?.with_timezone(&Utc);
    let tokens = tokenize(prompt);
    let mut scored: Vec<(f32, usize, MemoryItem)> = store
        .list(None, Some(Status::Active))?
        .into_iter()
        .map(|item| {
            let mut score = overlap_score(&tokens, &tokenize(&item.content));
            if let Some(f) = &ctx.current_file
                && item.relevance_hints.files.iter().any(|h| f.ends_with(h))
            {
                score += 0.4;
            }
            if let Some(c) = &ctx.crate_name
                && item.relevance_hints.crates.iter().any(|h| h == c)
            {
                score += 0.3;
            }
            if let Some(l) = &ctx.language
                && item
                    .relevance_hints
                    .languages
                    .iter()
                    .any(|h| h.eq_ignore_ascii_case(l))
            {
                score += 0.2;
            }
            if let Some(cmd) = &ctx.command
                && item.relevance_hints.commands.iter().any(|h| h == cmd)
            {
                score += 0.1;
            }
            let freq = 1.0 + item.counters.used_count as f32 * 0.1;
            score *= freq;
            if let Some(last) = &item.counters.last_used_at
                && let Ok(dt) = DateTime::parse_from_rfc3339(last)
            {
                let age_days = (now - dt.with_timezone(&Utc)).num_days();
                let decay = 0.5f32.powf(age_days as f32 / 7.0);
                score *= decay;
            }
            let token_len = item.content.split_whitespace().count();
            (score, token_len, item)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    let mut used_tokens = 0usize;
    for (_, tokens, mut item) in scored {
        if out.len() >= ctx.item_cap {
            break;
        }
        if used_tokens + tokens > ctx.token_cap {
            break;
        }
        used_tokens += tokens;
        item.counters.used_count += 1;
        item.counters.last_used_at = Some(ctx.now_rfc3339.clone());
        store.update(&item)?;
        out.push(item);
    }
    Ok(out)
}

fn tokenize(s: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for w in s.split(|c: char| !c.is_alphanumeric()) {
        if w.is_empty() {
            continue;
        }
        set.insert(w.to_ascii_lowercase());
    }
    set
}

fn overlap_score(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f32;
    inter / a.len() as f32
}
