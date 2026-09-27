pub mod api;
pub mod dag;
pub mod demo;
pub mod error;
pub mod events;
pub mod guard;
pub mod models;
pub mod web;

use std::path::Path;
use std::time::Duration;

use axum::Router;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub events: events::Events,
}

pub async fn open_db(path: &Path) -> anyhow::Result<SqlitePool> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new().max_connections(8).connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub fn router(state: AppState) -> Router {
    api::routes()
        .fallback(web::static_handler)
        .with_state(state)
        .layer(axum::middleware::from_fn(guard::middleware))
        .layer(TraceLayer::new_for_http())
}

/// Every mutation goes through here. IMMEDIATE takes the write lock up front,
/// so read-check-write sequences (cycle and height checks, ticket numbering,
/// ranks) cannot interleave with another writer.
pub async fn write_tx(db: &SqlitePool) -> Result<Transaction<'static, Sqlite>, sqlx::Error> {
    db.begin_with("BEGIN IMMEDIATE").await
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
