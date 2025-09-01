# Memory backends

Codex stores per-repository memory to help recall decisions and context. Two storage formats are available:

## JSONL (default)

- One line per entry at `.codex/memory/memory.jsonl` inside each repo.
- Human-readable and easy to back up or edit with standard tools.

## SQLite (optional)

- Stores entries in `.codex/memory/memory.db`.
- Offers atomic updates, indexing and faster queries.
- Requires a build with SQLite support and enabling at runtime.

### Enabling SQLite

1. Ensure your Codex build includes the `sqlite` feature (pre-built binaries include it).
2. Set `CODEX_MEMORY_BACKEND=sqlite` in the environment.

### Migrating existing data

Convert an existing JSONL store to SQLite:

```bash
codex memory migrate
```

This reads `memory.jsonl` and writes `memory.db` in the same directory.

### Compaction

To reclaim free space in the SQLite database:

```bash
codex memory compact
```

This runs `VACUUM` on `memory.db`.

Unsetting `CODEX_MEMORY_BACKEND` returns Codex to the default JSONL backend.
