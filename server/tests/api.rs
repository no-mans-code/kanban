use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request};
use http_body_util::BodyExt;
use kanban_server::auth::Principal;
use kanban_server::{AppState, open_db, router};
use serde_json::{Value, json};
use tower::ServiceExt;

struct TestApp {
    app: Router,
    _dir: tempfile::TempDir,
}

impl TestApp {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir.path().join("test.db")).await.unwrap();
        let app = router(AppState::new(db));
        TestApp { app, _dir: dir }
    }

    /// These tests exercise ticket/project business logic, not the auth
    /// layer itself (that's tests/auth.rs, over real bearer tokens and
    /// cookies). So every request here runs as a full-access principal
    /// attached directly — the same mechanism `inproc::call` uses for
    /// in-process callers — acting as whichever user "setup()" creates
    /// first (id 1, "me"), matching how these tests were written.
    async fn req(&self, method: Method, uri: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body.map(|b| Body::from(b.to_string())).unwrap_or_default())
            .unwrap();
        req.extensions_mut().insert(Principal::acting_as(1));
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() })
    }

    async fn ok(&self, method: Method, uri: &str, body: Value) -> Value {
        let (status, value) = self.req(method.clone(), uri, Some(body)).await;
        assert!((200..300).contains(&status), "{method} {uri} -> {status}: {value}");
        value
    }

    async fn get(&self, uri: &str) -> Value {
        let (status, value) = self.req(Method::GET, uri, None).await;
        assert_eq!(status, 200, "GET {uri}: {value}");
        value
    }

    /// A user (id 1, the actor above) and project KAN with the default statuses.
    async fn setup(&self) -> Value {
        self.ok(Method::POST, "/api/users", json!({ "username": "me" })).await;
        self.ok(Method::POST, "/api/projects", json!({ "key": "KAN", "name": "Kanban" })).await
    }

    async fn ticket(&self, title: &str, extra: Value) -> String {
        let mut body = json!({ "project": "KAN", "title": title });
        body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let t = self.ok(Method::POST, "/api/tickets", body).await;
        t["key"].as_str().unwrap().to_string()
    }

    async fn link(&self, source: &str, target: &str) -> (u16, Value) {
        self.req(
            Method::POST,
            "/api/links",
            Some(json!({ "source": source, "target": target, "kind": "blocks" })),
        )
        .await
    }
}

