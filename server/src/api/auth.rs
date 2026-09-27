//! Signing in: first-run setup, password login, logout, and the session
//! cookie the web UI uses. Agents never come through here; they use tokens.

use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::FromRow;

use crate::auth::{
    Auth, Principal, SESSION_COOKIE, SESSION_DAYS, constant_time_eq, cookie, hash_password, new_secret,
    normalize_code, setup_required, sha256_hex, verify_password,
};
use crate::error::{ApiResult, AppError};
use crate::models::PALETTE;
use crate::{AppState, now_ms, write_tx};

#[derive(Serialize, FromRow)]
pub struct Me {
    id: i64,
    username: String,
    display_name: String,
    color: String,
    kind: String,
    is_admin: bool,
}

async fn me(state: &AppState, user_id: i64) -> ApiResult<Me> {
    Ok(sqlx::query_as::<_, Me>(
        "SELECT id, username, display_name, color, kind, is_admin FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?)
}

fn session_cookie(state: &AppState, value: &str, max_age_secs: i64) -> HeaderValue {
    // HttpOnly: page scripts can't read it. SameSite=Strict: other sites can't
    // make the browser send it, which (with the host guard) stops CSRF.
    let secure = if state.cookie_secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age_secs}{secure}")
        .parse()
        .expect("cookie is ASCII")
}

/// 429 with a Retry-After header, for login/setup rate limiting.
fn too_many(retry_after_secs: u64) -> AppError {
    AppError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        format!("Too many attempts. Try again in {retry_after_secs}s."),
    )
}

async fn start_session(state: &AppState, user_id: i64) -> ApiResult<HeaderValue> {
    let secret = new_secret();
    let now = now_ms();
    sqlx::query("INSERT INTO sessions (hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
        .bind(sha256_hex(&secret))
        .bind(user_id)
        .bind(now)
        .bind(now + SESSION_DAYS * 86_400_000)
        .execute(&state.db)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?").bind(now).execute(&state.db).await?;
    Ok(session_cookie(state, &secret, SESSION_DAYS * 86_400))
}

async fn status_body(state: &AppState, principal: Option<&Principal>) -> ApiResult<Value> {
    let user = match principal.and_then(|p| p.user_id) {
        Some(id) => Some(me(state, id).await?),
        None => None,
    };
    Ok(json!({
        "setup_required": setup_required(&state.db).await,
        "user": user,
        "via": principal.map(|p| if p.via_token { "token" } else { "session" }),
    }))
}

pub async fn status(
    State(state): State<AppState>,
    principal: Option<Extension<Principal>>,
) -> ApiResult<Json<Value>> {
    Ok(Json(status_body(&state, principal.as_ref().map(|p| &p.0)).await?))
}

#[derive(Deserialize)]
pub struct Setup {
    code: String,
    username: String,
    display_name: Option<String>,
    password: String,
}

/// First run only: create (or claim) the first administrator. Requires the
/// one-time code from the server log, so only whoever runs the server can
/// do it.
pub async fn setup(State(state): State<AppState>, Json(body): Json<Setup>) -> ApiResult<Response> {
    if !setup_required(&state.db).await {
        return Err(AppError::conflict(
            "already_set_up",
            "This board already has an administrator. Sign in instead.",
        ));
    }
    if let Err(retry) = state.login_limiter.check("setup", 10, Duration::from_secs(900)) {
        return Err(too_many(retry));
    }
    let expected = state.setup_code.lock().expect("setup code lock").clone();
    let matches = expected
        .as_deref()
        .is_some_and(|e| constant_time_eq(&normalize_code(e), &normalize_code(&body.code)));
    if !matches {
        tokio::time::sleep(Duration::from_millis(400)).await;
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "wrong_setup_code",
            "That setup code is wrong. The server prints it in its log when it starts.",
        ));
    }
    let username = body.username.trim();
    if username.is_empty()
        || !username.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(AppError::invalid("Username must be letters, digits, '-', '_' or '.'"));
    }
    let hash = hash_password(&body.password)?;
    let mut tx = write_tx(&state.db).await?;
    let existing: Option<(i64, String)> = sqlx::query_as("SELECT id, kind FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&mut *tx)
        .await?;
    let id = match existing {
        Some((_, kind)) if kind == "agent" => {
            return Err(AppError::invalid("That username belongs to an agent; pick another one"));
        }
        Some((id, _)) => {
            sqlx::query("UPDATE users SET password_hash = ?, is_admin = 1, active = 1 WHERE id = ?")
                .bind(&hash)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            id
        }
        None => {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&mut *tx).await?;
            let display =
                body.display_name.as_deref().map(str::trim).filter(|d| !d.is_empty()).unwrap_or(username);
            sqlx::query(
                "INSERT INTO users (username, display_name, color, created_at, kind, is_admin, password_hash)
                 VALUES (?, ?, ?, ?, 'human', 1, ?)",
            )
            .bind(username)
            .bind(display)
            .bind(PALETTE[count as usize % PALETTE.len()])
            .bind(now_ms())
            .bind(&hash)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid()
        }
    };
    tx.commit().await?;
    *state.setup_code.lock().expect("setup code lock") = None;
    state.events.emit("user.changed", None, &[]);
    let cookie = start_session(&state, id).await?;
    let admin =
        Principal { user_id: Some(id), admin: true, projects: None, read_only: false, via_token: false };
    let reply = status_body(&state, Some(&admin)).await?;
    Ok(([(header::SET_COOKIE, cookie)], Json(reply)).into_response())
}

