use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub detail: Option<Value>,
}

pub type ApiResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        AppError { status, code, message: message.into(), detail: None }
    }

    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }

    pub fn not_found(what: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", format!("{what} not found"))
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid", message)
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &e {
            if db.is_unique_violation() {
                return Self::conflict("duplicate", "That already exists");
            }
            if db.is_foreign_key_violation() {
                return Self::invalid("A referenced user, ticket, status or label does not exist");
            }
            if db.is_check_violation() {
                return Self::invalid("A value is not one of the allowed options");
            }
        }
        tracing::error!("database error: {e}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Internal database error")
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": { "code": self.code, "message": self.message, "detail": self.detail }
        });
        (self.status, Json(body)).into_response()
    }
}