fn status_id(project: &Value, name: &str) -> i64 {
    project["statuses"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap()["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn dag_rejects_cycles_and_enforces_the_height_limit() {
    let t = TestApp::new().await;
    t.setup().await;
    let a = t.ticket("A", json!({})).await;
    let b = t.ticket("B", json!({})).await;
    let c = t.ticket("C", json!({})).await;
    let d = t.ticket("D", json!({})).await;

    assert_eq!(t.link(&a, &b).await.0, 201);
    assert_eq!(t.link(&b, &c).await.0, 201);
    assert_eq!(t.link(&a, &b).await.0, 409, "duplicate link");

    let (status, err) = t.link(&c, &a).await;
    assert_eq!(status, 409);
    assert_eq!(err["error"]["code"], "cycle");
    assert_eq!(err["error"]["detail"]["chain"], json!([c, a, b, c]));

    // The current longest chain is A -> B -> C (3 tickets).
    let (status, err) = t.req(Method::PATCH, "/api/settings", Some(json!({ "max_dag_height": 2 }))).await;
    assert_eq!(status, 409, "cannot lower the limit below the existing height");
    assert_eq!(err["error"]["detail"]["chain"], json!([a, b, c]));

    t.ok(Method::PATCH, "/api/settings", json!({ "max_dag_height": 3 })).await;
    let (status, err) = t.link(&c, &d).await;
    assert_eq!(status, 409);
    assert_eq!(err["error"]["code"], "height_exceeded");
    assert_eq!(err["error"]["detail"]["chain"], json!([a, b, c, d]));
    // A branch that stays within the limit is still allowed.
    assert_eq!(t.link(&a, &d).await.0, 201);

    t.ok(Method::PATCH, "/api/settings", json!({ "max_dag_height": null })).await;
    assert_eq!(t.link(&c, &d).await.0, 201);
    let settings = t.get("/api/settings").await;
    assert_eq!(settings["max_dag_height"], Value::Null);
    assert_eq!(settings["dag_height"], 4);

    // D is blocked until its blockers are done.
    let detail = t.get(&format!("/api/tickets/{d}")).await;
    assert_eq!(detail["is_blocked"], true);
    assert_eq!(detail["links"].as_array().unwrap().len(), 2);

    let graph = t.get("/api/graph?project=KAN").await;
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 4);
    assert_eq!(graph["edges"].as_array().unwrap().len(), 4);
    assert_eq!(graph["longest_chain"].as_array().unwrap().len(), 4);

    let uri = format!("/api/links?source={c}&target={d}&kind=blocks");
    assert_eq!(t.req(Method::DELETE, &uri, None).await.0, 204);
    assert_eq!(t.get("/api/settings").await["dag_height"], 3);
}

#[tokio::test]
async fn hierarchy_follows_jira_rules() {
    let t = TestApp::new().await;
    t.setup().await;
    let (status, _) = t
        .req(Method::POST, "/api/tickets", Some(json!({ "project": "KAN", "title": "x", "type": "subtask" })))
        .await;
    assert_eq!(status, 400, "a subtask needs a parent");

    let epic = t.ticket("Epic", json!({ "type": "epic" })).await;
    let story = t.ticket("Story", json!({ "type": "story", "parent": epic })).await;
    let sub = t.ticket("Sub", json!({ "type": "subtask", "parent": story })).await;

    let bad = [
        json!({ "project": "KAN", "title": "x", "type": "subtask", "parent": epic }),
        json!({ "project": "KAN", "title": "x", "type": "epic", "parent": epic }),
        json!({ "project": "KAN", "title": "x", "type": "story", "parent": sub }),
    ];
    for body in bad {
        assert_eq!(t.req(Method::POST, "/api/tickets", Some(body.clone())).await.0, 400, "{body}");
    }

    let (status, err) =
        t.req(Method::PATCH, &format!("/api/tickets/{story}"), Some(json!({ "type": "subtask" }))).await;
    assert_eq!(status, 400, "{err}");

    let detail = t.get(&format!("/api/tickets/{story}")).await;
    assert_eq!(detail["parent_key"], epic);
    assert_eq!(detail["children"][0]["key"], sub);
    assert_eq!(t.get(&format!("/api/tickets/{epic}")).await["child_count"], 1);

    // Detach, then deleting the story takes its subtask with it.
    t.ok(Method::PATCH, &format!("/api/tickets/{story}"), json!({ "parent": null })).await;
    assert_eq!(t.get(&format!("/api/tickets/{story}")).await["parent_key"], Value::Null);
    assert_eq!(t.req(Method::DELETE, &format!("/api/tickets/{story}?children=delete"), None).await.0, 204);
    assert_eq!(t.req(Method::GET, &format!("/api/tickets/{sub}"), None).await.0, 404);
}

#[tokio::test]
async fn moves_reorder_columns_and_track_resolution() {
    let t = TestApp::new().await;
    let project = t.setup().await;
    let todo = status_id(&project, "To Do");
    let done = status_id(&project, "Done");
    let first = t.ticket("first", json!({})).await;
    let last = t.ticket("last", json!({})).await;

    // Insert 40 tickets, each directly after `first`. Every insert halves the
    // gap, so the column has to be renumbered partway through.
    let mut inserted = Vec::new();
    for i in 0..40 {
        let k = t.ticket(&format!("n{i}"), json!({})).await;
        t.ok(Method::POST, &format!("/api/tickets/{k}/move"), json!({ "status_id": todo, "after": first }))
            .await;
        inserted.push(k);
    }
    let order: Vec<String> = t
        .get("/api/tickets?project=KAN")
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["key"].as_str().unwrap().to_string())
        .collect();
    let mut expected = vec![first.clone()];
    expected.extend(inserted.iter().rev().cloned());
    expected.push(last.clone());
    assert_eq!(order, expected);

    let moved = t.ok(Method::POST, &format!("/api/tickets/{last}/move"), json!({ "status_id": done })).await;
    assert_eq!(moved["status_category"], "done");
    assert!(moved["resolved_at"].is_i64());
    let back = t.ok(Method::POST, &format!("/api/tickets/{last}/move"), json!({ "status_id": todo })).await;
    assert_eq!(back["resolved_at"], Value::Null);

    let (status, _) = t
        .req(
            Method::POST,
            &format!("/api/tickets/{last}/move"),
            Some(json!({ "status_id": done, "after": first })),
        )
        .await;
    assert_eq!(status, 400, "`after` must be in the target column");
}

