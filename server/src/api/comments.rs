use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use sqlx::SqliteConnection;

use super::Actor;
use crate::error::{ApiResult, AppError};
use crate::models::{Comment, log_activity, reindex, resolve_key, watch};
use crate::{AppState, now_ms, write_tx};

const MAX_BODY: usize = 100_000;

fn clean_body(body: &str) -> ApiResult<String> {
    let b = body.trim();
    if b.is_empty() {
        return Err(AppError::invalid("Comment cannot be empty"));
    }
    if b.len() > MAX_BODY {
        return Err(AppError::invalid("Comment is limited to 100,000 bytes"));
    }
    Ok(b.to_string())
}

async fn load(conn: &mut SqliteConnection, id: i64) -> ApiResult<Comment> {
    sqlx::query_as::<_, Comment>(
        "SELECT id, ticket_id, author_id, body, created_at, updated_at FROM comments WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| AppError::not_found("Comment"))
}

async fn project_and_key(conn: &mut SqliteConnection, ticket_id: i64) -> ApiResult<(i64, String)> {
    Ok(sqlx::query_as(
        "SELECT t.project_id, p.key || '-' || t.number FROM tickets t
           JOIN projects p ON p.id = t.project_id WHERE t.id = ?",
    )
    .bind(ticket_id)
    .fetch_one(&mut *conn)
    .await?)
}

/// First line of a comment, for the history log.
fn excerpt(body: &str) -> String {
    let line = body.lines().next().unwrap_or("");
    let mut out: String = line.chars().take(120).collect();
    if out.len() < body.len() {
        out.push('…');
    }
    out
}

pub async fn list(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<Json<Vec<Comment>>> {
    let mut conn = state.db.acquire().await?;
    let id = resolve_key(&mut conn, &key).await?;
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, ticket_id, author_id, body, created_at, updated_at FROM comments
          WHERE ticket_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Json(comments))
}

#[derive(Deserialize)]
pub struct CommentBody {
    body: String,
}

pub async fn create(
    State(state): State<AppState>,
    Actor(actor): Actor,
    Path(key): Path<String>,
    Json(input): Json<CommentBody>,
) -> ApiResult<(StatusCode, Json<Comment>)> {
    let body = clean_body(&input.body)?;
    let mut tx = write_tx(&state.db).await?;
    let ticket_id = resolve_key(&mut tx, &key).await?;
    let now = now_ms();
    let id = sqlx::query(
        "INSERT INTO comments (ticket_id, author_id, body, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(ticket_id)
    .bind(actor)
    .bind(&body)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    sqlx::query("UPDATE tickets SET updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(ticket_id)
        .execute(&mut *tx)
        .await?;
    watch(&mut tx, ticket_id, actor).await?;
    log_activity(&mut tx, ticket_id, actor, "commented", None, None, Some(excerpt(&body))).await?;
    reindex(&mut tx, ticket_id).await?;
    let comment = load(&mut tx, id).await?;
    let (project_id, ticket_key) = project_and_key(&mut tx, ticket_id).await?;
    tx.commit().await?;
    state.events.emit("comment.changed", Some(project_id), &[ticket_key]);
    Ok((StatusCode::CREATED, Json(comment)))
}

pub async fn update(
    State(state): State<AppState>,
    Actor(actor): Actor,
    Path(id): Path<i64>,
    Json(input): Json<CommentBody>,
) -> ApiResult<Json<Comment>> {
    let body = clean_body(&input.body)?;
    let mut tx = write_tx(&state.db).await?;
    let old = load(&mut tx, id).await?;
    if old.body != body {
        sqlx::query("UPDATE comments SET body = ?, updated_at = ? WHERE id = ?")
            .bind(&body)
            .bind(now_ms())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_activity(
            &mut tx,
            old.ticket_id,
            actor,
            "comment_edited",
            None,
            Some(excerpt(&old.body)),
            Some(excerpt(&body)),
        )
        .await?;
        reindex(&mut tx, old.ticket_id).await?;
    }
    let comment = load(&mut tx, id).await?;
    let (project_id, ticket_key) = project_and_key(&mut tx, old.ticket_id).await?;
    tx.commit().await?;
    state.events.emit("comment.changed", Some(project_id), &[ticket_key]);
    Ok(Json(comment))
}

pub async fn delete(
    State(state): State<AppState>,
    Actor(actor): Actor,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let mut tx = write_tx(&state.db).await?;
    let old = load(&mut tx, id).await?;
    sqlx::query("DELETE FROM comments WHERE id = ?").bind(id).execute(&mut *tx).await?;
    log_activity(&mut tx, old.ticket_id, actor, "comment_deleted", None, Some(excerpt(&old.body)), None)
        .await?;
    reindex(&mut tx, old.ticket_id).await?;
    let (project_id, ticket_key) = project_and_key(&mut tx, old.ticket_id).await?;
    tx.commit().await?;
    state.events.emit("comment.changed", Some(project_id), &[ticket_key]);
    Ok(StatusCode::NO_CONTENT)
}
