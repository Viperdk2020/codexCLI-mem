# Codex Memory — Phase 1–2 (Local‑First)

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

## Phase 2 (Optional SQLite backend)

Why: JSONL is simple and diff‑friendly, but updates require rewriting the file and queries get slower over time. An optional SQLite backend provides atomic updates, indexes, and faster listing/filters — while keeping JSONL as the default.

What’s included (feature‑gated behind `codex-memory/sqlite`):
- `SqliteMemoryStore` implementing the same `MemoryStore` trait.
- Lightweight schema with JSON columns for `tags`, `relevance_hints`, `counters`, and `expiry`.
- Import/Export from/to JSONL lines for easy backup and migration.
- `stats()` for quick counts by scope and status.

Selecting a backend
- Build feature: enable with `--features codex-memory/sqlite` when building/running dependent crates.
- Runtime env (optional):
  - `CODEX_MEMORY_BACKEND=sqlite|jsonl` (defaults to `jsonl`).
  - Repo paths: `CODEX_MEMORY_REPO_DB` or `CODEX_MEMORY_REPO_JSONL`.
  - Home paths: `CODEX_MEMORY_HOME_DB` or `CODEX_MEMORY_HOME_JSONL`.

Helper factory
- `codex_memory::factory::{open_repo_store, open_global_store, choose_backend_from_env}` selects the backend and standard paths (repo: `<repo>/.codex/memory/…`, home: `~/.codex/memory/…`).

Migration
- API: `codex_memory::migrate::migrate_jsonl_to_sqlite(jsonl_path, sqlite_path)` (only when built with `sqlite`).
- One‑shot: call migrate once, then set `CODEX_MEMORY_BACKEND=sqlite`. Both import/export remain available to move between formats.

Notes
- rusqlite is compiled with the bundled SQLite (`libsqlite3-sys/bundled`) to avoid system dependencies.
- JSONL remains the default path; no behavior change unless the feature/env is set.
