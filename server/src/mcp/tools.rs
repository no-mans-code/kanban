//! The MCP tools. Each one calls the board's own HTTP API in-process, so an
//! agent gets exactly the validation, DAG checks, history and live updates a
//! person in the browser gets. Arguments use the identifiers people use
//! (ticket keys, status names, usernames, label names), never database ids.

use std::collections::HashMap;

use axum::Router;
use axum::http::Method;
use serde_json::{Map, Value, json};

use crate::inproc::{call, enc};

/// A tool failure the model should see and can act on (shown with isError).
pub type ToolResult = Result<Value, String>;

pub struct Ctx {
    pub app: Router,
    pub actor: Option<i64>,
}

pub fn definitions() -> Value {
    let read_only = json!({ "readOnlyHint": true, "openWorldHint": false });
    let writes = json!({ "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false });
    let key = json!({ "type": "string", "description": "Ticket key, e.g. KAN-12" });
    json!([
        {
            "name": "list_projects",
            "description": "List every project with its key and workflow statuses (the board columns, left to right).",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": read_only,
        },
        {
            "name": "get_project",
            "description": "One project's statuses with the number of tickets in each, plus the labels in use.",
            "inputSchema": {
                "type": "object",
                "properties": { "project": { "type": "string", "description": "Project key, e.g. KAN" } },
                "required": ["project"],
            },
            "annotations": read_only,
        },
        {
            "name": "list_users",
            "description": "People and agents who can be assigned tickets, by username.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": read_only,
        },
        {
            "name": "search_tickets",
            "description": "Find tickets. Every filter is optional and they combine with AND. `query` searches titles, descriptions and comments, or matches an exact key.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project key" },
                    "query": { "type": "string", "description": "Free text or a ticket key" },
                    "status": { "type": "string", "description": "Status name, e.g. \"In Progress\"" },
                    "assignee": { "type": "string", "description": "Username, \"me\", or \"none\" for unassigned" },
                    "type": { "type": "string", "enum": ["epic", "story", "task", "bug", "subtask"] },
                    "priority": { "type": "string", "enum": ["highest", "high", "medium", "low", "lowest"] },
                    "label": { "type": "string", "description": "Label name" },
                    "parent": { "type": "string", "description": "Only children of this ticket key" },
                    "blocked": { "type": "boolean", "description": "true: only tickets waiting on an unfinished blocker; false: only unblocked ones" },
                    "include_done": { "type": "boolean", "description": "Include tickets in done statuses (default true)" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Default 50" },
                },
            },
            "annotations": read_only,
        },
        {
            "name": "list_ready_tickets",
            "description": "What can be worked on next in a project: tickets that are not done, not epics, and not waiting on any unfinished blocker, highest priority first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project key" },
                    "assignee": { "type": "string", "description": "Username, \"me\", or \"none\"; omit for everyone" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Default 20" },
                },
                "required": ["project"],
            },
            "annotations": read_only,
        },
        {
            "name": "get_ticket",
            "description": "Everything about one ticket: description, status, people, parent, children, links (blocks / is blocked by / ...), and comments.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": key,
                    "include_history": { "type": "boolean", "description": "Also return the change history (default false)" },
                },
                "required": ["key"],
            },
            "annotations": read_only,
        },
        {
            "name": "create_ticket",
            "description": "Create a ticket. Hierarchy follows Jira: an epic holds stories, tasks and bugs; those hold subtasks. A subtask needs a parent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project key" },
                    "title": { "type": "string" },
                    "type": { "type": "string", "enum": ["epic", "story", "task", "bug", "subtask"], "description": "Default task" },
                    "description": { "type": "string", "description": "Markdown" },
                    "priority": { "type": "string", "enum": ["highest", "high", "medium", "low", "lowest"] },
                    "status": { "type": "string", "description": "Status name; defaults to the first column" },
                    "assignee": { "type": "string", "description": "Username or \"me\"" },
                    "parent": { "type": "string", "description": "Parent ticket key" },
                    "labels": { "type": "array", "items": { "type": "string" }, "description": "Label names; missing labels are created" },
                },
                "required": ["project", "title"],
            },
            "annotations": writes,
        },
        {
            "name": "update_ticket",
            "description": "Change fields of a ticket. Only the fields you pass change. Use status to move it across the board.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": key,
                    "title": { "type": "string" },
                    "description": { "type": "string", "description": "Markdown; replaces the whole description" },
                    "type": { "type": "string", "enum": ["epic", "story", "task", "bug", "subtask"] },
                    "priority": { "type": "string", "enum": ["highest", "high", "medium", "low", "lowest"] },
                    "status": { "type": "string", "description": "Status name, e.g. \"Done\"" },
                    "assignee": { "type": "string", "description": "Username, \"me\", or \"none\" to unassign" },
                    "parent": { "type": "string", "description": "Parent ticket key, or \"none\" to detach" },
                    "labels": { "type": "array", "items": { "type": "string" }, "description": "Replace all labels" },
                    "add_labels": { "type": "array", "items": { "type": "string" } },
                    "remove_labels": { "type": "array", "items": { "type": "string" } },
                },
                "required": ["key"],
            },
            "annotations": writes,
        },
        {
            "name": "add_comment",
            "description": "Add a Markdown comment to a ticket. Mention other tickets by key (e.g. KAN-3) and they become links.",
            "inputSchema": {
                "type": "object",
                "properties": { "key": key, "body": { "type": "string" } },
                "required": ["key", "body"],
            },
            "annotations": writes,
        },
        {
            "name": "link_tickets",
            "description": "Link two tickets. With kind \"blocks\" (the default), `source` must be finished before `target` can be; the board rejects links that would create a cycle or exceed the configured maximum dependency depth.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Ticket key" },
                    "target": { "type": "string", "description": "Ticket key" },
                    "kind": { "type": "string", "enum": ["blocks", "relates", "duplicates", "clones"] },
                },
                "required": ["source", "target"],
            },
            "annotations": writes,
        },
        {
            "name": "unlink_tickets",
            "description": "Remove a link created with link_tickets.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string" },
                    "target": { "type": "string" },
                    "kind": { "type": "string", "enum": ["blocks", "relates", "duplicates", "clones"] },
                },
                "required": ["source", "target"],
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false },
        },
    ])
}

