# Guidance for AI coding assistants

Read `README.md`, section **"For AI assistants (and new contributors): read
this first"**. It has the file map, the invariants the code relies on, the data
model, the API, step-by-step recipes and known gotchas.

The rules most often broken:

- Writes go through `write_tx()` (BEGIN IMMEDIATE); never `pool.begin()`.
- The server enforces the DAG rules (cycles, max height); the UI never does.
- Never edit an applied migration; add a new numbered file.
- Log history with `log_activity`, refresh search with `reindex`, and emit an
  SSE event after commit.
- Markdown is rendered only through `renderMarkdown()`, which sanitizes it.

Before finishing a change:
`cd server && cargo fmt && cargo clippy --all-targets && cargo test` and
`cd web && npx svelte-check --tsconfig ./tsconfig.app.json && npm run build`.
