# Kanban

A local, Jira-style Kanban board: tickets, sub-tickets, comments, assignees,
watchers, labels, full history, and a **dependency DAG** with cycle
prevention and an optional maximum chain height. It is one Rust binary with
the web UI embedded in it, backed by a single SQLite file.

- **Backend:** Rust (axum, tokio, sqlx) + SQLite (WAL, FTS5 full-text search)
- **Frontend:** Svelte 5 + TypeScript + Vite; Svelte Flow + ELK for the DAG view
- **Live updates:** Server-Sent Events, so every open tab updates immediately
- **Themes:** dark (default, white text) and light, switchable with `T`

Part of the `ai-system` project, where it will replace the minimal ticket
service in `../board` (port 8600). **That integration has not started**; this
board is standalone for now.

---

## Run it

| Platform | Command | Needs |
|---|---|---|
| Windows | double-click `run-windows.cmd`, or `.\run-windows.ps1` | Rust (rustup), Node.js 20.19+, VS C++ Build Tools |
| macOS | `./run-macos.sh` | Rust (rustup), Node.js 20.19+, Xcode Command Line Tools |
| Linux / Unix | `./run-unix.sh` | Rust (rustup), Node.js 20.19+, a C compiler |
| Docker (any OS) | `docker compose up -d --build` | Docker |

Then open **http://127.0.0.1:8610**. All three scripts build the frontend and
then the release server, start it, and open a browser. They take the same
options (PowerShell spelling in brackets):

| Option | Effect |
|---|---|
| `--demo` (`-Demo`) | Seed a `DEMO` project with sample tickets on first start |
| `--port N` (`-Port N`) | Listen on another port |
| `--no-build` (`-NoBuild`) | Start the last build without rebuilding |
| `--no-open` (`-NoOpen`) | Don't open a browser |

The scripts store data in `kanban/data/kanban.db`. With Docker, data lives in
the named volume `kanban-data` and survives `docker compose down` and image
rebuilds; only `docker compose down -v` deletes it. Seed demo data in Docker
with `KANBAN_DEMO=1 docker compose up -d --build`.

**There is no authentication.** The server binds to 127.0.0.1 by default,
and the Docker setup publishes it on the host's 127.0.0.1 only. You choose
who you are "acting as" in the top-right corner; that is attribution, not
security. Don't expose it to a network without putting auth in front.

### Configuration (environment variables)

| Variable | Default | Meaning |
|---|---|---|
| `KANBAN_DB` | `data/kanban.db` (relative to the working directory) | SQLite file; created with its directory if missing |
| `KANBAN_PORT` | `8610` | HTTP port |
| `KANBAN_BIND` | `127.0.0.1` | Listen address; logs a warning if not loopback |
| `KANBAN_DEMO` | `0` | `1` = same as `--demo` |
| `RUST_LOG` | `kanban_server=info,tower_http=warn` | Log filter |

### Keyboard shortcuts

`c` create · `/` search everything · `f` filter the board · `g` then `b`/`l`/`d`/`s`
for board / list / dependencies / settings · `t` theme · `Esc` close panel · `?` help

---

## For AI assistants (and new contributors): read this first

This section is written so that any LLM can work on this codebase correctly
without reading every file. It lists what is where, the rules the code relies
on, and step-by-step recipes for common changes.

### Hard rules (invariants)

Breaking one of these breaks the product. Keep them.

1. **Every write goes through `write_tx()`** (`server/src/lib.rs`), which starts
   a `BEGIN IMMEDIATE` transaction. The cycle check, height check, ticket
   numbering and rank placement are read-check-write sequences that are only
   race-free because of this. Never write with `pool.begin()` or directly on
   the pool.
2. **Only `blocks` links form the DAG.** `relates`, `duplicates` and `clones` are
   plain links. A `blocks` edge `(source, target)` means *target cannot finish
   before source*. Every new `blocks` edge is checked for cycles and against
   the max-height setting **on the server** (`api/links.rs`). The UI never
   decides this.