#[tokio::test]
async fn search_covers_titles_descriptions_comments_and_keys() {
    let t = TestApp::new().await;
    t.setup().await;
    let a = t.ticket("Fix the login redirect", json!({ "description": "Happens after OAuth" })).await;
    let b = t.ticket("Unrelated", json!({})).await;
    t.ok(
        Method::POST,
        &format!("/api/tickets/{b}/comments"),
        json!({ "body": "Seen with a zebra-striped banner" }),
    )
    .await;

    let keys = |v: Value| -> Vec<String> {
        v.as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(keys(t.get("/api/tickets?q=logi").await), vec![a.clone()]);
    assert_eq!(keys(t.get("/api/tickets?q=oauth").await), vec![a.clone()]);
    assert_eq!(keys(t.get("/api/tickets?q=zebra").await), vec![b.clone()]);
    assert_eq!(keys(t.get(&format!("/api/tickets?q={}", b.to_lowercase())).await), vec![b.clone()]);
    // FTS5 syntax in user input must not break the query.
    assert_eq!(t.req(Method::GET, "/api/tickets?q=%22)%20OR%20NEAR(", None).await.0, 200);
}

#[tokio::test]
async fn mentioning_a_ticket_key_in_a_comment_links_it() {
    let t = TestApp::new().await;
    t.setup().await;
    let a = t.ticket("A", json!({})).await;
    let b = t.ticket("B", json!({})).await;
    let c = t.ticket("C", json!({})).await;

    t.ok(
        Method::POST,
        &format!("/api/tickets/{a}/comments"),
        json!({ "body": format!("Related to {b} and also, again, {b}.") }),
    )
    .await;

    let da = t.get(&format!("/api/tickets/{a}")).await;
    let db = t.get(&format!("/api/tickets/{b}")).await;
    let dc = t.get(&format!("/api/tickets/{c}")).await;
    assert_eq!(da["links"].as_array().unwrap().len(), 1, "one relates link, not one per mention");
    assert_eq!(da["links"][0]["ticket"]["key"], json!(b));
    assert_eq!(da["links"][0]["kind"], json!("relates"));
    assert_eq!(db["links"][0]["ticket"]["key"], json!(a));
    assert!(dc["links"].as_array().unwrap().is_empty(), "C was never mentioned");

    // Mentioning yourself, or a ticket already linked, is a no-op, not an error.
    t.ok(
        Method::POST,
        &format!("/api/tickets/{a}/comments"),
        json!({ "body": format!("{a} again mentions {b}.") }),
    )
    .await;
    let da = t.get(&format!("/api/tickets/{a}")).await;
    assert_eq!(da["links"].as_array().unwrap().len(), 1, "no duplicate or self link created");
}

#[tokio::test]
async fn edits_are_recorded_in_history() {
    let t = TestApp::new().await;
    let project = t.setup().await;
    let bob = t.ok(Method::POST, "/api/users", json!({ "username": "bob", "display_name": "Bob" })).await;
    let bug = t.ok(Method::POST, "/api/labels", json!({ "name": "bug" })).await;
    let key = t.ticket("Original", json!({})).await;

    let updated = t
        .ok(
            Method::PATCH,
            &format!("/api/tickets/{key}"),
            json!({
                "title": "Renamed", "assignee_id": bob["id"], "label_ids": [bug["id"]],
                "status_id": status_id(&project, "In Progress"), "priority": "high"
            }),
        )
        .await;
    assert_eq!(updated["title"], "Renamed");
    assert_eq!(updated["label_ids"], json!([bug["id"]]));
    assert_eq!(updated["watcher_ids"], json!([1, bob["id"]]), "reporter and assignee watch");

    let history = t.get(&format!("/api/tickets/{key}/activity")).await;
    let fields: Vec<&str> = history.as_array().unwrap().iter().filter_map(|a| a["field"].as_str()).collect();
    for f in ["title", "assignee", "labels", "status", "priority"] {
        assert!(fields.contains(&f), "missing {f} in {fields:?}");
    }
    let assignee = history.as_array().unwrap().iter().find(|a| a["field"] == "assignee").unwrap();
    assert_eq!(assignee["old_value"], "Unassigned");
    assert_eq!(assignee["new_value"], "Bob");

    // A no-op patch adds nothing.
    t.ok(Method::PATCH, &format!("/api/tickets/{key}"), json!({ "title": "Renamed" })).await;
    let again = t.get(&format!("/api/tickets/{key}/activity")).await;
    assert_eq!(again.as_array().unwrap().len(), history.as_array().unwrap().len());
}

#[tokio::test]
async fn due_dates_are_settable_sortable_and_filter_overdue() {
    let t = TestApp::new().await;
    t.setup().await;
    let past = t.ticket("Past due", json!({ "due_date": 0 })).await; // 1970-01-01
    let future = t.ticket("Not due yet", json!({ "due_date": 4_102_444_800_000_i64 })).await; // 2100-01-01
    let none = t.ticket("No due date", json!({})).await;

    let detail = t.get(&format!("/api/tickets/{past}")).await;
    assert_eq!(detail["due_date"], 0);
    let detail = t.get(&format!("/api/tickets/{none}")).await;
    assert_eq!(detail["due_date"], Value::Null);

    // Sorting puts due dates in order with nulls last.
    let sorted = t.get("/api/tickets?sort=due").await;
    let keys: Vec<&str> = sorted.as_array().unwrap().iter().map(|x| x["key"].as_str().unwrap()).collect();
    assert_eq!(keys, [past.as_str(), future.as_str(), none.as_str()]);

    // Only the not-done, past-due ticket is "overdue".
    let overdue = t.get("/api/tickets?overdue=true").await;
    let keys: Vec<&str> = overdue.as_array().unwrap().iter().map(|x| x["key"].as_str().unwrap()).collect();
    assert_eq!(keys, [past.as_str()]);

    // The activity log records the change with a readable date, not raw ms.
    t.ok(Method::PATCH, &format!("/api/tickets/{future}"), json!({ "due_date": 0 })).await;
    let history = t.get(&format!("/api/tickets/{future}/activity")).await;
    let due = history.as_array().unwrap().iter().find(|a| a["field"] == "due_date").unwrap();
    assert_eq!(due["old_value"], "2100-01-01");
    assert_eq!(due["new_value"], "1970-01-01");

    // Clearing it back to null works, and is distinct from "not provided".
    t.ok(Method::PATCH, &format!("/api/tickets/{future}"), json!({ "due_date": null })).await;
    let detail = t.get(&format!("/api/tickets/{future}")).await;
    assert_eq!(detail["due_date"], Value::Null);
}

#[tokio::test]
async fn wip_limits_are_settable_validated_and_clearable() {
    let t = TestApp::new().await;
    let project = t.setup().await;
    let todo = status_id(&project, "To Do");

    let statuses = t.ok(Method::PATCH, &format!("/api/statuses/{todo}"), json!({ "wip_limit": 3 })).await;
    let s = statuses.as_array().unwrap().iter().find(|s| s["id"] == todo).unwrap();
    assert_eq!(s["wip_limit"], 3);

    // It's advisory, not enforced: tickets past the limit still move in fine.
    for i in 0..5 {
        t.ticket(&format!("t{i}"), json!({ "status_id": todo })).await;
    }
    let over = t.get(&format!("/api/projects/{}", project["key"].as_str().unwrap())).await;
    let s = over["statuses"].as_array().unwrap().iter().find(|s| s["id"] == todo).unwrap();
    assert_eq!(s["wip_limit"], 3, "still reports the configured limit, not enforcing it");

    let (status, body) =
        t.req(Method::PATCH, &format!("/api/statuses/{todo}"), Some(json!({ "wip_limit": 0 }))).await;
    assert_eq!(status, 400, "{body}");

    let cleared = t.ok(Method::PATCH, &format!("/api/statuses/{todo}"), json!({ "wip_limit": null })).await;
    let s = cleared.as_array().unwrap().iter().find(|s| s["id"] == todo).unwrap();
    assert_eq!(s["wip_limit"], Value::Null);
}

#[tokio::test]
async fn deleting_a_status_in_use_needs_a_destination() {
    let t = TestApp::new().await;
    let project = t.setup().await;
    let review = status_id(&project, "In Review");
    let todo = status_id(&project, "To Do");
    let key = t.ticket("x", json!({ "status_id": review })).await;

    let (status, err) = t.req(Method::DELETE, &format!("/api/statuses/{review}"), None).await;
    assert_eq!(status, 409);
    assert_eq!(err["error"]["code"], "status_in_use");

    let statuses = t.ok(Method::DELETE, &format!("/api/statuses/{review}?move_to={todo}"), json!(null)).await;
    assert_eq!(statuses.as_array().unwrap().len(), 3);
    assert_eq!(t.get(&format!("/api/tickets/{key}")).await["status_id"], todo);
}

#[tokio::test]
async fn validation_errors_are_json() {
    let t = TestApp::new().await;
    t.setup().await;
    let (status, err) = t.req(Method::POST, "/api/projects", Some(json!({ "key": "k", "name": "x" }))).await;
    assert_eq!(status, 400);
    assert_eq!(err["error"]["code"], "invalid");
    let (status, _) = t.req(Method::GET, "/api/tickets/NOPE-1", None).await;
    assert_eq!(status, 404);
    let (status, _) = t.req(Method::GET, "/api/does-not-exist", None).await;
    assert_eq!(status, 404);
}

/// The host guard runs before authentication (see `router()`), so this
/// attaches a valid, full-access principal directly (bypassing the header
/// auth this test isn't about) to prove the guard rejects a bad Host/Origin
/// even for an otherwise fully authorized request.
async fn with_headers(t: &TestApp, method: Method, uri: &str, headers: &[(&str, &str)]) -> u16 {
    let mut req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let body = Body::from(json!({ "username": "probe" }).to_string());
    let mut req = req.body(body).unwrap();
    req.extensions_mut().insert(Principal::acting_as(1));
    t.app.clone().oneshot(req).await.unwrap().status().as_u16()
}

#[tokio::test]
async fn foreign_hosts_and_origins_are_rejected() {
    let t = TestApp::new().await;
    // DNS rebinding: a hostile name that resolves to 127.0.0.1.
    assert_eq!(with_headers(&t, Method::GET, "/api/users", &[("host", "attacker.example:8610")]).await, 403);
    assert_eq!(with_headers(&t, Method::GET, "/", &[("host", "localhost.attacker.example")]).await, 403);
    // A cross-site write.
    let origin = [("host", "127.0.0.1:8610"), ("origin", "https://attacker.example")];
    assert_eq!(with_headers(&t, Method::POST, "/api/users", &origin).await, 403);
    assert_eq!(with_headers(&t, Method::POST, "/api/users", &[("origin", "null")]).await, 403);
    assert_eq!(t.get("/api/users").await.as_array().unwrap().len(), 0, "nothing was created");

    // The UI itself, the Vite dev proxy and IPv6 loopback still work.
    let same_origin = [("host", "127.0.0.1:8610"), ("origin", "http://127.0.0.1:8610")];
    assert_eq!(with_headers(&t, Method::POST, "/api/users", &same_origin).await, 201);
    assert_eq!(with_headers(&t, Method::GET, "/api/users", &[("host", "localhost:5173")]).await, 200);
    assert_eq!(with_headers(&t, Method::GET, "/api/health", &[("host", "[::1]:8610")]).await, 200);
}

async fn page(t: &TestApp, uri: &str) -> (Vec<String>, i64) {
    let mut req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    req.extensions_mut().insert(Principal::acting_as(1));
    let res = t.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), 200);
    let total = res.headers()["x-total-count"].to_str().unwrap().parse().unwrap();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let rows: Value = serde_json::from_slice(&bytes).unwrap();
    (rows.as_array().unwrap().iter().map(|r| r["key"].as_str().unwrap().to_string()).collect(), total)
}

