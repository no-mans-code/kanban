use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqliteConnection;

use crate::auth::{Auth, Principal};
use crate::dag::{Edge, Graph};
use crate::error::{ApiResult, AppError};
use crate::models::{double_option, keys_for};
use crate::{AppState, write_tx};

const MAX_HEIGHT_KEY: &str = "max_dag_height";

/// None means unlimited, which is the default.
pub(crate) async fn max_height(conn: &mut SqliteConnection) -> ApiResult<Option<usize>> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(MAX_HEIGHT_KEY)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(value.and_then(|v| v.parse().ok()))
}

/// The true, full dependency graph. Used only to decide whether a write is
/// actually safe (cycle / height checks) — this must never be scoped to
/// what one caller can see, or the DAG's own invariant (acyclic, bounded)
/// could be broken by an edge that looks safe only because part of the
/// real graph was hidden from the check.
pub(crate) async fn blocks_edges(conn: &mut SqliteConnection) -> ApiResult<Vec<Edge>> {
    Ok(sqlx::query_as("SELECT source_id, target_id FROM ticket_links WHERE kind = 'blocks'")
        .fetch_all(&mut *conn)
        .await?)
}

/// The graph restricted to edges where *both* ends are in a project the
/// caller can see. Display-only: never use this to decide whether a write
/// is allowed, only to decide what a non-admin caller is shown.
pub(crate) async fn blocks_edges_visible(conn: &mut SqliteConnection, p: &Principal) -> ApiResult<Vec<Edge>> {
    let vis = p.visibility(conn).await?;
    let edges = blocks_edges(conn).await?;
    if vis.ids().is_none() {
        return Ok(edges); // site admin: sees everything anyway
    }
    let mut projects_of: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    for &(s, t) in &edges {
        for id in [s, t] {
            if let std::collections::hash_map::Entry::Vacant(e) = projects_of.entry(id) {
                let pid: i64 = sqlx::query_scalar("SELECT project_id FROM tickets WHERE id = ?")
                    .bind(id)
                    .fetch_one(&mut *conn)
                    .await?;
                e.insert(pid);
            }
        }
    }
    Ok(edges.into_iter().filter(|(s, t)| vis.sees(projects_of[s]) && vis.sees(projects_of[t])).collect())
}

#[derive(Serialize)]
pub struct SettingsView {
    max_dag_height: Option<usize>,
    /// Tickets on the longest `blocks` chain **that the caller can see**;
    /// 0 when there are none. A site admin sees the true board-wide figure.
    dag_height: usize,
    longest_chain: Vec<String>,
    is_site_admin: bool,
}

async fn view(conn: &mut SqliteConnection, p: &Principal) -> ApiResult<SettingsView> {
    let edges =
        if p.is_site_admin() { blocks_edges(conn).await? } else { blocks_edges_visible(conn, p).await? };
    let chain = Graph::new(&edges).longest_chain();
    Ok(SettingsView {
        max_dag_height: max_height(conn).await?,
        dag_height: chain.len(),
        longest_chain: keys_for(conn, &chain).await?,
        is_site_admin: p.is_site_admin(),
    })
}

/// Open to anyone signed in — the height limit is a fact about the whole
/// board, but what's shown as the "current longest chain" is scoped to
/// what the caller can see, never a foreign project's ticket keys.
pub async fn get(State(state): State<AppState>, Auth(p): Auth) -> ApiResult<Json<SettingsView>> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(view(&mut conn, &p).await?))
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    /// A positive integer, or null for unlimited.
    #[serde(default, deserialize_with = "double_option")]
    max_dag_height: Option<Option<i64>>,
}

/// Changing the board-wide policy is site-admin-only, the same as creating
/// a project: it's a decision about the whole board, not one project.
pub async fn update(
    State(state): State<AppState>,
    Auth(p): Auth,
    Json(body): Json<UpdateSettings>,
) -> ApiResult<Json<SettingsView>> {
    p.require_site_admin()?;
    let mut tx = write_tx(&state.db).await?;
    match body.max_dag_height {
        None => {}
        Some(None) => {
            sqlx::query("DELETE FROM settings WHERE key = ?").bind(MAX_HEIGHT_KEY).execute(&mut *tx).await?;
        }
        Some(Some(limit)) => {
            if limit < 1 {
                return Err(AppError::invalid("The maximum DAG height must be at least 1"));
            }
            let chain = Graph::new(&blocks_edges(&mut tx).await?).longest_chain();
            if chain.len() as i64 > limit {
                let keys = keys_for(&mut tx, &chain).await?;
                return Err(AppError::conflict(
                    "height_exceeded",
                    format!(
                        "The board already has a dependency chain of {} tickets ({}). \
                         Remove a link in it before lowering the limit below {}.",
                        chain.len(),
                        keys.join(" → "),
                        chain.len()
                    ),
                )
                .with_detail(json!({ "chain": keys, "limit": limit })));
            }
            sqlx::query(
                "INSERT INTO settings (key, value) VALUES (?, ?)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            )
            .bind(MAX_HEIGHT_KEY)
            .bind(limit.to_string())
            .execute(&mut *tx)
            .await?;
        }
    }
    let view = view(&mut tx, &p).await?;
    tx.commit().await?;
    state.events.emit("settings.changed", None, &[]);
    Ok(Json(view))
}
