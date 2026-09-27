//! Accounts. Everyone signed in can list them (assignee pickers need names);
//! only site admins create accounts or change other people's.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use sqlx::SqliteConnection;

use crate::auth::{Auth, hash_password};
use crate::error::{ApiResult, AppError};
use crate::models::{PALETTE, USER_SELECT, User};
use crate::{AppState, now_ms, write_tx};

async fn load(conn: &mut SqliteConnection, id: i64) -> ApiResult<User> {
    sqlx::query_as::<_, User>(sqlx::AssertSqlSafe(format!("{USER_SELECT} WHERE id = ?")))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::not_found("User"))
}

pub async fn list(State(state): State<AppState>, Auth(_): Auth) -> ApiResult<Json<Vec<User>>> {
    let users = sqlx::query_as::<_, User>(sqlx::AssertSqlSafe(format!(
        "{USER_SELECT} ORDER BY active DESC, display_name"
    )))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(users))
}

#[derive(Deserialize)]
pub struct CreateUser {
    username: String,
    display_name: Option<String>,
    color: Option<String>,
    /// human (default) | agent
    kind: Option<String>,
    /// Humans only: lets them sign in to the UI.
    password: Option<String>,
    #[serde(default)]
    is_admin: bool,
}

fn clean_username(name: &str) -> ApiResult<String> {
    let n = name.trim();
    let ok = !n.is_empty()
        && n.len() <= 40
        && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok {
        return Err(AppError::invalid(
            "Username must be 1-40 characters of letters, digits, '-', '_' or '.'",
        ));
    }
    Ok(n.to_string())
}

pub(crate) fn clean_color(color: &str) -> ApiResult<String> {
    let c = color.trim();
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    if !ok {
        return Err(AppError::invalid("Color must look like #a1b2c3"));
    }
    Ok(c.to_ascii_lowercase())
}

pub async fn create(
    State(state): State<AppState>,
    Auth(p): Auth,
    Json(body): Json<CreateUser>,
) -> ApiResult<(StatusCode, Json<User>)> {
    p.require_site_admin()?;
    let username = clean_username(&body.username)?;
    let display = body.display_name.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let display = display.unwrap_or(&username).to_string();
    let kind = body.kind.as_deref().unwrap_or("human");
    if !matches!(kind, "human" | "agent") {
        return Err(AppError::invalid("kind must be human or agent"));
    }
    if kind == "agent" && body.password.is_some() {
        return Err(AppError::invalid("Agents sign in with API tokens, not passwords"));
    }
    // Agents CAN be site admins: that's how a manager/master agent gets full
    // access to every project by default, the same as the human operator.
    // Ordinary agents stay unprivileged until added to a project, which is
    // exactly the thing stopping them from picking up work by accident.
    let hash = body.password.as_deref().map(hash_password).transpose()?;
    let mut tx = write_tx(&state.db).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&mut *tx).await?;
    let color = match body.color {
        Some(c) => clean_color(&c)?,
        None => PALETTE[count as usize % PALETTE.len()].to_string(),
    };
    let id = sqlx::query(
        "INSERT INTO users (username, display_name, color, created_at, kind, is_admin, password_hash)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&username)
    .bind(&display)
    .bind(&color)
    .bind(now_ms())
    .bind(kind)
    .bind(body.is_admin)
    .bind(hash)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let user = load(&mut tx, id).await?;
    tx.commit().await?;
    state.events.emit("user.changed", None, &[]);
    Ok((StatusCode::CREATED, Json(user)))
}

#[derive(Deserialize)]
pub struct UpdateUser {
    display_name: Option<String>,
    color: Option<String>,
    active: Option<bool>,
    is_admin: Option<bool>,
    /// Admins can set or reset a human's password.
    password: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUser>,
) -> ApiResult<Json<User>> {
    p.require_writable()?;
    // Anyone may change their own name and color; everything else is admin work.
    let self_service =
        p.user_id == Some(id) && body.active.is_none() && body.is_admin.is_none() && body.password.is_none();
    if !self_service {
        p.require_site_admin()?;
    }
    let mut tx = write_tx(&state.db).await?;
    let user = load(&mut tx, id).await?;
    if let Some(name) = body.display_name {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::invalid("Display name cannot be empty"));
        }
        sqlx::query("UPDATE users SET display_name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(color) = body.color {
        sqlx::query("UPDATE users SET color = ? WHERE id = ?")
            .bind(clean_color(&color)?)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let losing_admin = user.is_admin && (body.active == Some(false) || body.is_admin == Some(false));
    if losing_admin {
        let others: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM users WHERE is_admin = 1 AND active = 1 AND password_hash IS NOT NULL AND id <> ?",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if others == 0 {
            return Err(AppError::conflict(
                "last_admin",
                "Keep at least one active administrator who can sign in",
            ));
        }
    }
    if let Some(active) = body.active {
        sqlx::query("UPDATE users SET active = ? WHERE id = ?")
            .bind(active)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if !active {
            // Deactivating signs them out everywhere; their tokens stop working too.
            sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(id).execute(&mut *tx).await?;
        }
    }
    if let Some(admin) = body.is_admin {
        sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?")
            .bind(admin)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(password) = body.password {
        if user.kind == "agent" {
            return Err(AppError::invalid("Agents sign in with API tokens, not passwords"));
        }
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(hash_password(&password)?)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let user = load(&mut tx, id).await?;
    tx.commit().await?;
    state.events.emit("user.changed", None, &[]);
    Ok(Json(user))
}