#[tokio::test]
async fn listing_pages_through_every_ticket() {
    let t = TestApp::new().await;
    t.setup().await;
    for i in 0..7 {
        let priority = if i % 2 == 0 { "high" } else { "low" };
        t.ticket(&format!("t{i}"), json!({ "priority": priority })).await;
    }
    let mut seen = Vec::new();
    for offset in [0, 3, 6] {
        let (keys, total) =
            page(&t, &format!("/api/tickets?project=KAN&sort=key&limit=3&offset={offset}")).await;
        assert_eq!(total, 7);
        seen.extend(keys);
    }
    let expected: Vec<String> = (1..=7).map(|n| format!("KAN-{n}")).collect();
    assert_eq!(seen, expected, "no ticket skipped or repeated across pages");

    let (keys, total) = page(&t, "/api/tickets?project=KAN&priority=high&limit=2").await;
    assert_eq!((keys.len(), total), (2, 4), "the total respects filters, not the page size");
    let (keys, total) = page(&t, "/api/tickets?project=KAN&offset=50").await;
    assert_eq!((keys.len(), total), (0, 7));
}

async fn last_activity(t: &TestApp, key: &str) -> Value {
    let history = t.get(&format!("/api/tickets/{key}/activity")).await;
    history.as_array().unwrap().last().unwrap().clone()
}