pub async fn run(ctx: &Ctx, name: &str, args: &Map<String, Value>) -> Option<ToolResult> {
    Some(match name {
        "list_projects" => ctx.list_projects().await,
        "get_project" => ctx.get_project(args).await,
        "list_users" => ctx.list_users().await,
        "search_tickets" => ctx.search(args).await,
        "list_ready_tickets" => ctx.ready(args).await,
        "get_ticket" => ctx.get_ticket(args).await,
        "create_ticket" => ctx.create_ticket(args).await,
        "update_ticket" => ctx.update_ticket(args).await,
        "add_comment" => ctx.add_comment(args).await,
        "link_tickets" => ctx.link(args, true).await,
        "unlink_tickets" => ctx.link(args, false).await,
        _ => return None,
    })
}

// ---------------------------------------------------------------- argument helpers

fn text<'a>(args: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn required<'a>(args: &'a Map<String, Value>, name: &str) -> Result<&'a str, String> {
    text(args, name).ok_or_else(|| format!("`{name}` is required"))
}

fn names(args: &Map<String, Value>, name: &str) -> Option<Vec<String>> {
    args.get(name)?.as_array().map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
}

fn limit(args: &Map<String, Value>, default: usize) -> usize {
    args.get("limit").and_then(Value::as_u64).map(|n| n.clamp(1, 200) as usize).unwrap_or(default)
}

/// Unix milliseconds to an ISO 8601 UTC timestamp.
fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days, from Howard Hinnant's date algorithms.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Names for ids, so results never show database ids.
struct Names {
    users: HashMap<i64, String>,
    labels: HashMap<i64, String>,
}

impl Names {
    fn user(&self, id: &Value) -> Value {
        id.as_i64().and_then(|i| self.users.get(&i)).map(|u| json!(u)).unwrap_or(Value::Null)
    }

    fn brief(&self, t: &Value) -> Value {
        let labels: Vec<&String> = t["label_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| self.labels.get(&l.as_i64()?))
            .collect();
        json!({
            "key": t["key"],
            "title": t["title"],
            "type": t["type"],
            "status": t["status_name"],
            "done": t["status_category"] == "done",
            "priority": t["priority"],
            "assignee": self.user(&t["assignee_id"]),
            "parent": t["parent_key"],
            "labels": labels,
            "blocked": t["is_blocked"],
            "children": t["child_count"],
            "comments": t["comment_count"],
            "updated_at": t["updated_at"].as_i64().map(iso),
        })
    }
}

// ---------------------------------------------------------------- tools

impl Ctx {
    async fn request(&self, method: Method, uri: &str, body: Option<Value>) -> ToolResult {
        let (status, value) =
            call(&self.app, method, uri, self.actor, body).await.map_err(|e| e.to_string())?;
        if (200..300).contains(&status) {
            Ok(value)
        } else {
            Err(value["error"]["message"].as_str().unwrap_or("The board rejected the request").to_string())
        }
    }

    async fn get(&self, uri: &str) -> ToolResult {
        self.request(Method::GET, uri, None).await
    }

    async fn names(&self) -> Result<Names, String> {
        let collect = |v: Value, field: &str| -> HashMap<i64, String> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| Some((x["id"].as_i64()?, x[field].as_str()?.to_string())))
                .collect()
        };
        Ok(Names {
            users: collect(self.get("/api/users").await?, "username"),
            labels: collect(self.get("/api/labels").await?, "name"),
        })
    }

    /// "me", "none" or a username -> Some(user id) / None (unassigned).
    async fn user_id(&self, who: &str) -> Result<Option<i64>, String> {
        match who.to_ascii_lowercase().as_str() {
            "none" | "unassigned" | "" => Ok(None),
            "me" => self.actor.map(Some).ok_or_else(|| {
                "\"me\" needs an acting user: set the X-Kanban-User header (or --as for stdio)".to_string()
            }),
            name => {
                let users = self.get("/api/users").await?;
                users
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|u| u["username"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
                    .and_then(|u| u["id"].as_i64())
                    .map(Some)
                    .ok_or_else(|| format!("No user named \"{who}\". Call list_users to see who exists."))
            }
        }
    }

    /// Label names -> ids, creating labels that don't exist yet.
    async fn label_ids(&self, wanted: &[String]) -> Result<Vec<i64>, String> {
        let existing = self.get("/api/labels").await?;
        let mut ids = Vec::new();
        for name in wanted {
            let found = existing
                .as_array()
                .into_iter()
                .flatten()
                .find(|l| l["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name.trim())));
            let id = match found {
                Some(l) => l["id"].as_i64(),
                None => self.request(Method::POST, "/api/labels", Some(json!({ "name": name }))).await?["id"]
                    .as_i64(),
            };
            ids.extend(id);
        }
        Ok(ids)
    }

    async fn status_id(&self, project: &str, name: &str) -> Result<i64, String> {
        let p = self.get(&format!("/api/projects/{}", enc(project))).await?;
        let statuses = p["statuses"].as_array().cloned().unwrap_or_default();
        statuses
            .iter()
            .find(|s| s["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
            .and_then(|s| s["id"].as_i64())
            .ok_or_else(|| {
                let valid: Vec<&str> = statuses.iter().filter_map(|s| s["name"].as_str()).collect();
                format!("No status \"{name}\" in {project}. Valid: {}", valid.join(", "))
            })
    }

    async fn list_projects(&self) -> ToolResult {
        let projects = self.get("/api/projects").await?;
        let mut out = Vec::new();
        for p in projects.as_array().into_iter().flatten() {
            let key = p["key"].as_str().unwrap_or_default();
            let detail = self.get(&format!("/api/projects/{}", enc(key))).await?;
            let statuses: Vec<&Value> =
                detail["statuses"].as_array().into_iter().flatten().map(|s| &s["name"]).collect();
            out.push(json!({ "key": key, "name": p["name"], "description": p["description"], "statuses": statuses }));
        }
        Ok(json!(out))
    }

    async fn get_project(&self, args: &Map<String, Value>) -> ToolResult {
        let project = required(args, "project")?;
        let detail = self.get(&format!("/api/projects/{}", enc(project))).await?;
        let tickets = self.get(&format!("/api/tickets?project={}&limit=5000", enc(project))).await?;
        let tickets = tickets.as_array().cloned().unwrap_or_default();
        let names = self.names().await?;
        let statuses: Vec<Value> = detail["statuses"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                let count = tickets.iter().filter(|t| t["status_id"] == s["id"]).count();
                json!({ "name": s["name"], "category": s["category"], "tickets": count })
            })
            .collect();
        let mut labels: Vec<&String> = names.labels.values().collect();
        labels.sort();
        Ok(json!({
            "key": detail["key"], "name": detail["name"], "description": detail["description"],
            "statuses": statuses, "labels": labels,
        }))
    }

    async fn list_users(&self) -> ToolResult {
        let users = self.get("/api/users").await?;
        Ok(json!(
            users
                .as_array()
                .into_iter()
                .flatten()
                .map(
                    |u| json!({ "username": u["username"], "name": u["display_name"], "active": u["active"] })
                )
                .collect::<Vec<_>>()
        ))
    }

    /// Fetch with server-side filters, then apply the ones the API doesn't have.
    async fn find(&self, args: &Map<String, Value>) -> Result<Vec<Value>, String> {
        let mut query = vec!["limit=5000".to_string(), "sort=priority".to_string()];
        for (arg, param) in [
            ("project", "project"),
            ("query", "q"),
            ("type", "type"),
            ("priority", "priority"),
            ("parent", "parent"),
        ] {
            if let Some(v) = text(args, arg) {
                query.push(format!("{param}={}", enc(v)));
            }
        }
        if let Some(who) = text(args, "assignee") {
            let id = self.user_id(who).await?;
            query.push(format!("assignee={}", id.map_or("none".into(), |i| i.to_string())));
        }
        if let Some(label) = text(args, "label") {
            let labels = self.get("/api/labels").await?;
            let id = labels
                .as_array()
                .into_iter()
                .flatten()
                .find(|l| l["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(label)))
                .and_then(|l| l["id"].as_i64())
                .ok_or_else(|| format!("No label named \"{label}\""))?;
            query.push(format!("label_id={id}"));
        }
        let rows = self.get(&format!("/api/tickets?{}", query.join("&"))).await?;
        let status = text(args, "status");
        let blocked = args.get("blocked").and_then(Value::as_bool);
        let include_done = args.get("include_done").and_then(Value::as_bool).unwrap_or(true);
        Ok(rows
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| {
                status.is_none_or(|s| t["status_name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(s)))
            })
            .filter(|t| blocked.is_none_or(|b| t["is_blocked"].as_bool() == Some(b)))
            .filter(|t| include_done || t["status_category"] != "done")
            .collect())
    }

    async fn search(&self, args: &Map<String, Value>) -> ToolResult {
        let found = self.find(args).await?;
        let names = self.names().await?;
        let shown: Vec<Value> = found.iter().take(limit(args, 50)).map(|t| names.brief(t)).collect();
        Ok(json!({ "total": found.len(), "returned": shown.len(), "tickets": shown }))
    }

    async fn ready(&self, args: &Map<String, Value>) -> ToolResult {
        let mut filters = args.clone();
        filters.insert("blocked".into(), json!(false));
        filters.insert("include_done".into(), json!(false));
        let found: Vec<Value> =
            self.find(&filters).await?.into_iter().filter(|t| t["type"] != "epic").collect();
        let names = self.names().await?;
        let shown: Vec<Value> = found.iter().take(limit(args, 20)).map(|t| names.brief(t)).collect();
        Ok(json!({ "total": found.len(), "tickets": shown }))
    }

    async fn get_ticket(&self, args: &Map<String, Value>) -> ToolResult {
        let key = required(args, "key")?;
        let t = self.get(&format!("/api/tickets/{}", enc(key))).await?;
        let comments = self.get(&format!("/api/tickets/{}/comments", enc(key))).await?;
        let names = self.names().await?;
        let mut out = names.brief(&t);
        let o = out.as_object_mut().expect("brief is an object");
        o.insert("description".into(), t["description"].clone());
        o.insert("reporter".into(), names.user(&t["reporter_id"]));
        o.insert(
            "watchers".into(),
            json!(
                t["watcher_ids"].as_array().into_iter().flatten().map(|w| names.user(w)).collect::<Vec<_>>()
            ),
        );
        o.insert("created_at".into(), json!(t["created_at"].as_i64().map(iso)));
        o.insert("resolved_at".into(), json!(t["resolved_at"].as_i64().map(iso)));
        o.insert(
            "children".into(),
            json!(t["children"].as_array().into_iter().flatten().map(|c| names.brief(c)).collect::<Vec<_>>()),
        );
        o.insert(
            "links".into(),
            json!(
                t["links"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|l| json!({
                        "relation": l["label"],
                        "key": l["ticket"]["key"],
                        "title": l["ticket"]["title"],
                        "status": l["ticket"]["status_name"],
                        "done": l["ticket"]["status_category"] == "done",
                    }))
                    .collect::<Vec<_>>()
            ),
        );
        o.insert(
            "comments".into(),
            json!(
                comments
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| json!({ "author": names.user(&c["author_id"]), "at": c["created_at"].as_i64().map(iso), "body": c["body"] }))
                    .collect::<Vec<_>>()
            ),
        );
        if args.get("include_history").and_then(Value::as_bool).unwrap_or(false) {
            let history = self.get(&format!("/api/tickets/{}/activity", enc(key))).await?;
            let rows: Vec<Value> = history
                .as_array()
                .into_iter()
                .flatten()
                .map(|a| json!({
                    "by": names.user(&a["actor_id"]), "at": a["created_at"].as_i64().map(iso),
                    "action": a["action"], "field": a["field"], "from": a["old_value"], "to": a["new_value"],
                }))
                .collect();
            o.insert("history".into(), json!(rows));
        }
        Ok(out)
    }

    async fn create_ticket(&self, args: &Map<String, Value>) -> ToolResult {
        let project = required(args, "project")?;
        let mut body = json!({ "project": project, "title": required(args, "title")? });
        let b = body.as_object_mut().expect("object");
        for field in ["type", "description", "priority", "parent"] {
            if let Some(v) = text(args, field) {
                b.insert(field.into(), json!(v));
            }
        }
        if let Some(s) = text(args, "status") {
            b.insert("status_id".into(), json!(self.status_id(project, s).await?));
        }
        if let Some(who) = text(args, "assignee") {
            b.insert("assignee_id".into(), json!(self.user_id(who).await?));
        }
        if let Some(labels) = names(args, "labels") {
            b.insert("label_ids".into(), json!(self.label_ids(&labels).await?));
        }
        let created = self.request(Method::POST, "/api/tickets", Some(body)).await?;
        Ok(self.names().await?.brief(&created))
    }

    async fn update_ticket(&self, args: &Map<String, Value>) -> ToolResult {
        let key = required(args, "key")?;
        let current = self.get(&format!("/api/tickets/{}", enc(key))).await?;
        let project = key.rsplit_once('-').map(|(p, _)| p).unwrap_or(key);
        let mut patch = Map::new();
        for field in ["title", "description", "type", "priority"] {
            if let Some(v) = args.get(field).and_then(Value::as_str) {
                patch.insert(field.into(), json!(v));
            }
        }
        if let Some(s) = text(args, "status") {
            patch.insert("status_id".into(), json!(self.status_id(project, s).await?));
        }
        match args.get("assignee") {
            Some(Value::Null) => {
                patch.insert("assignee_id".into(), Value::Null);
            }
            Some(Value::String(who)) => {
                patch.insert("assignee_id".into(), json!(self.user_id(who).await?));
            }
            _ => {}
        }
        match args.get("parent") {
            Some(Value::Null) => {
                patch.insert("parent".into(), Value::Null);
            }
            Some(Value::String(p)) if p.eq_ignore_ascii_case("none") || p.trim().is_empty() => {
                patch.insert("parent".into(), Value::Null);
            }
            Some(Value::String(p)) => {
                patch.insert("parent".into(), json!(p));
            }
            _ => {}
        }
        let (replace, add, remove) =
            (names(args, "labels"), names(args, "add_labels"), names(args, "remove_labels"));
        if replace.is_some() || add.is_some() || remove.is_some() {
            let mut ids: Vec<i64> = match &replace {
                Some(l) => self.label_ids(l).await?,
                None => {
                    current["label_ids"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect()
                }
            };
            if let Some(l) = &add {
                ids.extend(self.label_ids(l).await?);
            }
            if let Some(l) = &remove {
                let names = self.names().await?;
                ids.retain(|id| {
                    !names.labels.get(id).is_some_and(|n| l.iter().any(|r| r.eq_ignore_ascii_case(n)))
                });
            }
            ids.sort_unstable();
            ids.dedup();
            patch.insert("label_ids".into(), json!(ids));
        }
        if patch.is_empty() {
            return Err("Nothing to change: pass at least one field to update".into());
        }
        let updated = self
            .request(Method::PATCH, &format!("/api/tickets/{}", enc(key)), Some(Value::Object(patch)))
            .await?;
        Ok(self.names().await?.brief(&updated))
    }

    async fn add_comment(&self, args: &Map<String, Value>) -> ToolResult {
        let key = required(args, "key")?;
        let body = required(args, "body")?;
        self.request(
            Method::POST,
            &format!("/api/tickets/{}/comments", enc(key)),
            Some(json!({ "body": body })),
        )
        .await?;
        Ok(json!({ "ok": true, "ticket": key.to_ascii_uppercase() }))
    }

    async fn link(&self, args: &Map<String, Value>, create: bool) -> ToolResult {
        let source = required(args, "source")?.to_ascii_uppercase();
        let target = required(args, "target")?.to_ascii_uppercase();
        let kind = text(args, "kind").unwrap_or("blocks");
        if create {
            let body = json!({ "source": source, "target": target, "kind": kind });
            self.request(Method::POST, "/api/links", Some(body)).await?;
        } else {
            let uri =
                format!("/api/links?source={}&target={}&kind={}", enc(&source), enc(&target), enc(kind));
            self.request(Method::DELETE, &uri, None).await?;
        }
        let verb = if create { "now" } else { "no longer" };
        let relation = match kind {
            "blocks" => format!("{source} {verb} blocks {target} ({target} waits for {source})"),
            other => format!("{source} {verb} {other} {target}"),
        };
        Ok(json!({ "ok": true, "result": relation }))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso_dates() {
        assert_eq!(super::iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::iso(1_790_491_175_007), "2026-09-27T06:39:35Z");
        assert_eq!(super::iso(951_782_400_000), "2000-02-29T00:00:00Z");
    }
}
