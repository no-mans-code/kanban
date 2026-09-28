use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use sqlx::SqliteConnection;

use super::links::record as record_link;
use crate::auth::{Auth, Role};
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

/// Ticket keys mentioned in a comment body (e.g. "see KAN-3"), deduplicated.
/// A candidate is a maximal run of ASCII letters/digits/hyphens shaped like
/// a real key: a project key (2-10 chars, a letter then letters/digits),
/// '-', then a number. Mirrors the project-key rules in `projects::clean_key`.
fn mentioned_keys(body: &str) -> Vec<String> {
    let mut out: Vec<String> = body
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter_map(|word| {
            let (project, number) = word.rsplit_once('-')?;
            let project = project.to_ascii_uppercase();
            let mut chars = project.chars();
            let key_ok = (2..=10).contains(&project.len())
                && chars.next().is_some_and(|c| c.is_ascii_uppercase())
                && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
            let number_ok = !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit());
            (key_ok && number_ok).then(|| format!("{project}-{number}"))
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Links `ticket_id` to every ticket mentioned by key in `body` that exists
/// and is visible to `p`, skipping ones already linked in some way. Mentions
/// of the ticket's own key, or of keys that don't resolve or aren't visible,
/// are ignored rather than failing the comment.
async fn link_mentions(
    tx: &mut sqlx::SqliteConnection,
    p: &crate::auth::Principal,
    ticket_id: i64,
    body: &str,
    actor: Option<i64>,
) -> ApiResult<()> {
    for key in mentioned_keys(body) {
        let Ok(mentioned_id) = resolve_key(tx, &key).await else { continue };
        if mentioned_id == ticket_id {
            continue;
        }
        if p.require_ticket(tx, mentioned_id, Role::Viewer).await.is_err() {
            continue;
        }
        let existing: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM ticket_links WHERE kind = 'relates'
              AND ((source_id = ? AND target_id = ?) OR (source_id = ? AND target_id = ?))",
        )
        .bind(ticket_id)
        .bind(mentioned_id)
        .bind(mentioned_id)
        .bind(ticket_id)
        .fetch_one(&mut *tx)
        .await?;
        if existing > 0 {
            continue;
        }
        sqlx::query(
            "INSERT INTO ticket_links (source_id, target_id, kind, created_at) VALUES (?, ?, 'relates', ?)",
        )
        .bind(ticket_id)
        .bind(mentioned_id)
        .bind(now_ms())
        .execute(&mut *tx)
        .await?;
        record_link(tx, ticket_id, mentioned_id, "relates", actor, "linked").await?;
    }
    Ok(())
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

pub async fn list(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(key): Path<String>,
) -> ApiResult<Json<Vec<Comment>>> {
    let mut conn = state.db.acquire().await?;
    let id = resolve_key(&mut conn, &key).await?;
    p.require_ticket(&mut conn, id, Role::Viewer).await?;
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
    Auth(p): Auth,
    Path(key): Path<String>,
    Json(input): Json<CommentBody>,
) -> ApiResult<(StatusCode, Json<Comment>)> {
    p.require_writable()?;
    let body = clean_body(&input.body)?;
    let mut tx = write_tx(&state.db).await?;
    let ticket_id = resolve_key(&mut tx, &key).await?;
    p.require_ticket(&mut tx, ticket_id, Role::Member).await?;
    let actor = p.user_id;
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
    link_mentions(&mut tx, &p, ticket_id, &body, actor).await?;
    let comment = load(&mut tx, id).await?;
    let (project_id, ticket_key) = project_and_key(&mut tx, ticket_id).await?;
    tx.commit().await?;
    state.events.emit("comment.changed", Some(project_id), &[ticket_key]);
    Ok((StatusCode::CREATED, Json(comment)))
}

/// Only the comment's own author, a project admin, or a site admin may
/// change or remove it — membership alone isn't enough to edit someone
/// else's words.
async fn require_owner_or_admin(
    conn: &mut SqliteConnection,
    p: &crate::auth::Principal,
    comment: &Comment,
) -> ApiResult<()> {
    if p.user_id.is_some() && p.user_id == comment.author_id {
        return Ok(());
    }
    let project_id: i64 = sqlx::query_scalar("SELECT project_id FROM tickets WHERE id = ?")
        .bind(comment.ticket_id)
        .fetch_one(&mut *conn)
        .await?;
    p.require(conn, project_id, Role::Admin).await.map(|_| ())
}

pub async fn update(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(id): Path<i64>,
    Json(input): Json<CommentBody>,
) -> ApiResult<Json<Comment>> {
    p.require_writable()?;
    let body = clean_body(&input.body)?;
    let mut tx = write_tx(&state.db).await?;
    let old = load(&mut tx, id).await?;
    require_owner_or_admin(&mut tx, &p, &old).await?;
    let actor = p.user_id;
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
    Auth(p): Auth,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    p.require_writable()?;
    let mut tx = write_tx(&state.db).await?;
    let old = load(&mut tx, id).await?;
    require_owner_or_admin(&mut tx, &p, &old).await?;
    let actor = p.user_id;
    sqlx::query("DELETE FROM comments WHERE id = ?").bind(id).execute(&mut *tx).await?;
    log_activity(&mut tx, old.ticket_id, actor, "comment_deleted", None, Some(excerpt(&old.body)), None)
        .await?;
    reindex(&mut tx, old.ticket_id).await?;
    let (project_id, ticket_key) = project_and_key(&mut tx, old.ticket_id).await?;
    tx.commit().await?;
    state.events.emit("comment.changed", Some(project_id), &[ticket_key]);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::mentioned_keys;

    #[test]
    fn finds_plain_mentions() {
        assert_eq!(mentioned_keys("See KAN-3 for context."), vec!["KAN-3"]);
    }

    #[test]
    fn dedupes_and_sorts() {
        assert_eq!(mentioned_keys("KAN-9 blocks KAN-2, also KAN-9 again"), vec!["KAN-2", "KAN-9"]);
    }

    #[test]
    fn uppercases_lowercase_mentions() {
        assert_eq!(mentioned_keys("fixed in kan-3"), vec!["KAN-3"]);
    }

    #[test]
    fn ignores_non_key_shapes() {
        assert!(mentioned_keys("k-3 A-3 TOOLONGKEY9-3 KAN-3x plain text 2024-01-01").is_empty());
    }

    #[test]
    fn rejects_hyphenated_project_part() {
        // rsplit_once takes the LAST '-', so "PART-OF-KAN-3" has project
        // "PART-OF-KAN" which fails the letters/digits-only check.
        assert!(mentioned_keys("PART-OF-KAN-3").is_empty());
    }

    #[test]
    fn picks_multiple_distinct_mentions() {
        assert_eq!(mentioned_keys("KAN-1 and PER-42 are related"), vec!["KAN-1", "PER-42"]);
    }
}