#[tokio::test]
async fn deletion_is_recorded_on_every_ticket_it_touches() {
    let t = TestApp::new().await;
    t.setup().await;
    let epic = t.ticket("Epic", json!({ "type": "epic" })).await;
    let story = t.ticket("Story", json!({ "type": "story", "parent": epic })).await;
    let sub = t.ticket("Sub", json!({ "type": "subtask", "parent": story })).await;
    let blocker = t.ticket("Blocker", json!({})).await;
    let blocked = t.ticket("Blocked", json!({})).await;
    assert_eq!(t.link(&blocker, &blocked).await.0, 201);
    assert_eq!(t.get(&format!("/api/tickets/{blocked}")).await["is_blocked"], true);

    assert_eq!(t.req(Method::DELETE, &format!("/api/tickets/{epic}?children=detach"), None).await.0, 204);
    let last = last_activity(&t, &story).await;
    assert_eq!((last["field"].as_str(), last["old_value"].as_str()), (Some("parent"), Some(epic.as_str())));
    assert_eq!(last["new_value"], format!("None ({epic} deleted)"));

    assert_eq!(t.req(Method::DELETE, &format!("/api/tickets/{blocker}?dependents=drop"), None).await.0, 204);
    let last = last_activity(&t, &blocked).await;
    assert_eq!(last["action"], "unlinked");
    assert_eq!(last["field"], "is blocked by");
    assert_eq!(last["old_value"], format!("{blocker} (deleted)"));
    assert_eq!(t.get(&format!("/api/tickets/{blocked}")).await["is_blocked"], false);

    assert_eq!(t.req(Method::DELETE, &format!("/api/tickets/{sub}"), None).await.0, 204);
    let last = last_activity(&t, &story).await;
    assert_eq!(last["action"], "child_deleted");
    assert_eq!(last["old_value"], format!("{sub}: Sub"));
}

