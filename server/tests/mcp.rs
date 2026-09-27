use axum::Router;
use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use kanban_server::mcp::bridge;
use kanban_server::{AppState, open_db, router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Mcp {
    app: Router,
    token: String,
    _dir: tempfile::TempDir,
}

/// A response's `Set-Cookie` value, stripped of the attributes, ready to
/// send back as `Cookie: name=value`.
fn cookie_from(res: &axum::http::Response<Body>) -> String {
    let raw = res.headers()["set-cookie"].to_str().unwrap();
    raw.split(';').next().unwrap().to_string()
}

impl Mcp {
    /// Bootstraps a real administrator through `/api/auth/setup` (the same
    /// one-time-code flow a person uses), then mints a real API token from
    /// that session — proving the whole chain (setup -> session -> token ->
    /// `/mcp` bearer auth) works, not just the tool logic behind it.
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir.path().join("test.db")).await.unwrap();
        let state = AppState::new(db);
        *state.setup_code.lock().unwrap() = Some("TEST-CODE".into());
        let app = router(state);

        let req = Request::post("/api/auth/setup")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "code": "TEST-CODE", "username": "admin", "password": "hunter2hunter2" }).to_string(),
            ))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), 200, "setup should succeed");
        let cookie = cookie_from(&res);

        let req = Request::post("/api/projects")
            .header("content-type", "application/json")
            .header("cookie", &cookie)
            .body(Body::from(json!({ "key": "KAN", "name": "Kanban" }).to_string()))
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), 201);

        let req = Request::post("/api/tokens")
            .header("content-type", "application/json")
            .header("cookie", &cookie)
            .body(Body::from(json!({ "name": "mcp test token" }).to_string()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), 201);
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let token = created["token"].as_str().unwrap().to_string();
        assert!(token.starts_with("kbn_"));

        Mcp { app, token, _dir: dir }
    }

    async fn send(&self, message: Value) -> (u16, Value) {
        let req = Request::post("/mcp")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("authorization", format!("Bearer {}", self.token))
            .body(Body::from(message.to_string()))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() })
    }

    /// Call a tool; returns (is_error, parsed text content).
    async fn tool(&self, name: &str, args: Value) -> (bool, Value) {
        let (status, reply) =
            self.send(json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": name, "arguments": args } })).await;
        assert_eq!(status, 200, "{reply}");
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed = serde_json::from_str(text).unwrap_or_else(|_| json!(text));
        (result["isError"].as_bool().unwrap(), parsed)
    }
}

#[tokio::test]
async fn mcp_requires_a_bearer_token() {
    let dir = tempfile::tempdir().unwrap();
    let db = open_db(&dir.path().join("test.db")).await.unwrap();
    let app = router(AppState::new(db));

    let req = Request::post("/mcp")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string()))
        .unwrap();
    assert_eq!(app.clone().oneshot(req).await.unwrap().status(), 401, "no token at all");

    let req = Request::post("/mcp")
        .header("content-type", "application/json")
        .header("authorization", "Bearer kbn_not_a_real_token_at_all_00000000000000000")
        .body(Body::from(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string()))
        .unwrap();
    assert_eq!(app.clone().oneshot(req).await.unwrap().status(), 401, "a well-formed but unknown token");
}