3. **Height = number of tickets on the longest `blocks` chain** (A→B→C is 3).
   `max_dag_height` is global (all projects), stored in `settings`; absent
   means unlimited. Lowering it below the current height is refused with the
   offending chain.
4. **Parent/child hierarchy is separate from the DAG** and follows Jira:
   epic → story/task/bug → subtask (`valid_parent` in `models.rs`). A subtask
   must have a parent, an epic cannot have one, and parents must be in the same
   project. Levels strictly decrease, so the hierarchy can never cycle.
5. **Ticket keys (`KAN-12`) are the public identifier** in URLs and the API.
   Numeric ids are internal. Numbers come from `projects.next_number` and are
   never reused.
6. **Every user-visible change writes an `activity` row** via `log_activity`,
   with human-readable old/new values (names, not ids), so history still reads
   correctly after things are renamed or deleted.
7. **The search index is maintained by the server, not triggers.** After
   changing a ticket's title or description, or any of its comments, call
   `reindex(conn, ticket_id)`. After deleting a ticket, delete its `ticket_fts`
   row.
8. **Emit an event after every commit** (`state.events.emit(...)`) or open
   browsers won't update. Emit *after* `tx.commit()`, never before.
9. **Never edit an existing migration.** Add `server/migrations/000N_name.sql`.
   sqlx records checksums and refuses to start if an applied migration changed.
10. **Markdown is rendered only through `renderMarkdown()`** (`web/src/lib/markdown.ts`),
    which sanitizes with DOMPurify. Comments and descriptions are written by
    agents too, so treat them as untrusted. Never `{@html}` anything else.
11. **No authentication by design**; keep the default bind on loopback.
    `X-Actor: <user id>` only attributes changes.

### Repository map

```
kanban/
  run-unix.sh, run-macos.sh,        one-shot build + launch per OS
  run-windows.ps1, run-windows.cmd
  Dockerfile, compose.yaml          image = frontend build -> Rust build (embeds it) -> slim runtime; /data volume
  server/                           Rust crate `kanban-server`
    Cargo.toml, build.rs            build.rs makes Cargo rebuild when web/dist changes (the UI is embedded)
    migrations/0001_init.sql        the whole schema (see Data model)
    src/main.rs                     env config, `--demo`, `healthcheck` subcommand, starts axum
    src/lib.rs                      AppState, open_db (WAL, foreign keys, migrations), router(), write_tx(), now_ms()
    src/error.rs                    AppError -> JSON {error:{code,message,detail}}; maps sqlx constraint errors to 400/409
    src/events.rs                   broadcast channel feeding SSE
    src/dag.rs                      pure graph algorithms + unit tests (cycle path, longest chain, chain through a new edge)
    src/models.rs                   row structs, SUMMARY_SELECT (the one ticket-summary query), shared helpers
                                    (resolve_key, summaries, log_activity, reindex, watch, valid_parent, validation)
    src/api/mod.rs                  route table, `Actor` extractor (X-Actor header), /api/health, /api/events (SSE)
    src/api/tickets.rs              list/search/filter, detail, create, update (PATCH), move (drag and drop), delete, watchers, activity
    src/api/comments.rs             comment CRUD
    src/api/links.rs                link create/delete with DAG checks; /api/graph for the DAG view
    src/api/settings.rs             max_dag_height get/set; blocks_edges() and max_height() helpers
    src/api/projects.rs             projects and their workflow statuses (create, rename, recategorize, reorder, delete-with-move)
    src/api/labels.rs, users.rs     labels (global) and people
    src/web.rs                      serves the embedded frontend (SPA fallback to index.html; /api/* never falls back)
    src/demo.rs                     first-run user + `--demo` data, created by calling the real API in-process
    tests/api.rs                    integration tests against a real temp SQLite file
  web/                              Svelte 5 app (runes only; no legacy `export let` or stores)
    src/main.ts, App.svelte         mount; routing, keyboard shortcuts, same-origin link interception
    src/app.css                     design tokens (CSS variables) for dark/light + shared .btn/.input/.chip/.md styles
    src/lib/api.ts                  typed fetch client; the ONLY place that calls the server; throws ApiError
    src/lib/types.ts                TypeScript mirrors of server JSON + enums (TYPES, PRIORITIES, LINK_CHOICES)
    src/lib/store.svelte.ts         global state: users, labels, projects, current project, actor, theme, SSE -> version counters
    src/lib/router.svelte.ts        history-API router; `?ticket=KEY` opens the side panel on any page
    src/lib/filters.svelte.ts       board/list filters (instant client filtering + server full-text hits)
    src/lib/toast.svelte.ts         toasts and the promise-based confirm dialog
    src/lib/markdown.ts             marked + DOMPurify; ticket keys become links
    src/components/                 BoardView (drag and drop), ListView, GraphView (lazy-loaded; ELK is ~1.4MB),
                                    TicketPanel (+ panel/*), CreateTicket, SettingsView, pickers, Sidebar, Topbar
```

