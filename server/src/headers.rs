//! Response headers that cost nothing and close off whole classes of
//! browser-side attacks: clickjacking, MIME-sniffing, and (via CSP) any
//! script or style this app didn't itself ship, in case something ever
//! finds its way into a response despite the Markdown sanitizer.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

// style-src needs 'unsafe-inline': the UI sets colors (avatars, labels,
// status dots, the DAG graph) via per-element inline styles, which Chromium
// gates on style-src regardless of whether they're set from HTML or from
// JS (element.style.x = ...). That's a CSS-injection surface, not a script
// one — it can reshape the page, not run code — so script-src stays 'self'
// only with no exceptions; that's the boundary that actually matters.
const CSP: &str = "default-src 'self'; base-uri 'none'; form-action 'self'; \
     script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
     connect-src 'self'; frame-ancestors 'none'; object-src 'none'";

pub async fn middleware(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("cross-origin-opener-policy", HeaderValue::from_static("same-origin"));
    h.insert("cross-origin-resource-policy", HeaderValue::from_static("same-origin"));
    h.insert("content-security-policy", HeaderValue::from_static(CSP));
    res
}