async fn detail(t: &TestApp, key: &str) -> Value {
    t.get(&format!("/api/tickets/{key}")).await
}

async fn blockers_of(t: &TestApp, key: &str) -> Vec<String> {
    let d = detail(t, key).await;
    let mut keys: Vec<String> = d["links"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["label"] == "is blocked by")
        .map(|l| l["ticket"]["key"].as_str().unwrap().to_string())
        .collect();
    keys.sort();
    keys
}

#[tokio::test]
async fn deleting_requires_decisions_and_promote_bridge_rewire_the_work() {
    let t = TestApp::new().await;
    t.setup().await;
    let epic = t.ticket("Epic", json!({ "type": "epic" })).await;
    let story = t.ticket("Story", json!({ "type": "story", "parent": epic })).await;
    let sub1 = t.ticket("Sub 1", json!({ "type": "subtask", "parent": story })).await;
    let sub2 = t.ticket("Sub 2", json!({ "type": "subtask", "parent": story })).await;
    let before = t.ticket("Must happen first", json!({})).await;
    let after = t.ticket("Waits for the story", json!({})).await;
    assert_eq!(t.link(&before, &story).await.0, 201);
    assert_eq!(t.link(&story, &after).await.0, 201);

    // No decisions: refused, and the answer is the plan.
    let (status, err) = t.req(Method::DELETE, &format!("/api/tickets/{story}"), None).await;
    assert_eq!(status, 409);
    assert_eq!(err["error"]["code"], "decision_required");
    let plan = &err["error"]["detail"];
    assert_eq!(plan["children"].as_array().unwrap().len(), 2);
    assert_eq!(plan["parent"]["key"], epic);
    assert_eq!(plan["blockers"][0]["key"], before);
    assert_eq!(plan["dependents"][0]["key"], after);
    assert_eq!(plan["dependents"][0]["becomes_ready_if_dropped"], true, "only the story blocks it");
    assert_eq!(plan["children_options"], json!(["delete", "detach", "move", "promote"]));
    assert_eq!(plan["dependents_options"], json!(["drop", "bridge", "children", "transfer"]));
    assert_eq!(
        t.get(&format!("/api/tickets/{story}/delete-plan")).await,
        *plan,
        "the preview is the same plan"
    );

    // Only one of the two decisions: still refused.
    let (status, _) = t.req(Method::DELETE, &format!("/api/tickets/{story}?children=promote"), None).await;
    assert_eq!(status, 409);

    let uri = format!("/api/tickets/{story}?children=promote&dependents=bridge");
    assert_eq!(t.req(Method::DELETE, &uri, None).await.0, 204);
    for sub in [&sub1, &sub2] {
        let d = detail(&t, sub).await;
        assert_eq!((d["type"].as_str(), d["parent_key"].as_str()), (Some("task"), Some(epic.as_str())));
    }
    assert_eq!(blockers_of(&t, &after).await, vec![before.clone()], "A→X→B became A→B");
    assert_eq!(detail(&t, &after).await["is_blocked"], true);
    let history = t.get(&format!("/api/tickets/{sub1}/activity")).await;
    let fields: Vec<&str> = history.as_array().unwrap().iter().filter_map(|a| a["field"].as_str()).collect();
    assert!(fields.contains(&"type") && fields.contains(&"parent"), "{fields:?}");
}

