//! Model Context Protocol server, so any MCP client (Claude Code, Claude
//! Desktop, Cursor, VS Code, ...) can use the board through typed tools.
//!
//! Two transports share one implementation:
//! - Streamable HTTP: `POST /mcp` on the board itself (JSON responses).
//! - stdio: `kanban-server mcp` forwards stdin/stdout to a running board's
//!   `/mcp` (see `bridge`), so there is still exactly one writer and every
//!   change shows up live in open browsers.
//!
//! Changes are attributed to the user named in the `X-Kanban-User` header
//! (`--as` for stdio); an unknown username is created as a new user.

pub mod bridge;
mod tools;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::{AppState, inproc, router};

/// Protocol revisions this server speaks, newest first. Tools-only servers
/// behave the same under all of them.
const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const DEFAULT_VERSION: &str = "2025-06-18";

const INSTRUCTIONS: &str = "A Jira-style Kanban board. Tickets are identified by keys like KAN-12 \
and belong to projects (KAN). Each project has its own workflow statuses: call list_projects or \
get_project to see them, and move a ticket with update_ticket's `status`. Hierarchy: epics hold \
stories, tasks and bugs; those hold subtasks. A \"blocks\" link from A to B means B cannot be \
finished before A. Use list_ready_tickets to find work that is not blocked, and add_comment to \
report progress or decisions on a ticket.";

pub const USER_HEADER: &str = "x-kanban-user";

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// `POST /mcp`. Replies 202 with no body to notifications and responses.
pub async fn http(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let user = headers.get(USER_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string);
    let message: Value = match serde_json::from_slice(&body) {
        Ok(m) => m,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(error(Value::Null, -32700, "Parse error")))
                .into_response();
        }
    };
    match handle(&state, user.as_deref(), message).await {
        Some(reply) => Json(reply).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// `GET /mcp` would open a server-to-client stream; this server has nothing
/// to push, which the spec allows it to say with 405.
pub async fn no_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

pub async fn handle(state: &AppState, user: Option<&str>, message: Value) -> Option<Value> {
    if message.is_array() {
        return Some(error(Value::Null, -32600, "Batch requests are not supported"));
    }
    let method = message.get("method").and_then(Value::as_str);
    let id = message.get("id").cloned();
    // A notification (no id), or the client answering a request of ours.
    let (Some(method), Some(id)) = (method, id) else { return None };
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    Some(match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or(DEFAULT_VERSION);
            let version = VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(DEFAULT_VERSION);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "kanban", "title": "Kanban board", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": tools::definitions() })),
        "tools/call" => {
            let Some(name) = params["name"].as_str() else {
                return Some(error(id, -32602, "tools/call needs a tool name"));
            };
            let args = params["arguments"].as_object().cloned().unwrap_or_default();
            let ctx = match context(state, user).await {
                Ok(ctx) => ctx,
                Err(message) => return Some(result(id, tool_output(Err(message)))),
            };
            match tools::run(&ctx, name, &args).await {
                Some(outcome) => result(id, tool_output(outcome)),
                None => error(id, -32602, &format!("Unknown tool: {name}")),
            }
        }
        other => error(id, -32601, &format!("Method not found: {other}")),
    })
}

fn tool_output(outcome: tools::ToolResult) -> Value {
    match outcome {
        Ok(value) => json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() }],
            "isError": false,
        }),
        Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
    }
}

/// The acting user for a tool call, created on first use so an agent can
/// simply be configured with a name.
async fn context(state: &AppState, user: Option<&str>) -> Result<tools::Ctx, String> {
    let app = router(state.clone());
    let actor = match user.map(str::trim).filter(|u| !u.is_empty()) {
        None => None,
        Some(name) => {
            let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
                .bind(name)
                .fetch_optional(&state.db)
                .await
                .map_err(|e| e.to_string())?;
            match existing {
                Some(id) => Some(id),
                None => {
                    let body = json!({ "username": name });
                    let (status, user) = inproc::call(&app, Method::POST, "/api/users", None, Some(body))
                        .await
                        .map_err(|e| e.to_string())?;
                    if status != 201 {
                        return Err(format!(
                            "Cannot act as \"{name}\": {}",
                            user["error"]["message"].as_str().unwrap_or("invalid username")
                        ));
                    }
                    user["id"].as_i64()
                }
            }
        }
    };
    Ok(tools::Ctx { app, actor })
}
