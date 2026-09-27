# Guidance for AI coding assistants

Read `README.md`, section **"For AI assistants (and new contributors): read
this first"**. It has the file map, the invariants the code relies on, the data
model, the API, step-by-step recipes and known gotchas.

The rules most often broken:

- Writes go through `write_tx()` (BEGIN IMMEDIATE); never `pool.begin()`.
- The server enforces the DAG rules (cycles, max height); the UI never does.
- Every route needs a `Principal`, gotten via `Auth`, `inproc::call`, or a
  test's direct attachment — never a header the caller controls.
- New endpoints must call `require_writable`/`require`/`require_ticket`/
  `require_site_admin` explicitly; nothing is accessible by default.
- The DAG *check* always runs against the true board-wide graph
  (`blocks_edges`); only the *display* of a chain is visibility-scoped
  (`blocks_edges_visible`, `Principal::sees_all`). Don't mix these up.
- Never edit an applied migration; add a new numbered file.
- Log history with `log_activity`, refresh search with `reindex`, and emit an
  SSE event after commit.
- Markdown is rendered only through `renderMarkdown()`, which sanitizes it.

Before finishing a change:
`cd server && cargo fmt && cargo clippy --all-targets && cargo test` and
`cd web && npx svelte-check --tsconfig ./tsconfig.app.json && npm run build`.
