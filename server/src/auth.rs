//! Who is calling, and what they may see and do.
//!
//! - Humans sign in with a password and get a session cookie.
//! - Agents (and humans, for scripts) send `Authorization: Bearer kbn_...`.
//!   A token belongs to an account and can be narrowed to some projects,
//!   made read-only, and given an expiry.
//! - Access is project membership: viewer < member < admin. A project you
//!   aren't a member of doesn't exist for you (404, not 403), so its name,
//!   keys and contents never leak. Site admins see every project.
//!
//! Every request gets a `Principal` from `middleware`; handlers take it with
//! the `Auth` extractor. In-process callers (demo seeding, MCP tools) attach
//! one themselves, which is how an MCP tool runs with exactly the caller's
//! access.

use std::collections::HashSet;

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};

use crate::error::{ApiResult, AppError};
use crate::{AppState, now_ms};

pub const TOKEN_PREFIX: &str = "kbn_";
pub const SESSION_COOKIE: &str = "kanban_session";
pub const SESSION_DAYS: i64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Viewer,
    Member,
    Admin,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "viewer" => Some(Role::Viewer),
            "member" => Some(Role::Member),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Member => "member",
            Role::Admin => "admin",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Principal {
    /// None only for in-process system work (demo seeding).
    pub user_id: Option<i64>,
    pub admin: bool,
    /// A token narrowed to these projects. None = not narrowed.
    pub projects: Option<Vec<i64>>,
    pub read_only: bool,
    pub via_token: bool,
}

/// Which projects a caller can see.
pub struct Visibility {
    all: bool,
    ids: HashSet<i64>,
}

impl Visibility {
    pub fn sees(&self, project_id: i64) -> bool {
        self.all || self.ids.contains(&project_id)
    }

    /// None when everything is visible, for SQL `IN (...)` filters.
    pub fn ids(&self) -> Option<Vec<i64>> {
        (!self.all).then(|| self.ids.iter().copied().collect())
    }
}

fn forbidden(message: impl Into<String>) -> AppError {
    AppError::new(StatusCode::FORBIDDEN, "forbidden", message)
}

impl Principal {
    pub fn system() -> Self {
        Principal { user_id: None, admin: true, projects: None, read_only: false, via_token: false }
    }

    /// Full access, attributed to `user_id`. For in-process seeding only.
    pub fn acting_as(user_id: i64) -> Self {
        Principal { user_id: Some(user_id), ..Principal::system() }
    }

    /// Site-wide administration: accounts, projects, global settings. A
    /// narrowed or read-only token never carries it, even an admin's.
    pub fn is_site_admin(&self) -> bool {
        self.admin && self.projects.is_none() && !self.read_only
    }

    pub fn require_site_admin(&self) -> ApiResult<()> {
        if self.is_site_admin() { Ok(()) } else { Err(forbidden("Only an administrator can do that")) }
    }

    pub fn require_writable(&self) -> ApiResult<()> {
        if self.read_only { Err(forbidden("This API token is read-only")) } else { Ok(()) }
    }

    pub async fn role_in(&self, conn: &mut SqliteConnection, project_id: i64) -> ApiResult<Option<Role>> {
        if let Some(scope) = &self.projects
            && !scope.contains(&project_id)
        {
            return Ok(None);
        }
        let role = if self.admin {
            Some(Role::Admin)
        } else {
            match self.user_id {
                None => None,
                Some(user) => {
                    let role: Option<String> = sqlx::query_scalar(
                        "SELECT role FROM project_members WHERE project_id = ? AND user_id = ?",
                    )
                    .bind(project_id)
                    .bind(user)
                    .fetch_optional(&mut *conn)
                    .await?;
                    role.as_deref().and_then(Role::parse)
                }
            }
        };
        Ok(role.map(|r| if self.read_only { r.min(Role::Viewer) } else { r }))
    }

    pub async fn visibility(&self, conn: &mut SqliteConnection) -> ApiResult<Visibility> {
        if self.admin && self.projects.is_none() {
            return Ok(Visibility { all: true, ids: HashSet::new() });
        }
        let mut ids: HashSet<i64> = if self.admin {
            HashSet::new()
        } else {
            match self.user_id {
                None => HashSet::new(),
                Some(user) => sqlx::query_scalar("SELECT project_id FROM project_members WHERE user_id = ?")
                    .bind(user)
                    .fetch_all(&mut *conn)
                    .await?
                    .into_iter()
                    .collect(),
            }
        };
        if let Some(scope) = &self.projects {
            ids = if self.admin {
                scope.iter().copied().collect()
            } else {
                ids.into_iter().filter(|p| scope.contains(p)).collect()
            };
        }
        Ok(Visibility { all: false, ids })
    }

