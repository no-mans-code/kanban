//! API tokens: how agents (and scripts) authenticate. A token belongs to an
//! account and can be narrowed to some of that account's projects, made
//! read-only, and given an expiry. The secret is returned once, at creation.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, SqliteConnection};

use crate::auth::{Auth, Principal, TOKEN_PREFIX, new_secret, sha256_hex};
use crate::error::{ApiResult, AppError};
use crate::models::{double_option, project_id_by_key};
use crate::{AppState, now_ms, write_tx};

const DEFAULT_DAYS: i64 = 90;

#[derive(Serialize, FromRow)]
pub struct TokenRow {
    id: i64,
    user_id: i64,
    username: String,
    name: String,
    /// The first characters after `kbn_`, to tell tokens apart in a list.
    hint: String,
    read_only: bool,
    all_projects: bool,
    created_at: i64,
    expires_at: Option<i64>,
    last_used_at: Option<i64>,
    revoked_at: Option<i64>,
}

#[derive(Serialize)]
pub struct TokenView {
    #[serde(flatten)]
    row: TokenRow,
    /// Project keys the token is limited to; null when it has all of its owner's projects.
    projects: Option<Vec<String>>,
}

const TOKEN_SELECT: &str = "SELECT t.id, t.user_id, u.username, t.name, t.hint, t.read_only, t.all_projects,
        t.created_at, t.expires_at, t.last_used_at, t.revoked_at
   FROM api_tokens t JOIN users u ON u.id = t.user_id";

async fn view(conn: &mut SqliteConnection, row: TokenRow) -> ApiResult<TokenView> {
    let projects = if row.all_projects {
        None
    } else {
        Some(
            sqlx::query_scalar(
                "SELECT p.key FROM api_token_projects tp JOIN projects p ON p.id = tp.project_id
                  WHERE tp.token_id = ? ORDER BY p.key",
            )
            .bind(row.id)
            .fetch_all(&mut *conn)
            .await?,
        )
    };
    Ok(TokenView { row, projects })
}

/// Tokens are managed from a signed-in session. A token that could mint
/// tokens would let a narrow, leaked token widen itself.
fn require_session(p: &Principal) -> ApiResult<i64> {
    match p.user_id {
        Some(id) if !p.via_token => Ok(id),
        _ => Err(AppError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Manage API tokens from a signed-in session",
        )),
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    user_id: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    Auth(p): Auth,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<TokenView>>> {
    let me = require_session(&p)?;
    let owner = q.user_id.unwrap_or(me);
    if owner != me {
        p.require_site_admin()?;
    }
    let mut conn = state.db.acquire().await?;
    let rows = sqlx::query_as::<_, TokenRow>(sqlx::AssertSqlSafe(format!(
        "{TOKEN_SELECT} WHERE t.user_id = ? ORDER BY t.revoked_at IS NOT NULL, t.created_at DESC"
    )))
    .bind(owner)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(view(&mut conn, row).await?);
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct CreateToken {
    name: String,
    /// Whose token this is; defaults to the caller. Admins create agents' tokens.
    user_id: Option<i64>,
    /// Project keys; omit for every project the owner can access.
    projects: Option<Vec<String>>,
    #[serde(default)]
    read_only: bool,
    /// Days until it stops working; omit for 90, null for never.
    #[serde(default, deserialize_with = "double_option")]
    expires_in_days: Option<Option<i64>>,
}

pub async fn create(
    State(state): State<AppState>,
    Auth(p): Auth,
    Json(body): Json<CreateToken>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let me = require_session(&p)?;
    let owner = body.user_id.unwrap_or(me);
    if owner != me {
        p.require_site_admin()?;
    }
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::invalid(
            "Give the token a name of 1-80 characters, e.g. \"coder agent on KAN\"",
        ));
    }
    let days = body.expires_in_days.unwrap_or(Some(DEFAULT_DAYS));
    if days.is_some_and(|d| !(1..=3650).contains(&d)) {
        return Err(AppError::invalid("expires_in_days must be between 1 and 3650, or null for never"));
    }

    let mut tx = write_tx(&state.db).await?;
    let (is_admin, active): (bool, bool) = sqlx::query_as("SELECT is_admin, active FROM users WHERE id = ?")
        .bind(owner)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("User"))?;
    if !active {
        return Err(AppError::invalid("That account is deactivated"));
    }
    // A token can never reach further than its owner.
    let owner_access = Principal {
        user_id: Some(owner),
        admin: is_admin,
        projects: None,
        read_only: false,
        via_token: false,
    };
    let visible = owner_access.visibility(&mut tx).await?;
    let mut project_ids = Vec::new();
    for key in body.projects.iter().flatten() {
        let id = project_id_by_key(&mut tx, key).await?;
        if !visible.sees(id) || !p.visibility(&mut tx).await?.sees(id) {
            return Err(AppError::invalid(format!(
                "That account isn't a member of {}",
                key.to_ascii_uppercase()
            )));
        }
        project_ids.push(id);
    }
    if body.projects.as_ref().is_some_and(|p| p.is_empty()) {
        return Err(AppError::invalid("List at least one project, or leave `projects` out for all of them"));
    }

    let secret = new_secret();
    let token = format!("{TOKEN_PREFIX}{secret}");
    let now = now_ms();
    let id = sqlx::query(
        "INSERT INTO api_tokens (user_id, name, hint, hash, read_only, all_projects, created_by, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(owner)
    .bind(name)
    .bind(&secret[..6])
    .bind(sha256_hex(&token))
    .bind(body.read_only)
    .bind(body.projects.is_none())
    .bind(me)
    .bind(now)
    .bind(days.map(|d| now + d * 86_400_000))
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    for project in project_ids {
        sqlx::query("INSERT INTO api_token_projects (token_id, project_id) VALUES (?, ?)")
            .bind(id)
            .bind(project)
            .execute(&mut *tx)
            .await?;
    }
    let row = sqlx::query_as::<_, TokenRow>(sqlx::AssertSqlSafe(format!("{TOKEN_SELECT} WHERE t.id = ?")))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let view = view(&mut tx, row).await?;
    tx.commit().await?;
    let mut out = serde_json::to_value(view).unwrap_or_default();
    out["token"] = json!(token);
    Ok((StatusCode::CREATED, Json(out)))
}

pub async fn revoke(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let me = require_session(&p)?;
    let mut tx = write_tx(&state.db).await?;
    let owner: i64 = sqlx::query_scalar("SELECT user_id FROM api_tokens WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("Token"))?;
    if owner != me {
        p.require_site_admin()?;
    }
    sqlx::query("UPDATE api_tokens SET revoked_at = COALESCE(revoked_at, ?) WHERE id = ?")
        .bind(now_ms())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
