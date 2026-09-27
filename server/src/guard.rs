//! Keeps web pages from reaching a board that only means to serve this
//! machine. Binding to 127.0.0.1 stops other computers, but not a web page
//! open in the user's own browser:
//!
//! - DNS rebinding: a hostile domain resolves to 127.0.0.1, so the browser
//!   treats the page as same-origin with the board. Its requests carry the
//!   hostile name in `Host`, which is how we catch them.
//! - Cross-site requests carry the page's `Origin`.
//!
//! A request passes only if both headers, when present, name an allowed host.

use std::sync::LazyLock;

use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::AppError;

const LOOPBACK: [&str; 4] = ["localhost", "127.0.0.1", "[::1]", "::1"];

/// Loopback names plus `KANBAN_ALLOWED_HOSTS` (comma-separated host names,
/// for anyone who deliberately serves the board on a LAN address).
static ALLOWED: LazyLock<Vec<String>> = LazyLock::new(|| {
    let extra = std::env::var("KANBAN_ALLOWED_HOSTS").unwrap_or_default();
    LOOPBACK
        .iter()
        .map(|h| h.to_string())
        .chain(extra.split(',').map(|h| h.trim().to_ascii_lowercase()).filter(|h| !h.is_empty()))
        .collect()
});

/// Host name without the port: `localhost:8610` -> `localhost`, `[::1]:8610` -> `[::1]`.
fn host_name(authority: &str) -> String {
    let a = authority.trim().to_ascii_lowercase();
    if a.starts_with('[') {
        return a.split_inclusive(']').next().unwrap_or(&a).to_string();
    }
    match a.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host.to_string(),
        _ => a,
    }
}

fn allowed(authority: &str) -> bool {
    ALLOWED.contains(&host_name(authority))
}

/// `http://localhost:5173` -> `localhost:5173`. `null` and garbage yield None.
fn origin_authority(origin: &str) -> Option<&str> {
    let (_, rest) = origin.split_once("://")?;
    Some(rest.split('/').next().unwrap_or(rest))
}

pub fn check(headers: &HeaderMap) -> Result<(), &'static str> {
    // In-process calls (tests, demo seeding) have no Host; browsers always send one.
    if let Some(host) = headers.get(header::HOST)
        && !host.to_str().is_ok_and(allowed)
    {
        return Err("Host is not allowed. Set KANBAN_ALLOWED_HOSTS to serve the board under another name.");
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let ok = origin.to_str().ok().and_then(origin_authority).is_some_and(allowed);
        if !ok {
            return Err("Cross-site requests are not allowed.");
        }
    }
    Ok(())
}

pub async fn middleware(req: Request, next: Next) -> Response {
    match check(req.headers()) {
        Ok(()) => next.run(req).await,
        Err(message) => AppError::new(StatusCode::FORBIDDEN, "forbidden_origin", message).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ports_and_brackets() {
        assert_eq!(host_name("localhost:8610"), "localhost");
        assert_eq!(host_name("127.0.0.1"), "127.0.0.1");
        assert_eq!(host_name("[::1]:8610"), "[::1]");
        assert_eq!(host_name("Example.COM:80"), "example.com");
    }

    #[test]
    fn loopback_only_by_default() {
        for ok in ["localhost:8610", "127.0.0.1:5173", "[::1]:8610", "localhost"] {
            assert!(allowed(ok), "{ok}");
        }
        for bad in [
            "attacker.example:8610",
            "127.0.0.1.attacker.example",
            "localhost.attacker.example",
            "10.0.0.5:8610",
        ] {
            assert!(!allowed(bad), "{bad}");
        }
    }

    #[test]
    fn origins() {
        assert_eq!(origin_authority("http://localhost:5173"), Some("localhost:5173"));
        assert_eq!(origin_authority("null"), None);
    }
}
