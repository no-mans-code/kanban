# Kanban

A local, Jira-style Kanban board: tickets, sub-tickets, comments, assignees,
watchers, labels, full history, and a **dependency DAG** with cycle
prevention and an optional maximum chain height. It is one Rust binary with
the web UI embedded in it, backed by a single SQLite file.

- **Backend:** Rust (axum, tokio, sqlx) + SQLite (WAL, FTS5 full-text search)
- **Frontend:** Svelte 5 + TypeScript + Vite; Svelte Flow + ELK for the DAG view
- **Live updates:** Server-Sent Events, so every open tab updates immediately
- **AI agents:** a built-in MCP server (HTTP at `/mcp`, or stdio via `kanban-server mcp`), authenticated by API token
- **Accounts and roles:** humans sign in with a password; agents use scoped API tokens; a site admin sees every
  project by default, everyone else only the projects they're added to
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

### Accounts, roles and tokens

The first time the server starts with no administrator yet, it prints a
**one-time setup code** to its log (`no administrator yet — open the board
and enter this one-time setup code: XXXX-XXXX`). Open the board, enter the
code, and pick a username and password — that account is a **site admin**:
it sees and manages every project, the same as a master switch.

From there, everything is **project membership**:

- **Site admins** ("master") always see and manage every project, current and
  future, without being added anywhere. Use this for yourself and, if you
  want, one manager/orchestrator agent — Settings → People → "Make master".
- **Everyone else** — every ordinary agent, and any human you don't make an
  admin — only sees a project once someone adds them to it, with a role:
  **viewer** (read), **member** (create/edit tickets, comment, link),
  **admin** (workflow, membership, deleting tickets). A project that isn't
  yours doesn't 403 — it 404s, as if it doesn't exist. This is the whole
  point: it's what stops an agent from picking up work that isn't its by
  accident, without you having to remember to hide anything.
- **Agents authenticate with API tokens**, not passwords (`Authorization:
  Bearer kbn_...`). Create one from **Settings → Tokens**: name it, optionally
  narrow it to specific projects (default: every project that account can
  reach) and mark it read-only, set an expiry (default 90 days), and the
  board shows you the secret **once**. A token can never reach further than
  the account it belongs to — narrowing only ever restricts, never grants.
  The same tokens authenticate MCP.
- Humans sign in with a session cookie (`HttpOnly`, `SameSite=Strict`, 30
  days); agents never get a cookie. Revoking a token or deactivating an
  account takes effect immediately, everywhere, including open MCP
  connections and SSE streams.

**Still, this is a local, single-machine tool, not a hosted product:** there's
no email verification, password reset flow, or audit log UI, and the server
binds to 127.0.0.1 by default. Even so, because "localhost only" doesn't stop
a malicious web page open in your own browser, every request's `Host` and
`Origin` headers must also name an allowed host (loopback by default) —
`server/src/guard.rs` blocks DNS-rebinding and cross-site attacks before auth
even runs. Login and setup attempts are rate-limited per account. Security
response headers (CSP, `X-Frame-Options`, etc.) are set on every response
(`server/src/headers.rs`). None of this is a substitute for not exposing the
board to a real network without a reverse proxy and TLS in front of it.

### Configuration (environment variables)

| Variable | Default | Meaning |
|---|---|---|
| `KANBAN_DB` | `data/kanban.db` (relative to the working directory) | SQLite file; created with its directory if missing |
| `KANBAN_PORT` | `8610` | HTTP port |
| `KANBAN_BIND` | `127.0.0.1` | Listen address; logs a warning if not loopback |
| `KANBAN_ALLOWED_HOSTS` | *(empty)* | Extra host names the board may be reached by, comma-separated (loopback names are always allowed). Needed only if you serve it on a LAN name or IP |
| `KANBAN_DEMO` | `0` | `1` = same as `--demo` |
| `KANBAN_COOKIE_SECURE` | `0` | `1` adds `Secure` to the session cookie. Only set this if the board is actually served over HTTPS (e.g. behind a reverse proxy) — over plain HTTP it makes the browser silently refuse to ever send the cookie |
| `RUST_LOG` | `kanban_server=info,tower_http=warn` | Log filter |

### Connect AI agents (MCP)