### Data model (SQLite)

| Table | Purpose / notes |
|---|---|
| `users` | People and agents. Never deleted, only `active = 0`. `username` unique, case-insensitive. |
| `projects` | `key` (2-10 chars, `^[A-Z][A-Z0-9]+$`), `next_number` for ticket numbering. |
| `statuses` | Per-project workflow columns: `name`, `category` (`todo`/`in_progress`/`done`), `position`. The category decides `resolved_at`. |
| `labels` | Global, unique name (case-insensitive). |
| `tickets` | `type` (epic/story/task/bug/subtask), `status_id`, `priority` (highest…lowest), `assignee_id`, `reporter_id`, `parent_id`, `rank` (REAL, ordering within a column), `resolved_at` (set while in a `done`-category status). |
| `ticket_labels`, `ticket_watchers` | Many-to-many. Reporter, assignee and commenters are added as watchers automatically. |
| `comments` | Markdown body, `author_id` (null = system). |
| `ticket_links` | `(source_id, target_id, kind)` primary key; indexed both ways. **This edge table is the DAG.** |
| `activity` | History: `action`, `field`, `old_value`, `new_value` (display text). |
| `settings` | Key/value; currently only `max_dag_height`. |
| `ticket_fts` | FTS5 over title, description and all comments; `rowid` = ticket id. |

**Why an edge table for the DAG:** it keeps integrity in the database
(foreign keys, cascading deletes, no duplicate edges), makes "what does X
block / what blocks X" an index lookup in either direction, and a link change
touches one row. A closure table would make reachability reads faster but
grows with the square of the ticket count and makes edge deletion complex. A
JSON `depends_on` array (what `../board` uses) can't be indexed or checked.
On each new `blocks` edge the server loads all `blocks` edges and runs
iterative algorithms in `dag.rs` (BFS for the cycle, Kahn's topological order
for longest chains). That is O(V+E) and fine at tens of thousands of edges.

**Ranks:** a move takes the midpoint of its new neighbours' ranks, so it
updates one row. When the gap falls below 1e-6, that column is renumbered
(spacing 1024) and the placement retried (`place()` in `tickets.rs`).

### API reference (all JSON, prefix `/api`)

Send `X-Actor: <user id>` on writes to attribute them. Errors look like
`{"error": {"code": "...", "message": "...", "detail": ...}}`. Codes:
`invalid` (400), `not_found` (404), `duplicate`, `cycle`, `height_exceeded`,
`status_in_use`, `last_status` (409), `internal` (500). For `cycle` and
`height_exceeded`, `detail.chain` lists the ticket keys involved.

