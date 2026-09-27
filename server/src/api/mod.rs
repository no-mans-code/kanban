mod comments;
mod labels;
mod links;
mod projects;
mod settings;
mod tickets;
mod users;

use std::convert::Infallible;

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use futures::{Stream, StreamExt};
use serde_json::{Value, json};
use tokio_stream::wrappers::BroadcastStream;

use crate::AppState;
use crate::error::AppError;

/// Who is making the change, from the `X-Actor: <user id>` header. There is
/// no authentication: the board binds to localhost and trusts its callers,
/// like the rest of ai-system. Absent means "system".
pub struct Actor(pub Option<i64>);

impl<S: Send + Sync> FromRequestParts<S> for Actor {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        match parts.headers.get("x-actor") {
            None => Ok(Actor(None)),
            Some(v) => v
                .to_str()
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .map(|id| Actor(Some(id)))
                .ok_or_else(|| AppError::invalid("X-Actor must be a user id")),
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/events", get(events))
        .route("/api/users", get(users::list).post(users::create))
        .route("/api/users/{id}", patch(users::update))
        .route("/api/projects", get(projects::list).post(projects::create))
        .route("/api/projects/{key}", get(projects::get).patch(projects::update))
        .route("/api/projects/{key}/statuses", get(projects::list_statuses).post(projects::create_status))
        .route("/api/projects/{key}/statuses/order", put(projects::reorder_statuses))
        .route("/api/statuses/{id}", patch(projects::update_status).delete(projects::delete_status))
        .route("/api/labels", get(labels::list).post(labels::create))
        .route("/api/labels/{id}", patch(labels::update).delete(labels::delete))
        .route("/api/tickets", get(tickets::list).post(tickets::create))
        .route("/api/tickets/{key}", get(tickets::get).patch(tickets::update).delete(tickets::delete))
        .route("/api/tickets/{key}/move", post(tickets::move_ticket))
        .route("/api/tickets/{key}/activity", get(tickets::activity))
        .route("/api/tickets/{key}/watchers", post(tickets::add_watcher))
        .route("/api/tickets/{key}/watchers/{user_id}", delete(tickets::remove_watcher))
        .route("/api/tickets/{key}/comments", get(comments::list).post(comments::create))
        .route("/api/comments/{id}", patch(comments::update).delete(comments::delete))
        .route("/api/links", post(links::create).delete(links::delete))
        .route("/api/graph", get(links::graph))
        .route("/api/settings", get(settings::get).patch(settings::update))
}

async fn health(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let tickets: i64 = sqlx::query_scalar("SELECT count(*) FROM tickets").fetch_one(&state.db).await?;
    Ok(Json(json!({ "ok": true, "tickets": tickets })))
}

async fn events(State(state): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).map(|msg| {
        Ok(match msg {
            Ok(data) => Event::default().event("change").data(data),
            // The client fell behind and missed events; tell it to refetch everything.
            Err(_) => Event::default().event("resync").data("{}"),
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