#[tokio::test]
async fn detach_move_children_and_transfer_strategies() {
    let t = TestApp::new().await;
    t.setup().await;
    // detach + dependents=children: the dependent now waits for the former subtask.
    let story = t.ticket("Story", json!({ "type": "story" })).await;
    let sub = t.ticket("Sub", json!({ "type": "subtask", "parent": story })).await;
    let waiting = t.ticket("Waiting", json!({})).await;
    assert_eq!(t.link(&story, &waiting).await.0, 201);
    let uri = format!("/api/tickets/{story}?children=detach&dependents=children");
    assert_eq!(t.req(Method::DELETE, &uri, None).await.0, 204);
    let d = detail(&t, &sub).await;
    assert_eq!((d["type"].as_str(), d["parent_key"].clone()), (Some("task"), Value::Null));
    assert_eq!(blockers_of(&t, &waiting).await, vec![sub.clone()]);

    // move + transfer to the same replacement story.
    let old = t.ticket("Old story", json!({ "type": "story" })).await;
    let child = t.ticket("Child", json!({ "type": "subtask", "parent": old })).await;
    let replacement = t.ticket("Replacement", json!({ "type": "story" })).await;
    let epic = t.ticket("Epic", json!({ "type": "epic" })).await;
    let downstream = t.ticket("Downstream", json!({})).await;
    assert_eq!(t.link(&old, &downstream).await.0, 201);

    // Invalid choices change nothing.
    let bad = [
        format!("/api/tickets/{old}?children=move&move_to={epic}&dependents=drop"),
        format!("/api/tickets/{old}?children=delete&dependents=children"),
        format!("/api/tickets/{old}?children=move&dependents=drop"),
    ];
    for uri in &bad {
        assert_eq!(t.req(Method::DELETE, uri, None).await.0, 400, "{uri}");
    }
    assert_eq!(detail(&t, &child).await["parent_key"], old);

    let uri = format!(
        "/api/tickets/{old}?children=move&move_to={replacement}&dependents=transfer&transfer_to={replacement}"
    );
    assert_eq!(t.req(Method::DELETE, &uri, None).await.0, 204);
    assert_eq!(detail(&t, &child).await["parent_key"], replacement);
    assert_eq!(blockers_of(&t, &downstream).await, vec![replacement.clone()]);
    assert_eq!(t.req(Method::GET, &format!("/api/tickets/{old}"), None).await.0, 404);
}