    /// Checks access to a project. Invisible projects are reported as missing.
    pub async fn require(&self, conn: &mut SqliteConnection, project_id: i64, need: Role) -> ApiResult<Role> {
        match self.role_in(conn, project_id).await? {
            None => Err(AppError::not_found("Project")),
            Some(role) if role < need => Err(forbidden(if self.read_only {
                "This API token is read-only".to_string()
            } else {
                format!("You need {} access to this project to do that", need.as_str())
            })),
            Some(role) => Ok(role),
        }
    }

    /// Checks access to a ticket's project; returns that project's id.
    pub async fn require_ticket(
        &self,
        conn: &mut SqliteConnection,
        ticket_id: i64,
        need: Role,
    ) -> ApiResult<i64> {
        let project_id: i64 = sqlx::query_scalar("SELECT project_id FROM tickets WHERE id = ?")
            .bind(ticket_id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| AppError::not_found("Ticket"))?;
        match self.require(conn, project_id, need).await {
            Err(e) if e.status == StatusCode::NOT_FOUND => Err(AppError::not_found("Ticket")),
            other => other.map(|_| project_id),
        }
    }

    /// Whether every one of these tickets is in a project the caller can
    /// see. Used before showing a chain of ticket keys in an error or a
    /// plan: the dependency graph is board-wide, but nothing in a response
    /// may name a ticket from a project the caller can't see.
    pub async fn sees_all(&self, conn: &mut SqliteConnection, ticket_ids: &[i64]) -> ApiResult<bool> {
        if self.admin && self.projects.is_none() {
            return Ok(true);
        }
        let vis = self.visibility(conn).await?;
        for &id in ticket_ids {
            let project_id: i64 = sqlx::query_scalar("SELECT project_id FROM tickets WHERE id = ?")
                .bind(id)
                .fetch_one(&mut *conn)
                .await?;
            if !vis.sees(project_id) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Handlers take `Auth(principal)`.
pub struct Auth(pub Principal);

impl<S: Send + Sync> FromRequestParts<S> for Auth {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<Principal>().cloned().map(Auth).ok_or_else(unauthenticated)
    }
}

pub fn unauthenticated() -> AppError {
    AppError::new(
        StatusCode::UNAUTHORIZED,
        "unauthenticated",
        "Sign in, or send an API token as `Authorization: Bearer kbn_...`",
    )
}

fn is_public(path: &str) -> bool {
    let api = path == "/api" || path.starts_with("/api/") || path == "/mcp";
    !api || path == "/api/health" || path.starts_with("/api/auth/")
}

pub async fn middleware(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    if req.extensions().get::<Principal>().is_some() {
        return next.run(req).await;
    }
    let found = match bearer(req.headers()) {
        Some(token) => match from_token(&state.db, token).await {
            Ok(p) => Some(p),
            Err(message) => {
                return AppError::new(StatusCode::UNAUTHORIZED, "invalid_token", message).into_response();
            }
        },
        None => match cookie(req.headers(), SESSION_COOKIE) {
            Some(session) => from_session(&state.db, &session).await,
            None => None,
        },
    };
    match found {
        Some(principal) => {
            req.extensions_mut().insert(principal);
            next.run(req).await
        }
        None if is_public(req.uri().path()) => next.run(req).await,
        None => {
            let mut res = unauthenticated().into_response();
            res.headers_mut().insert(header::WWW_AUTHENTICATE, "Bearer".parse().expect("static"));
            res
        }
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

type TokenRow = (i64, i64, bool, bool, Option<i64>, Option<i64>, Option<i64>, bool, bool);

async fn from_token(db: &SqlitePool, token: &str) -> Result<Principal, &'static str> {
    if !token.starts_with(TOKEN_PREFIX) {
        return Err("That is not a kanban API token (they start with kbn_)");
    }
    let row: Option<TokenRow> = sqlx::query_as(
        "SELECT t.id, t.user_id, t.read_only, t.all_projects, t.expires_at, t.revoked_at, t.last_used_at,
                u.is_admin, u.active
           FROM api_tokens t JOIN users u ON u.id = t.user_id
          WHERE t.hash = ?",
    )
    .bind(sha256_hex(token))
    .fetch_optional(db)
    .await
    .map_err(|_| "Could not check the token")?;
    let Some((id, user_id, read_only, all_projects, expires_at, revoked_at, last_used, is_admin, active)) =
        row
    else {
        return Err("Unknown API token");
    };
    let now = now_ms();
    if revoked_at.is_some() {
        return Err("This API token has been revoked");
    }
    if expires_at.is_some_and(|e| e <= now) {
        return Err("This API token has expired");
    }
    if !active {
        return Err("This token's account is deactivated");
    }
    let projects = if all_projects {
        None
    } else {
        Some(
            sqlx::query_scalar("SELECT project_id FROM api_token_projects WHERE token_id = ?")
                .bind(id)
                .fetch_all(db)
                .await
                .map_err(|_| "Could not check the token")?,
        )
    };
    // Recording every use would turn each read into a write.
    if last_used.is_none_or(|t| now - t > 60_000) {
        let _ = sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(db)
            .await;
    }
    Ok(Principal { user_id: Some(user_id), admin: is_admin, projects, read_only, via_token: true })
}

async fn from_session(db: &SqlitePool, session: &str) -> Option<Principal> {
    let (user_id, is_admin): (i64, bool) = sqlx::query_as(
        "SELECT u.id, u.is_admin FROM sessions s JOIN users u ON u.id = s.user_id
          WHERE s.hash = ? AND s.expires_at > ? AND u.active = 1",
    )
    .bind(sha256_hex(session))
    .bind(now_ms())
    .fetch_optional(db)
    .await
    .ok()??;
    Some(Principal {
        user_id: Some(user_id),
        admin: is_admin,
        projects: None,
        read_only: false,
        via_token: false,
    })
}

// ---------------------------------------------------------------- secrets

const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 43 base62 characters, about 256 bits of entropy from the OS generator.
pub fn new_secret() -> String {
    let mut out = String::with_capacity(43);
    let mut buf = [0u8; 64];
    while out.len() < 43 {
        getrandom::fill(&mut buf).expect("the OS random number generator is unavailable");
        // 248 = 62 * 4: rejecting larger bytes keeps every character equally likely.
        for b in buf.iter().filter(|b| **b < 248) {
            if out.len() == 43 {
                break;
            }
            out.push(BASE62[(*b % 62) as usize] as char);
        }
    }
    out
}

/// A one-time code like `K7QM-3XRD` for claiming the first admin account.
/// No 0/O or 1/I, since people type it from a log.
pub fn new_setup_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut buf = [0u8; 8];
    getrandom::fill(&mut buf).expect("the OS random number generator is unavailable");
    let chars: String = buf.iter().map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect();
    format!("{}-{}", &chars[..4], &chars[4..])
}

pub fn normalize_code(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// Constant-time equality: comparing a secret with `==` short-circuits at
/// the first differing byte, which leaks a fraction of a bit of timing
/// information per attempt. The setup code is short (~40 bits) and this
/// comparison is otherwise the only thing standing in front of it, so it's
/// worth doing properly even though nothing here is reachable off this
/// machine.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Tokens and session ids are long random strings, so a fast hash is the
/// right tool (a slow password hash would add nothing but latency).
pub fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hash_password(password: &str) -> ApiResult<String> {
    if password.chars().count() < 8 {
        return Err(AppError::invalid("Passwords need at least 8 characters"));
    }
    Argon2::default().hash_password(password.as_bytes()).map(|h| h.to_string()).map_err(|_| {
        AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Could not hash the password")
    })
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    PasswordHash::new(stored)
        .is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

/// True until someone has set up an administrator who can sign in.
pub async fn setup_required(db: &SqlitePool) -> bool {
    let admins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE is_admin = 1 AND active = 1 AND password_hash IS NOT NULL",
    )
    .fetch_one(db)
    .await
    .unwrap_or(0);
    admins == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_long_and_distinct() {
        let a = new_secret();
        let b = new_secret();
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| BASE62.contains(&c)));
    }

    #[test]
    fn passwords_round_trip() {
        let h = hash_password("correct horse").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong horse", &h));
        assert!(hash_password("short").is_err());
    }

    #[test]
    fn reads_cookies_and_bearer_tokens() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; kanban_session=abc; b=2".parse().unwrap());
        h.insert(header::AUTHORIZATION, "Bearer kbn_xyz".parse().unwrap());
        assert_eq!(cookie(&h, SESSION_COOKIE).as_deref(), Some("abc"));
        assert_eq!(bearer(&h), Some("kbn_xyz"));
    }

    #[test]
    fn only_health_auth_and_the_ui_are_public() {
        for p in ["/", "/p/KAN/board", "/assets/x.js", "/api/health", "/api/auth/login", "/api/auth/status"] {
            assert!(is_public(p), "{p}");
        }
        for p in ["/api/tickets", "/api/users", "/mcp", "/api/events", "/api"] {
            assert!(!is_public(p), "{p}");
        }
    }
}
