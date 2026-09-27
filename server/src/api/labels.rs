use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use super::users::clean_color;
use crate::error::{ApiResult, AppError};
use crate::models::{Label, PALETTE};
use crate::{AppState, write_tx};

fn clean_label(name: &str) -> ApiResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 40 || n.contains(',') {
        return Err(AppError::invalid("Label must be 1-40 characters with no commas"));
    }
    Ok(n.to_string())
}

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Label>>> {
    let labels = sqlx::query_as::<_, Label>("SELECT id, name, color FROM labels ORDER BY name")
        .fetch_all(&state.db)
        .await?;
    Ok(Json(labels))
}

#[derive(Deserialize)]
pub struct CreateLabel {
    name: String,
    color: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateLabel>,
) -> ApiResult<(StatusCode, Json<Label>)> {
    let name = clean_label(&body.name)?;
    let mut tx = write_tx(&state.db).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM labels").fetch_one(&mut *tx).await?;
    let color = match body.color {
        Some(c) => clean_color(&c)?,
        None => PALETTE[count as usize % PALETTE.len()].to_string(),
    };
    let id = sqlx::query("INSERT INTO labels (name, color) VALUES (?, ?)")
        .bind(&name)
        .bind(&color)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    tx.commit().await?;
    state.events.emit("label.changed", None, &[]);
    Ok((StatusCode::CREATED, Json(Label { id, name, color })))
}

#[derive(Deserialize)]
pub struct UpdateLabel {
    name: Option<String>,
    color: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateLabel>,
) -> ApiResult<Json<Label>> {
    let mut tx = write_tx(&state.db).await?;
    if let Some(name) = body.name {
        sqlx::query("UPDATE labels SET name = ? WHERE id = ?")
            .bind(clean_label(&name)?)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(color) = body.color {
        sqlx::query("UPDATE labels SET color = ? WHERE id = ?")
            .bind(clean_color(&color)?)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let label = sqlx::query_as::<_, Label>("SELECT id, name, color FROM labels WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("Label"))?;
    tx.commit().await?;
    state.events.emit("label.changed", None, &[]);
    Ok(Json(label))
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let mut tx = write_tx(&state.db).await?;
    let deleted = sqlx::query("DELETE FROM labels WHERE id = ?").bind(id).execute(&mut *tx).await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::not_found("Label"));
    }
    tx.commit().await?;
    state.events.emit("label.changed", None, &[]);
    Ok(StatusCode::NO_CONTENT)
}