#[tokio::test]
async fn protocol_handshake_and_errors() {
    let m = Mcp::new().await;
    let (status, init) = m
        .send(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } } }))
        .await;
    assert_eq!(status, 200);
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "kanban");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    let (_, init) = m.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } })).await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "unknown versions get one we support");

    let (status, body) = m.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
    assert_eq!((status, body), (202, Value::Null));

    let (_, list) = m.send(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" })).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "list_projects",
        "get_project",
        "list_users",
        "search_tickets",
        "list_ready_tickets",
        "get_ticket",
        "create_ticket",
        "update_ticket",
        "add_comment",
        "link_tickets",
        "unlink_tickets",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
    assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object" && t["description"].is_string()));

    let (_, ping) = m.send(json!({ "jsonrpc": "2.0", "id": 4, "method": "ping" })).await;
    assert_eq!(ping["result"], json!({}));
    let (_, unknown) = m.send(json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/list" })).await;
    assert_eq!(unknown["error"]["code"], -32601);
    let (_, bad_tool) = m.send(json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": { "name": "drop_database" } })).await;
    assert_eq!(bad_tool["error"]["code"], -32602);
}

#[tokio::test]
async fn an_agent_can_run_a_whole_workflow() {
    let m = Mcp::new().await;

    let (err, a) = m
        .tool(
            "create_ticket",
            json!({
                "project": "KAN", "title": "Write the parser", "status": "in progress",
                "assignee": "me", "labels": ["backend", "parser"], "priority": "high"
            }),
        )
        .await;
    assert!(!err, "{a}");
    assert_eq!((a["key"].as_str(), a["status"].as_str()), (Some("KAN-1"), Some("In Progress")));
    assert_eq!(a["assignee"], "admin", "the bearer token attributed the change to its owning account");
    assert_eq!(a["labels"], json!(["backend", "parser"]), "missing labels were created");

    let (_, b) = m.tool("create_ticket", json!({ "project": "KAN", "title": "Test the parser" })).await;
    assert_eq!(b["key"], "KAN-2");

    let (err, linked) = m.tool("link_tickets", json!({ "source": "KAN-1", "target": "KAN-2" })).await;
    assert!(!err);
    assert!(linked["result"].as_str().unwrap().contains("KAN-2 waits for KAN-1"));
    let (err, cycle) = m.tool("link_tickets", json!({ "source": "KAN-2", "target": "KAN-1" })).await;
    assert!(err, "cycles are refused");
    assert!(cycle.as_str().unwrap().contains("cycle"), "{cycle}");

    let (_, ready) = m.tool("list_ready_tickets", json!({ "project": "KAN" })).await;
    let keys: Vec<&str> =
        ready["tickets"].as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert_eq!(keys, vec!["KAN-1"], "KAN-2 waits on KAN-1");

    let (err, msg) = m.tool("update_ticket", json!({ "key": "KAN-1", "status": "Shipped" })).await;
    assert!(err);
    assert!(msg.as_str().unwrap().contains("Valid: To Do, In Progress, In Review, Done"), "{msg}");

    let (err, done) = m
        .tool("update_ticket", json!({ "key": "KAN-1", "status": "Done", "remove_labels": ["parser"] }))
        .await;
    assert!(!err, "{done}");
    assert_eq!((done["done"].as_bool(), done["labels"].clone()), (Some(true), json!(["backend"])));
    let (_, ready) = m.tool("list_ready_tickets", json!({ "project": "KAN" })).await;
    assert_eq!(ready["tickets"][0]["key"], "KAN-2", "unblocked once its blocker is done");

    let (err, _) = m
        .tool("add_comment", json!({ "key": "KAN-2", "body": "Starting on this now that KAN-1 is done." }))
        .await;
    assert!(!err);
    let (_, detail) = m.tool("get_ticket", json!({ "key": "KAN-2", "include_history": true })).await;
    assert_eq!(detail["comments"][0]["author"], "admin");
    assert_eq!(detail["links"][0]["relation"], "is blocked by");
    assert_eq!(detail["links"][0]["done"], true);
    assert!(detail["history"].as_array().unwrap().len() >= 2);
    assert!(detail["created_at"].as_str().unwrap().ends_with('Z'));

    let (_, found) = m.tool("search_tickets", json!({ "query": "parser", "status": "done" })).await;
    assert_eq!(found["total"], 1);
    let (_, unassigned) = m.tool("search_tickets", json!({ "project": "KAN", "assignee": "none" })).await;
    assert_eq!(unassigned["tickets"][0]["key"], "KAN-2");

    let (err, cleared) = m.tool("update_ticket", json!({ "key": "KAN-1", "assignee": "none" })).await;
    assert!(!err);
    assert_eq!(cleared["assignee"], Value::Null, "\"none\" unassigns");

    let (err, _) = m.tool("unlink_tickets", json!({ "source": "KAN-1", "target": "KAN-2" })).await;
    assert!(!err);
    let (_, projects) = m.tool("list_projects", json!({})).await;
    assert_eq!(projects[0]["statuses"][0], "To Do");
    let (err, missing) = m.tool("get_ticket", json!({ "key": "KAN-99" })).await;
    assert!(err && missing.as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn due_dates_round_trip_as_plain_calendar_dates() {
    let m = Mcp::new().await;
    let (err, created) = m
        .tool("create_ticket", json!({ "project": "KAN", "title": "Ship it", "due_date": "2026-12-31" }))
        .await;
    assert!(!err, "{created}");
    assert_eq!(created["due_date"], "2026-12-31");

    let (err, bad) = m
        .tool("create_ticket", json!({ "project": "KAN", "title": "Bad date", "due_date": "31/12/2026" }))
        .await;
    assert!(err, "should reject a non-ISO date");
    assert!(bad.as_str().unwrap().contains("YYYY-MM-DD"), "{bad}");

    let (err, overdue) = m.tool("search_tickets", json!({ "project": "KAN", "overdue": true })).await;
    assert!(!err);
    assert_eq!(overdue["total"], 0, "a 2026-12-31 due date from this test's run isn't overdue yet");

    let (err, cleared) = m.tool("update_ticket", json!({ "key": "KAN-1", "due_date": null })).await;
    assert!(!err, "{cleared}");
    assert_eq!(cleared["due_date"], Value::Null);
}

/// The core promise this session's auth work is for: a token narrowed to
/// one project can use MCP to see and work that project, but a project it
/// isn't a member of simply doesn't exist to it — not a 403, a 404, exactly
/// like the REST API and the UI.
#[tokio::test]
async fn a_narrowed_agent_token_only_sees_its_own_project() {
    let m = Mcp::new().await;
    let cookie_req = Request::post("/api/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "username": "admin", "password": "hunter2hunter2" }).to_string()))
        .unwrap();
    let res = m.app.clone().oneshot(cookie_req).await.unwrap();
    assert_eq!(res.status(), 200);
    let cookie = cookie_from(&res);

    // A second project the agent is never added to.
    let req = Request::post("/api/projects")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(json!({ "key": "SEC", "name": "Secret project" }).to_string()))
        .unwrap();
    assert_eq!(m.app.clone().oneshot(req).await.unwrap().status(), 201);
    let req = Request::post("/api/tickets")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(
            json!({ "project": "SEC", "title": "Something the coder agent must never see" }).to_string(),
        ))
        .unwrap();
    assert_eq!(m.app.clone().oneshot(req).await.unwrap().status(), 201);

    // The agent account: a member of KAN only, with its own narrowed token.
    let req = Request::post("/api/users")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(json!({ "username": "coder-agent", "kind": "agent" }).to_string()))
        .unwrap();
    let res = m.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), 201);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let agent: Value = serde_json::from_slice(&bytes).unwrap();
    let agent_id = agent["id"].as_i64().unwrap();

    let req = Request::put(format!("/api/projects/KAN/members/{agent_id}"))
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(json!({ "role": "member" }).to_string()))
        .unwrap();
    assert_eq!(m.app.clone().oneshot(req).await.unwrap().status(), 200);

    let req = Request::post("/api/tokens")
        .header("content-type", "application/json")
        .header("cookie", &cookie)
        .body(Body::from(
            json!({ "name": "coder agent token", "user_id": agent_id, "projects": ["KAN"] }).to_string(),
        ))
        .unwrap();
    let res = m.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), 201, "a site admin may mint a token for an agent it manages");
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let created: Value = serde_json::from_slice(&bytes).unwrap();
    let agent_token = created["token"].as_str().unwrap().to_string();

    let send = |token: String, message: Value| {
        let app = m.app.clone();
        async move {
            let req = Request::post("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(message.to_string()))
                .unwrap();
            let res = app.oneshot(req).await.unwrap();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        }
    };
    let call = |token: String, tool: &'static str, args: Value| {
        let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": tool, "arguments": args } });
        send(token, msg)
    };

    let reply = call(agent_token.clone(), "list_projects", json!({})).await;
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("KAN"), "{text}");
    assert!(!text.contains("SEC"), "the agent's narrowed token must never see the other project: {text}");

    let reply = call(agent_token.clone(), "search_tickets", json!({})).await;
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains("must never see"), "cross-project search leaked a title: {text}");

    let reply = call(agent_token.clone(), "get_ticket", json!({ "key": "SEC-1" })).await;
    assert_eq!(reply["result"]["isError"], true);
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.to_lowercase().contains("not found"),
        "a ticket in an invisible project reads as not found, not forbidden: {text}"
    );

    let reply =
        call(agent_token, "create_ticket", json!({ "project": "SEC", "title": "should be refused" })).await;
    assert_eq!(reply["result"]["isError"], true);
}