| Method & path | Purpose |
|---|---|
| `GET /health` | `{ok, tickets}` |
| `GET /events` | SSE stream (see below) |
| `GET/POST /users`, `PATCH /users/{id}` | People (`username`, `display_name`, `color`, `active`) |
| `GET/POST /projects`, `GET/PATCH /projects/{key}` | Projects; detail includes `statuses` |
| `GET/POST /projects/{key}/statuses` | List / add a status (`name`, `category`) |
| `PUT /projects/{key}/statuses/order` | `{ids: [...]}`, every status exactly once |
| `PATCH /statuses/{id}`, `DELETE /statuses/{id}?move_to={id}` | Rename / recategorize; delete (tickets must move somewhere) |
| `GET/POST /labels`, `PATCH/DELETE /labels/{id}` | Labels |
| `GET /tickets` | Filters: `project`, `status_id`, `assignee` (id or `none`), `type`, `priority`, `label_id`, `parent` (key), `watcher`, `q` (full-text + exact key), `sort` (`rank`/`updated`/`created`/`priority`/`key`), `limit` (≤ 5000) |
| `POST /tickets` | `{project, type, title, description?, priority?, status_id?, assignee_id?, parent? (key), label_ids?, watcher_ids?}` |
| `GET /tickets/{key}` | Summary + `description`, `watcher_ids`, `children`, `links` |
| `PATCH /tickets/{key}` | Any of `title, description, type, priority, status_id, assignee_id (null to clear), parent (key or null), label_ids` |
| `POST /tickets/{key}/move` | `{status_id, after: key or null}`; drag-and-drop placement |
| `DELETE /tickets/{key}` | Also deletes its subtasks; an epic's children are only detached |
| `GET /tickets/{key}/activity` | History |
| `POST /tickets/{key}/watchers`, `DELETE /tickets/{key}/watchers/{user_id}` | Watchers |
| `GET/POST /tickets/{key}/comments`, `PATCH/DELETE /comments/{id}` | Comments |
| `POST /links`, `DELETE /links?source=&target=&kind=` | `{source, target, kind}`; for `blocks`, source must finish first |
| `GET /graph?project=KEY&all=bool` | DAG view: `nodes`, `edges`, `hierarchy`, `longest_chain` (ids), `max_height` |
| `GET/PATCH /settings` | `{max_dag_height: int or null}`; response adds `dag_height`, `longest_chain` |

**Ticket summary fields** (list, board, graph): `id, key, project_id, number,
type, title, status_id, status_name, status_category, priority, assignee_id,
reporter_id, parent_id, parent_key, rank, created_at, updated_at, resolved_at,
label_ids, child_count, done_child_count, is_blocked, comment_count`.
`is_blocked` = has a `blocks` predecessor whose status isn't done. Timestamps
are Unix milliseconds.

**Live events** (`GET /api/events`): `event: change` with
`data: {"type": "ticket.created|ticket.updated|ticket.moved|ticket.deleted|comment.changed|link.changed|project.changed|label.changed|user.changed|settings.changed", "project_id": n|null, "keys": [...]}`.
Events say *what* changed, not the new state; clients refetch. `event: resync`
means the client missed events and should refetch everything.

### Frontend notes

- **State:** `app` (store.svelte.ts) holds shared data. Views fetch their own
  ticket lists and refetch when `app.ticketsVersion` changes, which SSE bumps.
  That makes live updates and reconciliation after your own edits the same
  code path.
- **Optimistic UI:** drag and drop and status changes update locally first, then
  replace the local copy with the server's answer, and revert with a toast on error.
- **Routing:** `/p/{KEY}/board|list|graph`, `/settings/{tab}`, and `?ticket=KEY`
  for the panel. Internal `<a href>` links are intercepted in `App.svelte`, so
  plain anchors navigate without reloads.
- **Svelte Flow** needs `$state.raw` arrays for `nodes`/`edges`. Link creation
  goes through `onbeforeconnect`, which returns `false` so the server stays the
  source of truth.