#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
}

pub async fn login(State(state): State<AppState>, Json(body): Json<Login>) -> ApiResult<Response> {
    let username = body.username.trim();
    // Keyed by username, not by whether it exists, so this can't itself be
    // used to enumerate accounts.
    if let Err(retry) = state.login_limiter.check(
        &format!("login:{}", username.to_ascii_lowercase()),
        10,
        Duration::from_secs(900),
    ) {
        return Err(too_many(retry));
    }
    let row: Option<(i64, Option<String>, bool)> = sqlx::query_as(
        "SELECT id, password_hash, is_admin FROM users WHERE username = ? AND active = 1 AND kind = 'human'",
    )
    .bind(username)
    .fetch_optional(&state.db)
    .await?;
    let ok = row
        .as_ref()
        .and_then(|(_, hash, _)| hash.as_deref())
        .is_some_and(|h| verify_password(&body.password, h));
    let Some((id, _, is_admin)) = row.filter(|_| ok) else {
        // Slows guessing without locking anyone out.
        tokio::time::sleep(Duration::from_millis(400)).await;
        return Err(AppError::new(StatusCode::UNAUTHORIZED, "bad_credentials", "Wrong username or password"));
    };
    let cookie = start_session(&state, id).await?;
    let principal =
        Principal { user_id: Some(id), admin: is_admin, projects: None, read_only: false, via_token: false };
    let reply = status_body(&state, Some(&principal)).await?;
    Ok(([(header::SET_COOKIE, cookie)], Json(reply)).into_response())
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(session) = cookie(&headers, SESSION_COOKIE) {
        sqlx::query("DELETE FROM sessions WHERE hash = ?")
            .bind(sha256_hex(&session))
            .execute(&state.db)
            .await?;
    }
    Ok(([(header::SET_COOKIE, session_cookie(&state, "", 0))], StatusCode::NO_CONTENT).into_response())
}

#[derive(Deserialize)]
pub struct PasswordChange {
    current: String,
    new: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    Auth(p): Auth,
    Json(body): Json<PasswordChange>,
) -> ApiResult<StatusCode> {
    let user = p.user_id.filter(|_| !p.via_token).ok_or_else(|| {
        AppError::new(StatusCode::FORBIDDEN, "forbidden", "Change your password from a signed-in session")
    })?;
    let stored: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(user)
        .fetch_one(&state.db)
        .await?;
    if !stored.as_deref().is_some_and(|h| verify_password(&body.current, h)) {
        tokio::time::sleep(Duration::from_millis(400)).await;
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "bad_credentials",
            "Your current password is wrong",
        ));
    }
    let hash = hash_password(&body.new)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash)
        .bind(user)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
