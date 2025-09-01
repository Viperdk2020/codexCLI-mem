# Memory backends

Codex stores per-repo state so you and the CLI can recall decisions across sessions. Two storage backends are available.

## JSONL (default)

- One JSON object per line, easy to inspect and back up.
- Paths: `<repo>/.codex/memory/memory.jsonl` or `~/.codex/memory/memory.jsonl`.
- Works out of the box and is diff‑friendly for version control.

## SQLite (optional)

- Adds atomic updates and indexes for faster queries.
- Paths: `<repo>/.codex/memory/memory.db` or `~/.codex/memory/memory.db`.
- Requires a build with the SQLite feature (`--features codex-memory/sqlite`).
- Select at runtime with `CODEX_MEMORY_BACKEND=sqlite` (defaults to `jsonl`).

## Migrating existing data

Convert an existing JSONL store to SQLite:

```bash
codex memory migrate
```

After migration, enable SQLite with `CODEX_MEMORY_BACKEND=sqlite`.

## Compacting

Reclaim space and keep the store tidy:

```bash
codex memory compact
```

This vacuums a SQLite database or rewrites a JSONL file to drop unused entries.

Unsetting `CODEX_MEMORY_BACKEND` returns Codex to the default JSONL backend.
