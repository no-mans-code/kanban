use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::error::{ApiResult, AppError};
use crate::models::{PALETTE, User};
use crate::{AppState, now_ms, write_tx};

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<User>>> {
    let users = sqlx::query_as::<_, User>("SELECT id, username, display_name, color, active, created_at FROM users ORDER BY active DESC, display_name")
        .fetch_all(&state.db)
        .await?;
    Ok(Json(users))
}

#[derive(Deserialize)]
pub struct CreateUser {
    username: String,
    display_name: Option<String>,
    color: Option<String>,
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
    Json(body): Json<CreateUser>,
) -> ApiResult<(StatusCode, Json<User>)> {
    let username = clean_username(&body.username)?;
    let display = body.display_name.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let display = display.unwrap_or(&username).to_string();
    let mut tx = write_tx(&state.db).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&mut *tx).await?;
    let color = match body.color {
        Some(c) => clean_color(&c)?,
        None => PALETTE[count as usize % PALETTE.len()].to_string(),
    };
    let id = sqlx::query("INSERT INTO users (username, display_name, color, created_at) VALUES (?, ?, ?, ?)")
        .bind(&username)
        .bind(&display)
        .bind(&color)
        .bind(now_ms())
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    let user = sqlx::query_as::<_, User>(
        "SELECT id, username, display_name, color, active, created_at FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    state.events.emit("user.changed", None, &[]);
    Ok((StatusCode::CREATED, Json(user)))
}

#[derive(Deserialize)]
pub struct UpdateUser {
    display_name: Option<String>,
    color: Option<String>,
    active: Option<bool>,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUser>,
) -> ApiResult<Json<User>> {
    let mut tx = write_tx(&state.db).await?;
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM users WHERE id = ?").bind(id).fetch_optional(&mut *tx).await?;
    exists.ok_or_else(|| AppError::not_found("User"))?;
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
    if let Some(active) = body.active {
        sqlx::query("UPDATE users SET active = ? WHERE id = ?")
            .bind(active)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let user = sqlx::query_as::<_, User>(
        "SELECT id, username, display_name, color, active, created_at FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    state.events.emit("user.changed", None, &[]);
    Ok(Json(user))
}
