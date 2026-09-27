//! Model Context Protocol server, so any MCP client (Claude Code, Claude
//! Desktop, Cursor, VS Code, ...) can use the board through typed tools.
//!
//! Two transports share one implementation:
//! - Streamable HTTP: `POST /mcp` on the board itself (JSON responses).
//! - stdio: `kanban-server mcp` forwards stdin/stdout to a running board's
//!   `/mcp` (see `bridge`), so there is still exactly one writer and every
//!   change shows up live in open browsers.
//!
//! **Authentication is a real API token** (`Authorization: Bearer kbn_...`),
//! the same one used everywhere else — never a client-supplied username.
//! An MCP tool call runs with exactly the token owner's access: a token
//! narrowed to one project can only see and touch that project through MCP,
//! the same as through the REST API or the UI. There is no way for an MCP
//! caller to act as anyone but the account the token belongs to.

pub mod bridge;
mod tools;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::auth::Auth;
use crate::{AppState, router};

/// Protocol revisions this server speaks, newest first. Tools-only servers
/// behave the same under all of them.
const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const DEFAULT_VERSION: &str = "2025-06-18";

const INSTRUCTIONS: &str = "A Jira-style Kanban board. Tickets are identified by keys like KAN-12 \
and belong to projects (KAN). Each project has its own workflow statuses: call list_projects or \
get_project to see them, and move a ticket with update_ticket's `status`. Hierarchy: epics hold \
stories, tasks and bugs; those hold subtasks. A \"blocks\" link from A to B means B cannot be \
finished before A. Use list_ready_tickets to find work that is not blocked, and add_comment to \
report progress or decisions on a ticket. You only see the projects your API token has access to \
— if a project or ticket seems to be missing, it may need to be shared with your account first.";

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// `POST /mcp`. Requires the same bearer-token authentication as the rest of
/// the API — enforced by the global auth middleware before this handler is
/// even reached, so `Auth` below is always a real, checked identity. Replies
/// 202 with no body to notifications and responses.
pub async fn http(State(state): State<AppState>, Auth(p): Auth, body: Bytes) -> Response {
    let message: Value = match serde_json::from_slice(&body) {
        Ok(m) => m,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(error(Value::Null, -32700, "Parse error")))
                .into_response();
        }
    };
    match handle(&state, p, message).await {
        Some(reply) => Json(reply).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// `GET /mcp` would open a server-to-client stream; this server has nothing
/// to push, which the spec allows it to say with 405.
pub async fn no_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

pub async fn handle(state: &AppState, principal: crate::auth::Principal, message: Value) -> Option<Value> {
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
            let ctx = tools::Ctx { app: router(state.clone()), principal };
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
