# Codex Memory — Phase 1 Design (Local-First, ChatGPT-Style)

Goals
- Persistent, user-controlled memories that improve replies across sessions.
- Clear CRUD: add, list, update, delete; temporary chat bypasses memory.
- Smart recall: relevance + recency + frequency; small, capped preamble.
- Privacy: local-first, easy review/export, basic secret redaction.

Data Model (JSONL for Phase 1)
- id: UUID (string)
- created_at, updated_at: RFC3339
- scope: `global` | `repo` | `dir`
- status: `active` | `archived`
- type: `pref` | `fact` | `instruction` | `profile` | `note`
- content: short natural-language sentence (<= 240 chars)
- tags: [string]
- relevance_hints: { files?: [path], crates?: [string], langs?: ["rust", ...] }
- counters: { seen_count, used_count, last_used_at? }
- expiry?: RFC3339 or duration hint

Recall Algorithm (Deterministic)
1) Candidate set from `repo` scope; if fewer than K, include `global`.
2) Token overlap score against current prompt + hints overlap (file/crate/lang).
3) Recency decay + frequency boosts.
4) Select top N, render a concise preamble (token-capped).

API Sketch (codex-core)
- MemoryStore trait with JSONL implementation.
- CRUD methods and `recall_for(prompt, ctx)` scoring.
- No embeddings in Phase 1 (possible Phase 3 behind a feature flag).

CLI/TUI (Future PRs)
- `codex memory add|list|edit|rm|archive|export|import`.
- TUI "Memories" panel, preamble preview, inline quick-save.

Safety
- Refuse to save likely secrets (regex/entropy); allow `--force`.
- Local files only; export/import JSON.

Migration
- Read existing `~/.codex/memory/*.jsonl` and repo `.codex/memory/memory.jsonl`.
- Reclassify obvious exec/apply_patch entries as non-recallable; keep as history.

Testing
- Unit tests for scoring, TTL/archival, and redaction.
- Snapshot tests for TUI when integrated.

Out of Scope (Phase 1)
- Embeddings/vector DB; shared/org memory; remote sync.

