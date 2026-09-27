use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqliteConnection;

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

pub(crate) async fn blocks_edges(conn: &mut SqliteConnection) -> ApiResult<Vec<Edge>> {
    Ok(sqlx::query_as("SELECT source_id, target_id FROM ticket_links WHERE kind = 'blocks'")
        .fetch_all(&mut *conn)
        .await?)
}

#[derive(Serialize)]
pub struct SettingsView {
    max_dag_height: Option<usize>,
    /// Tickets on the longest `blocks` chain; 0 when there are no dependencies.
    dag_height: usize,
    longest_chain: Vec<String>,
}

async fn view(conn: &mut SqliteConnection) -> ApiResult<SettingsView> {
    let chain = Graph::new(&blocks_edges(conn).await?).longest_chain();
    Ok(SettingsView {
        max_dag_height: max_height(conn).await?,
        dag_height: chain.len(),
        longest_chain: keys_for(conn, &chain).await?,
    })
}

pub async fn get(State(state): State<AppState>) -> ApiResult<Json<SettingsView>> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(view(&mut conn).await?))
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    /// A positive integer, or null for unlimited.
    #[serde(default, deserialize_with = "double_option")]
    max_dag_height: Option<Option<i64>>,
}

pub async fn update(
    State(state): State<AppState>,
    Json(body): Json<UpdateSettings>,
) -> ApiResult<Json<SettingsView>> {
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
    let view = view(&mut tx).await?;
    tx.commit().await?;
    state.events.emit("settings.changed", None, &[]);
    Ok(Json(view))
}