The board is also a [Model Context Protocol](https://modelcontextprotocol.io)
server, so any MCP client can read and work the board through typed tools:
`list_projects`, `get_project`, `list_users`, `search_tickets`,
`list_ready_tickets` (what's unblocked and not done), `get_ticket`,
`create_ticket`, `update_ticket`, `add_comment`, `link_tickets` and
`unlink_tickets`. Tools take keys, status names, usernames and label names.
They go through the same API as the UI, so cycle and height checks, history,
permissions and live updates all apply exactly as they do for a person —
**a narrowed token only ever sees the projects it's a member of, over MCP
just as over the REST API.** Deleting is deliberately not exposed to agents.

First create an agent account and an API token for it from **Settings →
People** and **Settings → Tokens** (see above). The board must be running.

| Client | Setup |
|---|---|
| Claude Code | `claude mcp add --transport http kanban http://127.0.0.1:8610/mcp --header "Authorization: Bearer kbn_..."` |
| Any client with HTTP support | URL `http://127.0.0.1:8610/mcp`, header `Authorization: Bearer kbn_...` |
| Any client with stdio only (Claude Desktop, Cursor, ...) | command `<path>/kanban/server/target/release/kanban-server`, args `["mcp", "--token", "kbn_..."]` |
| Board in Docker, stdio client | command `docker`, args `["exec", "-i", "kanban-board", "kanban-server", "mcp", "--token", "kbn_..."]` |

A typical JSON config for stdio clients (the Tokens page in the UI writes
this out for you, with the real token filled in, right after you create one):

```json
{
  "mcpServers": {
    "kanban": {
      "command": "C:/path/to/kanban/server/target/release/kanban-server.exe",
      "args": ["mcp", "--token", "kbn_..."]
    }
  }
}
```

`kanban-server mcp` forwards to `http://127.0.0.1:$KANBAN_PORT` (override with
`--url` or `KANBAN_MCP_URL`; the token can also come from `KANBAN_MCP_TOKEN`
instead of `--token`) and returns a clear error, not a hang, if the board
isn't running or the token is invalid, expired or revoked.

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
11. **Every route needs a `Principal`, gotten one of three ways**, and no
    fourth way may exist: (a) `Auth(p): Auth` extracts one the auth
    middleware already put in the request's extensions after checking a
    bearer token or session cookie; (b) code running **inside** the server
    (demo seeding, MCP tools) builds its own `Principal` and calls
    `inproc::call`, which attaches it directly — this is the only legitimate
    bypass of header-based auth, and it must never be reachable from
    anything that echoes untrusted input into that `Principal`; (c) tests use
    the same in-process attachment to skip auth *plumbing* while testing
    *business logic* (`tests/api.rs`) — auth itself is tested for real, over
    HTTP headers, in `tests/auth.rs`. Never accept an identity from a header
    the caller controls (the old `X-Actor` is gone for exactly this reason).
12. **The dependency graph is checked board-wide, always** (`blocks_edges()`
    in `settings.rs`) — never scope the cycle/height *check* to what one
    caller can see, or a hidden edge could let a real cycle through. Only the
    *display* of a chain (an error message, `GET /settings`) is scoped to
    visibility, via `blocks_edges_visible()` or by dropping the `chain` detail
    entirely when it touches a project the caller can't see
    (`Principal::sees_all`). Keep those two concerns separate.
13. **No authentication by design allows binding past loopback.** Keep the
    default bind on 127.0.0.1. The `guard.rs` middleware (Host/Origin
    allow-list) runs *before* authentication and is the only thing standing
    between a malicious web page and the API; never remove it or add
    permissive CORS. `headers.rs` (CSP, frame-options, ...) runs on every
    response; if you add a legitimate need for `'unsafe-inline'` or a new
    origin, extend the CSP deliberately, in one place, with a comment saying
    why — don't work around it in a component.

### Repository map

```
kanban/
  run-unix.sh, run-macos.sh,        one-shot build + launch per OS
  run-windows.ps1, run-windows.cmd
  Dockerfile, compose.yaml          image = frontend build -> Rust build (embeds it) -> slim runtime; /data volume
  server/                           Rust crate `kanban-server`
    Cargo.toml, build.rs            build.rs makes Cargo rebuild when web/dist changes (the UI is embedded)
    migrations/0001_init.sql        the ticket/board schema
    migrations/0002_auth.sql        users.kind/is_admin/password_hash, project_members, api_tokens(_projects), sessions
    src/main.rs                     env config, `--demo`, `healthcheck` and `mcp` subcommands, prints the setup code, starts axum
    src/lib.rs                      AppState (db, events, setup_code, login_limiter, cookie_secure), open_db, router(), write_tx(), now_ms()
    src/error.rs                    AppError -> JSON {error:{code,message,detail}}; maps sqlx constraint errors to 400/409
    src/auth.rs                     Principal, Role, session/token authentication, password hashing, secrets — see Accounts, roles and tokens
    src/guard.rs                    Host/Origin allow-list middleware (DNS rebinding + cross-site protection); runs before auth
    src/headers.rs                  security response headers (CSP, X-Frame-Options, ...) on every response
    src/ratelimit.rs                in-memory attempt limiter for login/setup, keyed by username
    src/events.rs                   broadcast channel feeding SSE
    src/dag.rs                      pure graph algorithms + unit tests (cycle path, longest chain, chain through a new edge)
    src/models.rs                   row structs, SUMMARY_SELECT (the one ticket-summary query), shared helpers
                                    (resolve_key, summaries, log_activity, reindex, watch, valid_parent, validation)
    src/api/mod.rs                  route table, `Auth` re-export, /api/health, /api/events (SSE, visibility-filtered)
    src/api/auth.rs                 /api/auth/{status,setup,login,logout,password}
    src/api/tokens.rs               /api/tokens: create (scoped/read-only/expiry), list, revoke
    src/api/tickets.rs              list/search/filter (visibility-scoped), detail, create, update (PATCH), move, watchers, activity
    src/api/deletion.rs             delete plan (preview) and deletion with explicit decisions for children and dependents
    src/api/comments.rs             comment CRUD (edit/delete: author or a project/site admin only)
    src/api/links.rs                link create/delete with DAG checks; /api/graph for the DAG view (both visibility-scoped)
    src/api/settings.rs             max_dag_height get (any signed-in caller, scoped) / set (site admin); blocks_edges() / blocks_edges_visible()
    src/api/projects.rs             projects, workflow statuses, and /members (role management)
    src/api/labels.rs, users.rs     labels (global) and accounts (site-admin-managed; self-service for your own name/color)
    src/web.rs                      serves the embedded frontend (SPA fallback to index.html; /api/* never falls back)
    src/inproc.rs                   calls the API in-process with an explicit Principal (demo seeding, MCP tools)
    src/mcp/mod.rs                  MCP JSON-RPC: initialize, ping, tools/list, tools/call; POST /mcp handler (bearer-token authenticated)
    src/mcp/tools.rs                the MCP tools: schemas + implementations on top of the HTTP API, scoped by the caller's Principal
    src/mcp/bridge.rs               `kanban-server mcp`: stdio <-> POST /mcp bridge (std-only HTTP/1.1 client, sends the bearer token)
    src/demo.rs                     first-run user + `--demo` data, created by calling the real API in-process as the system principal
    tests/api.rs                    ticket/DAG/deletion business-logic tests (auth is attached directly, not over headers)
    tests/auth.rs                   the real auth surface over HTTP: setup/login/logout, roles, token scoping, visibility, rate limits
    tests/mcp.rs                    MCP protocol, an agent workflow end to end, token-scoped visibility, and the stdio bridge over TCP
  web/                              Svelte 5 app (runes only; no legacy `export let` or stores)
    src/main.ts, App.svelte         mount; auth gate (setup/login/app), routing, keyboard shortcuts, same-origin link interception
    src/app.css                     design tokens (CSS variables) for dark/light + shared .btn/.input/.chip/.md styles
    src/lib/api.ts                  typed fetch client; the ONLY place that calls the server; throws ApiError; onUnauthorized() hook
    src/lib/types.ts                TypeScript mirrors of server JSON + enums (TYPES, PRIORITIES, LINK_CHOICES, Role)
    src/lib/store.svelte.ts         global state: me (signed-in identity), users, labels, projects (with role), current project, theme, SSE
    src/lib/router.svelte.ts        history-API router; `?ticket=KEY` opens the side panel on any page
    src/lib/filters.svelte.ts       board/list filters (instant client filtering + server full-text hits)
    src/lib/toast.svelte.ts         toasts and the promise-based confirm dialog
    src/lib/markdown.ts             marked + DOMPurify; ticket keys become links
    src/components/                 SetupView, LoginView, AccountMenu, BoardView (drag and drop), ListView,
                                    GraphView (lazy-loaded; ELK is ~1.4MB), TicketPanel (+ panel/*), CreateTicket,
                                    SettingsView (+ settings/PeoplePanel, settings/TokensPanel), ProjectMembers, Sidebar, Topbar
```

### Data model (SQLite)

| Table | Purpose / notes |
|---|---|
| `users` | People and agents. `kind` (human/agent), `is_admin` (site admin = sees every project), `password_hash` (humans only, Argon2id; null for agents). Never deleted, only `active = 0`. `username` unique, case-insensitive. |
| `project_members` | `(project_id, user_id) -> role` (viewer/member/admin). Visibility for non-admins; a project you're not in doesn't exist to you. |
| `api_tokens`, `api_token_projects` | A token belongs to one user; only its SHA-256 hash is stored. `all_projects` or a narrowed set via the join table; `read_only`; `expires_at`; `revoked_at`. |
| `sessions` | Browser login sessions; `hash` (SHA-256 of the cookie value) is the key, so the plaintext session id is never stored either. |
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

Every route needs `Authorization: Bearer kbn_...` or a session cookie, except
`/health`, `/auth/status`, `/auth/setup` and `/auth/login`. Errors look like
`{"error": {"code": "...", "message": "...", "detail": ...}}`. Codes:
`invalid` (400), `unauthenticated` (401), `forbidden`, `cross_project_dependency` (403),
`not_found` (404), `duplicate`, `cycle`, `height_exceeded`, `status_in_use`,
`last_status`, `last_admin`, `last_project_admin`, `decision_required`,
`already_set_up` (409), `rate_limited` (429), `internal` (500). For `cycle`
and `height_exceeded`, `detail.chain` lists the ticket keys involved — omitted
entirely if the chain passes through a project you can't see.

| Method & path | Purpose |
|---|---|
| `GET /health` | `{ok}`. Public, and deliberately says nothing about the board's contents |
| `GET /events` | SSE stream, filtered to projects the caller can see (see below) |
| `GET /auth/status` | `{setup_required, user: Me\|null, via: "token"\|"session"\|null}`. Public |
| `POST /auth/setup` | `{code, username, display_name?, password}`. Works once, before any admin exists |
| `POST /auth/login`, `POST /auth/logout` | `{username, password}` -> sets/clears the session cookie |
| `POST /auth/password` | `{current, new}`. Session only, not a token |
| `GET /tokens?user_id=`, `POST /tokens`, `DELETE /tokens/{id}` | API tokens. `POST` body: `{name, user_id?, projects?: [KEY], read_only?, expires_in_days?}`; response includes the plaintext `token` **once**. Managing another account's tokens needs site admin |
| `GET/POST /users`, `PATCH /users/{id}` | Accounts (`username`, `display_name`, `color`, `kind`, `is_admin`, `active`, `password`). Site-admin only, except your own `display_name`/`color` |
| `GET/POST /projects`, `GET/PATCH /projects/{key}` | Only projects you can see; each item/detail includes your `role`. `POST` (create) is site-admin only |
| `GET /projects/{key}/members`, `PUT/DELETE /projects/{key}/members/{user_id}` | Project roles (viewer/member/admin); project-admin only to change |
| `GET/POST /projects/{key}/statuses` | List / add a status (`name`, `category`) |
| `PUT /projects/{key}/statuses/order` | `{ids: [...]}`, every status exactly once |
| `PATCH /statuses/{id}`, `DELETE /statuses/{id}?move_to={id}` | Rename / recategorize; delete (tickets must move somewhere) |
| `GET/POST /labels`, `PATCH/DELETE /labels/{id}` | Labels |
| `GET /tickets` | Filters: `project`, `status_id`, `assignee` (id or `none`), `type`, `priority`, `label_id`, `parent` (key), `watcher`, `q` (full-text + exact key), `sort` (`rank`/`updated`/`created`/`priority`/`key`), `limit` (default 2000, max 5000), `offset`. The `X-Total-Count` header gives the number of matches across all pages; `api.allTickets()` in the web client pages through them |
| `POST /tickets` | `{project, type, title, description?, priority?, status_id?, assignee_id?, parent? (key), label_ids?, watcher_ids?}` |
| `GET /tickets/{key}` | Summary + `description`, `watcher_ids`, `children`, `links` |
| `PATCH /tickets/{key}` | Any of `title, description, type, priority, status_id, assignee_id (null to clear), parent (key or null), label_ids` |
| `POST /tickets/{key}/move` | `{status_id, after: key or null}`; drag-and-drop placement |
| `GET /tickets/{key}/delete-plan` | Everything a deletion would touch: children, grandchildren, parent, blockers, dependents (with `becomes_ready_if_dropped`), other links, and the valid `children_options` / `dependents_options` |
| `DELETE /tickets/{key}?children=&dependents=&move_to=&transfer_to=` | Deletes, applying explicit decisions. `children`: `delete` · `detach` (subtasks become tasks) · `move` (+`move_to`) · `promote` (subtasks become tasks in the grandparent epic). `dependents`: `drop` · `bridge` (its blockers now block them) · `children` · `transfer` (+`transfer_to`). A decision is **required** whenever that group is non-empty (else `409 decision_required` with the plan). New links pass the cycle/height checks or nothing is deleted |
| `GET /tickets/{key}/activity` | History |
| `POST /tickets/{key}/watchers`, `DELETE /tickets/{key}/watchers/{user_id}` | Watchers |
| `GET/POST /tickets/{key}/comments`, `PATCH/DELETE /comments/{id}` | Comments |
| `POST /links`, `DELETE /links?source=&target=&kind=` | `{source, target, kind}`; for `blocks`, source must finish first |
| `GET /graph?project=KEY&all=bool` | DAG view: `nodes`, `edges`, `hierarchy`, `longest_chain` (ids), `max_height` |
| `GET /settings` | Any signed-in caller; `dag_height`/`longest_chain` are scoped to what you can see (the true board-wide figures for a site admin) |
| `PATCH /settings` | `{max_dag_height: int or null}`. Site-admin only — it's a board-wide policy |

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
`State`, `Auth`, `Path`, `Query`, `Json`; return `ApiResult<...>`). Decide and
enforce its access explicitly: `p.require_writable()?` for any mutation (read-
only tokens must be blocked), then `p.require(&mut tx, project_id, Role::_)?`
or `p.require_ticket(&mut tx, id, Role::_)?` for anything project- or ticket-
scoped, or `p.require_site_admin()?` for board-wide actions. Register the
route in `api/mod.rs::routes()` (axum 0.8 path syntax: `/{param}`), use
`write_tx` for writes, emit an event after commit, and add a method to
`web/src/lib/api.ts`. Add a case to `tests/auth.rs` proving the access you
just decided on is actually enforced, not just the happy path.

**Add a link kind:** extend the `CHECK` constraint via a migration (SQLite needs
a table rebuild for that), `LINK_KINDS` in `models.rs`, `link_label()` in
`tickets.rs`, and `LinkKind` / `LINK_CHOICES` in `types.ts`. Only `blocks`
takes part in the DAG checks.

**Add an MCP tool:** add its schema to `definitions()` and a match arm in
`run()` in `server/src/mcp/tools.rs`, then implement it on `Ctx` by calling the
HTTP API (`self.get` / `self.request`). Never touch the database directly from
a tool, or it will skip validation, history and live events. Take human
identifiers (keys, names) and return `Names::brief` shapes, never raw ids.
Return `Err(message)` for problems the model can fix; it is shown with
`isError: true`. Add a case to `tests/mcp.rs`.

**Add a Settings tab:** add to `TABS` and a branch in `SettingsView.svelte`.
Global settings go in the `settings` table through `api/settings.rs`.

### Checks to run after a change

```bash
cd server && cargo fmt && cargo clippy --all-targets && cargo test   # unit + api + auth + mcp tests
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
- **Windows Smart App Control** can block freshly compiled test binaries
  ("An Application Control policy has blocked this file"). That's the OS, not
  a failing test. Retry, or run the suite in Docker:
  `docker run --rm -v "$PWD:/src:ro" rust:1-bookworm bash -c "cp -r /src/server /w && cd /w && cargo test"`.
- **MCP stdio must keep stdout clean:** `kanban-server mcp` exits before logging
  is set up. Anything else printed to stdout would corrupt the protocol.
- **Graceful shutdown is intentionally off:** open SSE streams would make
  Ctrl+C hang. SQLite in WAL mode is safe to stop at any time.
- **CSP `style-src` needs `'unsafe-inline'`.** Chromium gates inline style
  *attributes* on `style-src` regardless of whether they're set from HTML or
  from JS (`element.style.x = ...`, which is how Svelte's `style:` directive
  works) — it's not only about `<style>` tags. `script-src` has no such
  exception and stays `'self'` only; move any inline `<script>` (there was
  one, the early theme-flash fix) to an external file instead of relaxing it.
- **Visibility vs. the DAG check are different concerns, on purpose** (see
  hard rule #12). If you're touching `links.rs`, `deletion.rs` or
  `settings.rs`, know which one you're changing before you change it.

### Not built yet (Phase 2)

Sprints and backlog planning, work-in-progress limits per column, attachments,
due dates and time tracking, components and versions, a JQL-like query
language, bulk edit, saved filters, @mentions and notifications, webhooks,
custom fields, workflow transition rules, swimlanes, reports (burndown,
cumulative flow), password reset / email verification, an audit log UI for
token and permission changes, and integration with `ai-system`'s orchestrator
(replacing `../board`).
