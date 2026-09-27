use axum::Router;
use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use kanban_server::mcp::bridge;
use kanban_server::{AppState, events::Events, open_db, router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Mcp {
    app: Router,
    _dir: tempfile::TempDir,
}

impl Mcp {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir.path().join("test.db")).await.unwrap();
        let app = router(AppState { db, events: Events::default() });
        let req = Request::post("/api/projects")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "key": "KAN", "name": "Kanban" }).to_string()))
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), 201);
        Mcp { app, _dir: dir }
    }

    async fn send(&self, message: Value) -> (u16, Value) {
        let req = Request::post("/mcp")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("x-kanban-user", "agent-coder")
            .body(Body::from(message.to_string()))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() })
    }

    /// Call a tool; returns (is_error, parsed text content).
    async fn tool(&self, name: &str, args: Value) -> (bool, Value) {
        let (status, reply) = self
            .send(json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": name, "arguments": args } }))
            .await;
        assert_eq!(status, 200, "{reply}");
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed = serde_json::from_str(text).unwrap_or_else(|_| json!(text));
        (result["isError"].as_bool().unwrap(), parsed)
    }
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

    let (_, init) = m
        .send(json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } }))
        .await;
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
    let (_, bad_tool) =
        m.send(json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": { "name": "drop_database" } })).await;
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
    assert_eq!(a["assignee"], "agent-coder", "the X-Kanban-User header created and attributed the user");
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
    assert_eq!(detail["comments"][0]["author"], "agent-coder");
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
async fn stdio_bridge_talks_to_a_running_board() {
    let dir = tempfile::tempdir().unwrap();
    let db = open_db(&dir.path().join("test.db")).await.unwrap();
    let app = router(AppState { db, events: Events::default() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let input = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "list_users", "arguments": {} } }),
    ]
    .iter()
    .map(|m| m.to_string() + "\n")
    .collect::<String>();

    let output = tokio::task::spawn_blocking(move || {
        let target = bridge::Target::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        let mut out = Vec::new();
        bridge::run(input.as_bytes(), &mut out, &target, Some("stdio-agent")).unwrap();
        String::from_utf8(out).unwrap()
    })
    .await
    .unwrap();

    let replies: Vec<Value> = output.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies.len(), 2, "one line per request, none for the notification");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26");
    let users = replies[1]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(users.contains("stdio-agent"), "--as attributed and created the user: {users}");
}
