pub mod api;
pub mod auth;
pub mod dag;
pub mod demo;
pub mod error;
pub mod events;
pub mod guard;
pub mod headers;
pub mod inproc;
pub mod mcp;
pub mod models;
pub mod ratelimit;
pub mod web;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub events: events::Events,
    /// The one-time code for creating the first administrator. Only held in
    /// memory and printed to the server log, never stored.
    pub setup_code: Arc<Mutex<Option<String>>>,
    /// Throttles login and setup attempts, keyed by username / "setup".
    pub login_limiter: Arc<ratelimit::RateLimiter>,
    /// Send `Secure` on the session cookie — set this once the board is
    /// actually served over HTTPS (e.g. behind a reverse proxy); leave it
    /// off for plain-HTTP localhost use, where `Secure` would just make the
    /// browser silently refuse to ever send the cookie back.
    pub cookie_secure: bool,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        AppState {
            db,
            events: events::Events::default(),
            setup_code: Arc::new(Mutex::new(None)),
            login_limiter: Arc::new(ratelimit::RateLimiter::default()),
            cookie_secure: false,
        }
    }
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

/// Requests pass the host guard, then authentication, then reach a route.
/// Every response gets the security headers on the way back out.
pub fn router(state: AppState) -> Router {
    api::routes()
        .fallback(web::static_handler)
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth::middleware))
        .with_state(state)
        .layer(axum::middleware::from_fn(guard::middleware))
        .layer(axum::middleware::from_fn(headers::middleware))
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