### Recipes

**Add a field to tickets (e.g. `due_date`):**
1. New migration `server/migrations/0002_due_date.sql`: `ALTER TABLE tickets ADD COLUMN due_date TEXT;`
2. `models.rs`: add it to `TicketSummary` and to `SUMMARY_SELECT` (the field
   name must match the SQL column alias).
3. `api/tickets.rs`: accept it in `CreateTicket` and `UpdateTicket` (use
   `double_option` if it's nullable), validate it, write it, and `log_activity`
   the change inside `update`.
4. `web/src/lib/types.ts` (`TicketSummary`) and `api.ts` (`TicketInput`/`TicketPatch`).
5. UI: a field in `TicketPanel.svelte` (side column) and optionally in
   `TicketCard.svelte` / `ListView.svelte` / `CreateTicket.svelte`.
6. Add a case to `tests/api.rs`, then run the checks below.

**Add an endpoint:** write the handler in the matching `api/*.rs` (extractors:
`State`, `Actor`, `Path`, `Query`, `Json`; return `ApiResult<...>`), register it
in `api/mod.rs::routes()` (axum 0.8 path syntax: `/{param}`), use `write_tx`
for writes, emit an event after commit, and add a method to `web/src/lib/api.ts`.

**Add a link kind:** extend the `CHECK` constraint via a migration (SQLite needs
a table rebuild for that), `LINK_KINDS` in `models.rs`, `link_label()` in
`tickets.rs`, and `LinkKind` / `LINK_CHOICES` in `types.ts`. Only `blocks`
takes part in the DAG checks.

**Add a Settings tab:** add to `TABS` and a branch in `SettingsView.svelte`.
Global settings go in the `settings` table through `api/settings.rs`.

### Checks to run after a change

```bash
cd server && cargo fmt && cargo clippy --all-targets && cargo test   # 5 unit + 7 integration tests
cd web && npx svelte-check --tsconfig ./tsconfig.app.json && npm run build
```

For UI changes, also run the app and try the feature in a browser; the test
suites don't cover the UI. Development with hot reload uses two terminals:
`cd server && cargo run -- --demo` (API on :8610) and `cd web && npm run dev`
(UI on http://localhost:5173, which proxies `/api` to :8610).

### Gotchas (each of these was hit for real while building this)

- **sqlx 0.9:** query functions only accept `&'static str` or
  `AssertSqlSafe(...)`. For dynamic SQL use `QueryBuilder` with `push_bind`.
  Never `format!` values into SQL.
- **INTEGER vs REAL:** sqlx won't decode an SQLite INTEGER into `f64`.
  `COALESCE(MAX(rank), 0)` fails on an empty column; write `0.0`.
- **Embedded UI goes stale** if Cargo doesn't notice `web/dist` changed.
  `build.rs` handles that; don't remove it. Debug builds read `web/dist` from
  disk, release builds embed it.
- **Svelte effects:** reading state inside `$effect` subscribes to it. Use
  `untrack()` for reads that must not retrigger (see the drag guard in `BoardView.svelte`).
- **FTS5 input** is reduced to quoted prefix terms (`fts_query` in `tickets.rs`)
  so user text can never inject FTS syntax.
- **Shell scripts need LF line endings** (`.gitattributes` enforces it). A CRLF
  shebang fails with "no such file or directory".
- **Graceful shutdown is intentionally off:** open SSE streams would make
  Ctrl+C hang. SQLite in WAL mode is safe to stop at any time.

### Not built yet (Phase 2)

Sprints and backlog planning, work-in-progress limits per column, attachments,
due dates and time tracking, components and versions, a JQL-like query
language, bulk edit, saved filters, @mentions and notifications, webhooks,
custom fields, workflow transition rules, swimlanes, reports (burndown,
cumulative flow), authentication, and integration with `ai-system`'s
orchestrator (replacing `../board`).
