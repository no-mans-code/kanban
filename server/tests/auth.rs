//! End-to-end tests over the real authentication surface: HTTP headers and
//! cookies, exactly as a browser or an agent would send them. No Principal
//! is ever attached directly here — that shortcut belongs to tests/api.rs,
//! which tests ticket/DAG business logic, not this.

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use kanban_server::{AppState, open_db, router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct TestApp {
    app: Router,
    state: AppState,
    _dir: tempfile::TempDir,
}

impl TestApp {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir.path().join("test.db")).await.unwrap();
        let state = AppState::new(db);
        let app = router(state.clone());
        TestApp { app, state, _dir: dir }
    }

    async fn raw(
        &self,
        method: &str,
        uri: &str,
        auth: Option<&str>,
        body: Option<Value>,
    ) -> (u16, axum::http::HeaderMap, Value) {
        let mut req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
        if let Some(a) = auth {
            req = req.header(
                if a.starts_with("kbn_") { "authorization" } else { "cookie" },
                if a.starts_with("kbn_") { format!("Bearer {a}") } else { a.into() },
            );
        }
        let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_default()).unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        // Some public, non-API routes (the frontend shell, favicon) answer
        // with HTML/SVG, not JSON.
        (status, headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// Sets a deterministic setup code and claims the admin account.
    async fn bootstrap_admin(&self) -> String {
        *self.state.setup_code.lock().unwrap() = Some("ABCD-1234".into());
        let (status, headers, _) = self
            .post_public(
                "/api/auth/setup",
                json!({ "code": "abcd-1234", "username": "admin", "password": "correcthorsebattery" }),
            )
            .await;
        assert_eq!(status, 200, "setup should accept the code regardless of case/dashes");
        headers["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string()
    }

    async fn post_public(&self, uri: &str, body: Value) -> (u16, axum::http::HeaderMap, Value) {
        let req = Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        // Some public, non-API routes (the frontend shell, favicon) answer
        // with HTML/SVG, not JSON.
        (status, headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
}

#[tokio::test]
async fn every_api_route_needs_authentication_except_the_public_few() {
    let t = TestApp::new().await;
    for uri in
        ["/api/tickets", "/api/users", "/api/projects", "/api/labels", "/api/settings", "/api/events", "/mcp"]
    {
        let (status, _, _) = t.raw("GET", uri, None, None).await;
        assert_eq!(status, 401, "{uri} must require auth");
    }
    for uri in ["/api/health", "/api/auth/status", "/", "/favicon.svg"] {
        let (status, _, _) = t.raw("GET", uri, None, None).await;
        assert_ne!(status, 401, "{uri} should be reachable without auth");
    }
}

#[tokio::test]
async fn security_headers_are_present() {
    let t = TestApp::new().await;
    let (_, headers, _) = t.raw("GET", "/api/health", None, None).await;
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["x-frame-options"], "DENY");
    assert!(headers["content-security-policy"].to_str().unwrap().contains("default-src 'self'"));
    assert_eq!(headers["referrer-policy"], "no-referrer");
}

#[tokio::test]
async fn setup_then_login_then_logout() {
    let t = TestApp::new().await;

    let (status, _, status_body) = t.raw("GET", "/api/auth/status", None, None).await;
    assert_eq!(status, 200);
    assert_eq!(status_body["setup_required"], true);

    // The wrong code is refused.
    *t.state.setup_code.lock().unwrap() = Some("RIGHT-CODE".into());
    let (status, _, err) = t
        .post_public(
            "/api/auth/setup",
            json!({ "code": "WRONG-CODE", "username": "admin", "password": "correcthorsebattery" }),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(err["error"]["code"], "wrong_setup_code");

    let cookie = t.bootstrap_admin().await;
    let (status, _, me) = t.raw("GET", "/api/auth/status", Some(&cookie), None).await;
    assert_eq!(status, 200);
    assert_eq!(me["user"]["username"], "admin");
    assert_eq!(me["user"]["is_admin"], true);
    assert_eq!(me["setup_required"], false, "no second admin can be created without a fresh code");

    let (status, _, _) = t
        .post_public(
            "/api/auth/setup",
            json!({ "code": "RIGHT-CODE", "username": "someone-else", "password": "correcthorsebattery" }),
        )
        .await;
    assert_eq!(status, 409, "setup only runs once");

    // A short password is refused up front.
    let (status, _, err) = t
        .raw(
            "POST",
            "/api/users",
            Some(&cookie),
            Some(json!({ "username": "x", "kind": "human", "password": "short" })),
        )
        .await;
    assert_eq!(status, 400);
    assert!(err["error"]["message"].as_str().unwrap().contains("8 characters"));

    // Wrong password.
    let (status, _, _) =
        t.post_public("/api/auth/login", json!({ "username": "admin", "password": "nope" })).await;
    assert_eq!(status, 401);
    // Right password gets a fresh session.
    let (status, _, login) = t
        .post_public("/api/auth/login", json!({ "username": "admin", "password": "correcthorsebattery" }))
        .await;
    assert_eq!(status, 200);
    assert_eq!(login["user"]["username"], "admin");

    let (status, _, _) = t.raw("POST", "/api/auth/logout", Some(&cookie), None).await;
    assert_eq!(status, 204);
    let (status, _, _) = t.raw("GET", "/api/tickets", Some(&cookie), None).await;
    assert_eq!(status, 401, "the session is really gone, not just forgotten client-side");
}

#[tokio::test]
async fn repeated_bad_logins_are_rate_limited() {
    let t = TestApp::new().await;
    t.bootstrap_admin().await;

    let mut last = 0;
    for i in 0..12 {
        let (status, _, _) =
            t.post_public("/api/auth/login", json!({ "username": "admin", "password": "wrong" })).await;
        last = status;
        if status == 429 {
            assert!(
                i >= 10,
                "should allow a reasonable number of attempts before limiting, got blocked at {i}"
            );
            return;
        }
    }
    panic!("expected rate limiting to kick in, last status was {last}");
}

#[tokio::test]
async fn project_visibility_scopes_listing_and_is_a_404_not_a_403() {
    let t = TestApp::new().await;
    let admin = t.bootstrap_admin().await;

    for key in ["ALPHA", "BETA"] {
        let (status, _, _) =
            t.raw("POST", "/api/projects", Some(&admin), Some(json!({ "key": key, "name": key }))).await;
        assert_eq!(status, 201);
    }
    let (_, _, u) = t
        .raw("POST", "/api/users", Some(&admin), Some(json!({ "username": "viewer-alice", "kind": "agent" })))
        .await;
    let alice = u["id"].as_i64().unwrap();
    let (status, _, _) = t
        .raw(
            "PUT",
            &format!("/api/projects/ALPHA/members/{alice}"),
            Some(&admin),
            Some(json!({ "role": "member" })),
        )
        .await;
    assert_eq!(status, 200);
    let (_, _, token) =
        t.raw("POST", "/api/tokens", Some(&admin), Some(json!({ "name": "alice", "user_id": alice }))).await;
    let alice_token = token["token"].as_str().unwrap();

    for (project, title) in [("ALPHA", "visible ticket"), ("BETA", "invisible ticket")] {
        let (status, _, _) = t
            .raw("POST", "/api/tickets", Some(&admin), Some(json!({ "project": project, "title": title })))
            .await;
        assert_eq!(status, 201);
    }

    // No project filter: only ALPHA's ticket comes back, never BETA's.
    let (status, _, list) = t.raw("GET", "/api/tickets", Some(alice_token), None).await;
    assert_eq!(status, 200);
    let titles: Vec<&str> = list.as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect();
    assert_eq!(titles, vec!["visible ticket"]);

    // Asking for BETA directly is refused as though it doesn't exist.
    let (status, _, _) = t.raw("GET", "/api/tickets?project=BETA", Some(alice_token), None).await;
    assert_eq!(status, 404);
    let (status, _, _) = t.raw("GET", "/api/projects/BETA", Some(alice_token), None).await;
    assert_eq!(status, 404);
    let (status, _, _) = t.raw("GET", "/api/tickets/BETA-1", Some(alice_token), None).await;
    assert_eq!(status, 404);

    // list_projects only shows ALPHA.
    let (_, _, projects) = t.raw("GET", "/api/projects", Some(alice_token), None).await;
    let keys: Vec<&str> = projects.as_array().unwrap().iter().map(|p| p["key"].as_str().unwrap()).collect();
    assert_eq!(keys, vec!["ALPHA"]);
}

#[tokio::test]
async fn roles_gate_write_and_admin_actions() {
    let t = TestApp::new().await;
    let admin = t.bootstrap_admin().await;
    t.raw("POST", "/api/projects", Some(&admin), Some(json!({ "key": "KAN", "name": "Kanban" }))).await;

    let mut ids = std::collections::HashMap::new();
    for (name, role) in [("view-bot", "viewer"), ("work-bot", "member"), ("admin-bot", "admin")] {
        let (_, _, u) = t
            .raw("POST", "/api/users", Some(&admin), Some(json!({ "username": name, "kind": "agent" })))
            .await;
        let id = u["id"].as_i64().unwrap();
        t.raw("PUT", &format!("/api/projects/KAN/members/{id}"), Some(&admin), Some(json!({ "role": role })))
            .await;
        let (_, _, tok) =
            t.raw("POST", "/api/tokens", Some(&admin), Some(json!({ "name": name, "user_id": id }))).await;
        ids.insert(name, tok["token"].as_str().unwrap().to_string());
    }

    // Viewer: can read, cannot create.
    let (status, _, _) = t.raw("GET", "/api/tickets?project=KAN", Some(&ids["view-bot"]), None).await;
    assert_eq!(status, 200);
    let (status, _, err) = t
        .raw(
            "POST",
            "/api/tickets",
            Some(&ids["view-bot"]),
            Some(json!({ "project": "KAN", "title": "nope" })),
        )
        .await;
    assert_eq!(status, 403, "{err}");

    // Member: can create tickets, cannot manage members or the workflow.
    let (status, _, t1) = t
        .raw(
            "POST",
            "/api/tickets",
            Some(&ids["work-bot"]),
            Some(json!({ "project": "KAN", "title": "member can create" })),
        )
        .await;
    assert_eq!(status, 201);
    let (status, _, _) = t
        .raw(
            "PUT",
            "/api/projects/KAN/members/999999",
            Some(&ids["work-bot"]),
            Some(json!({ "role": "viewer" })),
        )
        .await;
    assert_eq!(status, 403);
    let (status, _, _) = t
        .raw(
            "POST",
            "/api/projects/KAN/statuses",
            Some(&ids["work-bot"]),
            Some(json!({ "name": "Blocked", "category": "todo" })),
        )
        .await;
    assert_eq!(status, 403);
    let key = t1["key"].as_str().unwrap();
    let (status, _, _) = t.raw("DELETE", &format!("/api/tickets/{key}"), Some(&ids["work-bot"]), None).await;
    assert_eq!(status, 403, "deleting is project-admin only");

    // Project admin: can manage the workflow and delete, but is not a SITE admin.
    let (status, _, _) = t
        .raw(
            "POST",
            "/api/projects/KAN/statuses",
            Some(&ids["admin-bot"]),
            Some(json!({ "name": "Blocked", "category": "todo" })),
        )
        .await;
    assert_eq!(status, 201);
    let (status, _, _) = t.raw("DELETE", &format!("/api/tickets/{key}"), Some(&ids["admin-bot"]), None).await;
    assert_eq!(status, 204);
    let (status, _, err) = t
        .raw("POST", "/api/projects", Some(&ids["admin-bot"]), Some(json!({ "key": "NEW", "name": "New" })))
        .await;
    assert_eq!(status, 403, "{err}");
    let (status, _, err) =
        t.raw("PATCH", "/api/settings", Some(&ids["admin-bot"]), Some(json!({ "max_dag_height": 5 }))).await;
    assert_eq!(status, 403, "{err}");
}

#[tokio::test]
async fn tokens_can_be_read_only_scoped_and_revoked() {
    let t = TestApp::new().await;
    let admin = t.bootstrap_admin().await;
    for key in ["KAN", "SEC"] {
        t.raw("POST", "/api/projects", Some(&admin), Some(json!({ "key": key, "name": key }))).await;
    }

    // A token narrowed to KAN and marked read-only.
    let (_, _, created) = t
        .raw(
            "POST",
            "/api/tokens",
            Some(&admin),
            Some(json!({ "name": "ro", "projects": ["KAN"], "read_only": true })),
        )
        .await;
    let ro = created["token"].as_str().unwrap();
    assert!(created["projects"].as_array().unwrap().iter().any(|p| p == "KAN"));

    let (status, _, _) = t.raw("GET", "/api/tickets?project=KAN", Some(ro), None).await;
    assert_eq!(status, 200);
    let (status, _, err) = t
        .raw("POST", "/api/tickets", Some(ro), Some(json!({ "project": "KAN", "title": "should fail" })))
        .await;
    assert_eq!(status, 403, "{err}");
    let (status, _, _) = t.raw("GET", "/api/tickets?project=SEC", Some(ro), None).await;
    assert_eq!(status, 404, "narrowed away from SEC even though the owning admin can see it");

    // Revoke it; it stops working immediately.
    let id = created["id"].as_i64().unwrap();
    let (status, _, _) = t.raw("DELETE", &format!("/api/tokens/{id}"), Some(&admin), None).await;
    assert_eq!(status, 204);
    let (status, _, _) = t.raw("GET", "/api/tickets?project=KAN", Some(ro), None).await;
    assert_eq!(status, 401);

    // A token can't outrank its own owner: an ordinary member's token
    // can never carry site-admin power, even if it's left unnarrowed.
    let (_, _, u) = t
        .raw("POST", "/api/users", Some(&admin), Some(json!({ "username": "plain-agent", "kind": "agent" })))
        .await;
    let uid = u["id"].as_i64().unwrap();
    t.raw(
        "PUT",
        &format!("/api/projects/KAN/members/{uid}"),
        Some(&admin),
        Some(json!({ "role": "member" })),
    )
    .await;
    let (_, _, tok) =
        t.raw("POST", "/api/tokens", Some(&admin), Some(json!({ "name": "plain", "user_id": uid }))).await;
    let plain = tok["token"].as_str().unwrap();
    let (status, _, _) =
        t.raw("POST", "/api/projects", Some(plain), Some(json!({ "key": "NOPE", "name": "no" }))).await;
    assert_eq!(status, 403);

    // A non-admin cannot mint a token for someone else.
    let (status, _, _) =
        t.raw("POST", "/api/tokens", Some(plain), Some(json!({ "name": "x", "user_id": 1 }))).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn a_cross_project_cycle_error_never_names_a_ticket_you_cant_see() {
    let t = TestApp::new().await;
    let admin = t.bootstrap_admin().await;
    for key in ["KAN", "SEC"] {
        t.raw("POST", "/api/projects", Some(&admin), Some(json!({ "key": key, "name": key }))).await;
    }
    let (_, _, u) = t
        .raw("POST", "/api/users", Some(&admin), Some(json!({ "username": "coder", "kind": "agent" })))
        .await;
    let uid = u["id"].as_i64().unwrap();
    t.raw(
        "PUT",
        &format!("/api/projects/KAN/members/{uid}"),
        Some(&admin),
        Some(json!({ "role": "member" })),
    )
    .await;
    let (_, _, tok) = t
        .raw(
            "POST",
            "/api/tokens",
            Some(&admin),
            Some(json!({ "name": "coder", "user_id": uid, "projects": ["KAN"] })),
        )
        .await;
    let coder = tok["token"].as_str().unwrap();

    // As admin (who can see both projects): KAN-A -> SEC-X -> KAN-B. Both
    // endpoints of the link the coder agent is about to attempt (A and B)
    // are in KAN, visible to it; only the ticket that actually completes
    // the cycle, SEC-X, is hidden from it.
    async fn ticket(t: &TestApp, admin: &str, proj: &str, title: &str) -> String {
        let (_, _, tk) = t
            .raw("POST", "/api/tickets", Some(admin), Some(json!({ "project": proj, "title": title })))
            .await;
        tk["key"].as_str().unwrap().to_string()
    }
    let a = ticket(&t, &admin, "KAN", "A").await;
    let b = ticket(&t, &admin, "KAN", "B").await;
    let x = ticket(&t, &admin, "SEC", "secret blocker title").await;
    t.raw("POST", "/api/links", Some(&admin), Some(json!({ "source": a, "target": x, "kind": "blocks" })))
        .await;
    t.raw("POST", "/api/links", Some(&admin), Some(json!({ "source": x, "target": b, "kind": "blocks" })))
        .await;

    // The coder agent, who can only see KAN, tries to link B -> A (both
    // endpoints visible to it). Adding that edge closes the real cycle
    // A -> X -> B -> A, but only because of the hidden SEC-X hop. It must
    // be refused, and the refusal must not name SEC-X or its title.
    let (status, _, err) = t
        .raw("POST", "/api/links", Some(coder), Some(json!({ "source": b, "target": a, "kind": "blocks" })))
        .await;
    assert_eq!(status, 409, "{err}");
    assert_eq!(err["error"]["code"], "cycle");
    let message = err["error"]["message"].as_str().unwrap();
    assert!(!message.contains("SEC"), "{message}");
    assert!(!message.contains("secret blocker title"), "{message}");
    assert!(err["error"]["detail"].is_null(), "no chain detail at all when part of it is invisible: {err}");
}

#[tokio::test]
async fn the_last_site_admin_and_last_project_admin_cannot_be_removed() {
    let t = TestApp::new().await;
    let admin = t.bootstrap_admin().await;
    let (_, _, me) = t.raw("GET", "/api/auth/status", Some(&admin), None).await;
    let admin_id = me["user"]["id"].as_i64().unwrap();

    let (status, _, err) = t
        .raw("PATCH", &format!("/api/users/{admin_id}"), Some(&admin), Some(json!({ "is_admin": false })))
        .await;
    assert_eq!(status, 409, "{err}");
    assert_eq!(err["error"]["code"], "last_admin");

    t.raw("POST", "/api/projects", Some(&admin), Some(json!({ "key": "KAN", "name": "Kanban" }))).await;
    // The site admin auto-became the project's admin on creation.
    let (status, _, err) =
        t.raw("DELETE", &format!("/api/projects/KAN/members/{admin_id}"), Some(&admin), None).await;
    assert_eq!(status, 409, "{err}");
    assert_eq!(err["error"]["code"], "last_project_admin");
}

#[tokio::test]
async fn mcp_and_rest_agree_on_visibility() {
    let t = TestApp::new().await;
    let admin = t.bootstrap_admin().await;
    t.raw("POST", "/api/projects", Some(&admin), Some(json!({ "key": "KAN", "name": "Kanban" }))).await;

    let (status, _, mcp_reply) = t
        .raw(
            "POST",
            "/mcp",
            Some(&admin),
            Some(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "list_projects", "arguments": {} } })),
        )
        .await;
    assert_eq!(status, 200);
    assert!(!mcp_reply["result"]["isError"].as_bool().unwrap());
    let text = mcp_reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("KAN"));
}