#[tokio::test]
async fn stdio_bridge_talks_to_a_running_board() {
    let m = Mcp::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = m.app.clone();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let input = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "list_users", "arguments": {} } }),
    ]
    .iter()
    .map(|m| m.to_string() + "\n")
    .collect::<String>();
    let token = m.token.clone();

    let output = tokio::task::spawn_blocking(move || {
        let target = bridge::Target::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        let mut out = Vec::new();
        bridge::run(input.as_bytes(), &mut out, &target, &token).unwrap();
        String::from_utf8(out).unwrap()
    })
    .await
    .unwrap();

    let replies: Vec<Value> = output.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies.len(), 2, "one line per request, none for the notification");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26");
    let users = replies[1]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(users.contains("admin"), "the bridge authenticated as the token's real owner: {users}");
}

#[tokio::test]
async fn stdio_bridge_reports_a_bad_token_without_hanging() {
    let m = Mcp::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = m.app.clone();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let input = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string() + "\n";
    let output = tokio::task::spawn_blocking(move || {
        let target = bridge::Target::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        let mut out = Vec::new();
        bridge::run(input.as_bytes(), &mut out, &target, "kbn_totally_wrong").unwrap();
        String::from_utf8(out).unwrap()
    })
    .await
    .unwrap();
    assert!(output.contains("rejected this API token"), "{output}");
}
