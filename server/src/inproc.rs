//! Calling the HTTP API from inside the server. Demo seeding and the MCP
//! tools go through the same handlers as a browser would, so they get the
//! same validation, DAG checks, history and live events.

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

pub async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    actor: Option<i64>,
    body: Option<Value>,
) -> anyhow::Result<(u16, Value)> {
    let mut req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    if let Some(a) = actor {
        req = req.header("x-actor", a.to_string());
    }
    let req = req.body(match body {
        Some(b) => Body::from(b.to_string()),
        None => Body::empty(),
    })?;
    let res = app.clone().oneshot(req).await?;
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await?.to_bytes();
    let value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes)? };
    Ok((status, value))
}

/// Percent-encode a value for a URL path segment or query string.
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