#[tokio::test]
async fn a_rewire_that_would_create_a_cycle_deletes_nothing() {
    let t = TestApp::new().await;
    t.setup().await;
    let story = t.ticket("Story", json!({ "type": "story" })).await;
    let sub = t.ticket("Sub", json!({ "type": "subtask", "parent": story })).await;
    let dependent = t.ticket("Dependent", json!({})).await;
    assert_eq!(t.link(&story, &dependent).await.0, 201);
    assert_eq!(t.link(&dependent, &sub).await.0, 201, "the subtask waits for the dependent");

    // Handing the dependent to the subtask would make them wait on each other.
    let uri = format!("/api/tickets/{story}?children=detach&dependents=children");
    let (status, err) = t.req(Method::DELETE, &uri, None).await;
    assert_eq!(status, 409);
    assert_eq!(err["error"]["code"], "cycle");
    assert!(err["error"]["message"].as_str().unwrap().starts_with("Nothing was deleted"));
    let d = detail(&t, &sub).await;
    assert_eq!((d["type"].as_str(), d["parent_key"].as_str()), (Some("subtask"), Some(story.as_str())));
    assert_eq!(detail(&t, &story).await["key"], story, "the story still exists");

    // A ticket with nothing attached needs no decisions.
    let lone = t.ticket("Lone", json!({})).await;
    assert_eq!(t.req(Method::DELETE, &format!("/api/tickets/{lone}"), None).await.0, 204);
}
