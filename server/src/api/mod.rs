mod auth;
mod comments;
mod deletion;
mod labels;
mod links;
mod projects;
mod settings;
mod tickets;
mod tokens;
mod users;

use std::collections::HashSet;
use std::convert::Infallible;

use axum::Json;
use axum::Router;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{delete, get, patch, post, put};
use futures::{Stream, StreamExt};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tokio_stream::wrappers::BroadcastStream;

use crate::AppState;
pub use crate::auth::Auth;
use crate::auth::Principal;
use axum::extract::State;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/mcp", post(crate::mcp::http).get(crate::mcp::no_stream))
        .route("/api/health", get(health))
        .route("/api/events", get(events))
        .route("/api/auth/status", get(auth::status))
        .route("/api/auth/setup", post(auth::setup))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/password", post(auth::change_password))
        .route("/api/tokens", get(tokens::list).post(tokens::create))
        .route("/api/tokens/{id}", delete(tokens::revoke))
        .route("/api/users", get(users::list).post(users::create))
        .route("/api/users/{id}", patch(users::update))
        .route("/api/projects", get(projects::list).post(projects::create))
        .route("/api/projects/{key}", get(projects::get).patch(projects::update))
        .route("/api/projects/{key}/members", get(projects::list_members))
        .route(
            "/api/projects/{key}/members/{user_id}",
            put(projects::set_member).delete(projects::remove_member),
        )
        .route("/api/projects/{key}/statuses", get(projects::list_statuses).post(projects::create_status))
        .route("/api/projects/{key}/statuses/order", put(projects::reorder_statuses))
        .route("/api/statuses/{id}", patch(projects::update_status).delete(projects::delete_status))
        .route(
            "/api/projects/{key}/components",
            get(projects::list_components).post(projects::create_component),
        )
        .route("/api/components/{id}", delete(projects::delete_component))
        .route("/api/projects/{key}/versions", get(projects::list_versions).post(projects::create_version))
        .route("/api/versions/{id}", delete(projects::delete_version))
        .route("/api/labels", get(labels::list).post(labels::create))
        .route("/api/labels/{id}", patch(labels::update).delete(labels::delete))
        .route("/api/tickets", get(tickets::list).post(tickets::create))
        .route("/api/tickets/{key}", get(tickets::get).patch(tickets::update).delete(deletion::delete))
        .route("/api/tickets/{key}/move", post(tickets::move_ticket))
        .route("/api/tickets/{key}/delete-plan", get(deletion::plan))
        .route("/api/tickets/{key}/activity", get(tickets::activity))
        .route("/api/tickets/{key}/watchers", post(tickets::add_watcher))
        .route("/api/tickets/{key}/watchers/{user_id}", delete(tickets::remove_watcher))
        .route("/api/tickets/{key}/comments", get(comments::list).post(comments::create))
        .route("/api/comments/{id}", patch(comments::update).delete(comments::delete))
        .route("/api/links", post(links::create).delete(links::delete))
        .route("/api/graph", get(links::graph))
        .route("/api/settings", get(settings::get).patch(settings::update))
}

/// Public, so it says nothing about the board's contents.
async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// Keeps only what this subscriber may see: an event about a project they
/// can't see is dropped, and invisible ticket keys are removed. Checked per
/// event, so membership changes apply immediately.
async fn visible_event(db: &SqlitePool, principal: &Principal, data: String) -> Option<String> {
    let mut event: Value = serde_json::from_str(&data).ok()?;
    let mut conn = db.acquire().await.ok()?;
    let vis = principal.visibility(&mut conn).await.ok()?;
    if let Some(pid) = event["project_id"].as_i64()
        && !vis.sees(pid)
    {
        return None;
    }
    let keys: Vec<String> =
        event["keys"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(String::from)).collect();
    if keys.is_empty() {
        return Some(data);
    }
    let projects: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, key FROM projects").fetch_all(&mut *conn).await.ok()?;
    let visible: HashSet<String> =
        projects.into_iter().filter(|(id, _)| vis.sees(*id)).map(|(_, key)| key).collect();
    let kept: Vec<String> = keys
        .into_iter()
        .filter(|k| k.rsplit_once('-').is_some_and(|(project, _)| visible.contains(project)))
        .collect();
    if kept.is_empty() {
        return None;
    }
    event["keys"] = json!(kept);
    Some(event.to_string())
}

async fn events(
    State(state): State<AppState>,
    Auth(principal): Auth,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let db = state.db.clone();
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(move |msg| {
        let (db, principal) = (db.clone(), principal.clone());
        async move {
            match msg {
                Ok(data) => visible_event(&db, &principal, data)
                    .await
                    .map(|d| Ok(Event::default().event("change").data(d))),
                // The client fell behind and missed events; tell it to refetch everything.
                Err(_) => Some(Ok(Event::default().event("resync").data("{}"))),
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
