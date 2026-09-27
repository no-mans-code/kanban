# Security

## Disclaimer

This board is built for one setting: **a single trusted machine, used by a
human operator and the agents they run locally.** It has not been
independently audited or penetration-tested. The code, tests, and this
document are the project's own self-assessment, not a third-party guarantee.
Nothing here is a warranty of fitness for any particular purpose — treat it
the way you'd treat any local developer tool you didn't write yourself: read
the code for anything security-sensitive before you rely on it, especially
before exposing it beyond loopback.

**Do not expose this board directly to the internet.** It has no built-in
TLS. Binding it to anything other than `127.0.0.1` prints a warning at
startup for exactly this reason (`server/src/main.rs`). If you need it
reachable from another machine, put a TLS-terminating reverse proxy in front
of it, set `KANBAN_ALLOWED_HOSTS` to the name it's served under, and set
`KANBAN_COOKIE_SECURE=1` — see the Configuration table in `README.md`.

## Threat model

In scope — things the design actively defends against:
- A malicious web page open in the operator's own browser (DNS rebinding,
  cross-site requests, clickjacking), even though the board only listens on
  loopback.
- A scoped agent trying to read or write a project it wasn't given access to,
  including by exploiting error messages (e.g. a dependency-cycle error
  naming a ticket in a project it can't see).
- Credential/token guessing against the login, setup, and API-token paths.
- Stored XSS via ticket/comment Markdown.

Out of scope by design (accepted, not defended against, see Disclaimer):
- A network attacker who isn't the browser on the operator's own machine —
  i.e. anything that requires exposing the board beyond loopback without the
  operator adding TLS themselves.
- A human or agent with filesystem or process access to the host machine
  itself; at that point they can read `data/kanban.db` or the process
  environment directly, which no application-layer control can prevent.

## What's implemented

- **Passwords:** Argon2id via the [`argon2`](https://docs.rs/argon2) crate at
  its default cost parameters — 19 MiB memory, 2 iterations, 1-way
  parallelism (`m=19456, t=2, p=1`) — which matches the second recommended
  Argon2id configuration in the [OWASP Password Storage Cheat
  Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html)
  and is a valid parameter choice under
  [RFC 9106](https://www.rfc-editor.org/rfc/rfc9106) (`server/src/auth.rs`).
  An 8-character minimum is enforced; there is no complexity rule or
  breach-list check (see Known limitations).
- **Tokens and sessions:** 256 bits of randomness from the OS CSPRNG
  (`getrandom`), stored as a SHA-256 hash rather than in plaintext. A fast
  hash is correct here, not a mistake — these are high-entropy random
  secrets, not human-chosen passwords, so there's nothing for an offline
  dictionary attack to search (`server/src/auth.rs::sha256_hex`,
  `new_secret`).
- **Session cookies:** `HttpOnly`, `SameSite=Strict`, 30-day expiry, `Secure`
  opt-in via `KANBAN_COOKIE_SECURE` (off by default because it's off by
  default over plain HTTP the browser would silently never send the cookie
  at all) (`server/src/api/auth.rs::session_cookie`).
- **CSRF / DNS rebinding:** every request's `Host` and `Origin`, when
  present, must name an allowed host (loopback, plus anything explicitly
  added via `KANBAN_ALLOWED_HOSTS`) — checked before authentication even
  runs (`server/src/guard.rs`). This is defense in depth on top of
  `SameSite=Strict`, which alone would stop most CSRF but not a DNS-rebinding
  attack, since that makes the hostile origin *appear* same-site.
- **Response headers:** CSP, `X-Frame-Options: DENY`,
  `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`,
  `Cross-Origin-Opener-Policy` and `Cross-Origin-Resource-Policy: same-origin`
  on every response (`server/src/headers.rs`). `script-src` is `'self'` with
  no exceptions; `style-src` needs `'unsafe-inline'` for a documented reason
  (inline `style:` attributes set from Svelte, not a `<script>` relaxation —
  see the comment in `headers.rs`).
- **Rate limiting:** 10 attempts per 15 minutes, keyed by username (or the
  fixed key `"setup"`), on both login and first-run setup
  (`server/src/api/auth.rs`). The setup code comparison is constant-time
  (`server/src/auth.rs::constant_time_eq`) even though, per its own comment,
  nothing off this machine can reach it.
- **Authorization:** every route requires a `Principal`; a project you
  aren't a member of 404s rather than 403s, so its name and contents never
  leak through an error. Cycle/height checks on the dependency DAG always run
  against the true, unscoped graph — only the *display* of a chain is
  visibility-scoped, so correctness never depends on what the caller is
  allowed to see (`server/src/auth.rs`, `AGENTS.md` rule 5).
- **Tokens:** narrowable to specific projects (never wider than the owning
  account), optionally read-only, optionally expiring, revoked instantly
  everywhere including open MCP/SSE connections.
- **Markdown:** rendered only through a sanitizing renderer
  (`renderMarkdown()`), never raw HTML.

## Known limitations (accepted, not planned unless noted)

- **The rate limiter is in-memory and per-process.** It resets on restart and
  isn't shared across instances. That's the right tradeoff for the
  single-process local deployment this is designed for; it would not be
  sufficient for a clustered or multi-instance deployment
  (`server/src/ratelimit.rs`).
- **No MFA/2FA.**
- **No password complexity rules or breach-list check** beyond the
  8-character minimum.
- **No audit log for account, role, or token changes** — ticket/project
  activity has one; account administration doesn't yet. Already tracked as a
  roadmap item in `README.md` under "Not built yet (Phase 2)".
- **No email verification or password-reset flow**, by design: the operator
  who can create the first admin account already has filesystem/process
  access to the machine the board runs on, so there's no separate identity to
  verify.
- **No built-in TLS.** See Disclaimer.

## Reporting a vulnerability

Please use GitHub's private **Security Advisory** feature on this repository
(Security tab → "Report a vulnerability") rather than a public issue, so a
fix can go out before the report does. For anything that isn't
security-sensitive, a regular issue is fine.
