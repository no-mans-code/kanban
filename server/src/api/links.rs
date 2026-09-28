use std::collections::{BTreeSet, HashSet};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::settings::{blocks_edges, blocks_edges_visible, max_height};
use super::tickets::link_label;
use crate::auth::{Auth, Role};
use crate::dag::Graph;
use crate::error::{ApiResult, AppError};
use crate::models::{
    LINK_KINDS, TicketSummary, check_one_of, keys_for, log_activity, project_id_by_key, resolve_key,
    summaries,
};
use crate::{AppState, now_ms, write_tx};

#[derive(Deserialize)]
pub struct LinkBody {
    /// For `blocks`: the ticket that has to finish first.
    source: String,
    target: String,
    kind: String,
}

pub async fn create(
    State(state): State<AppState>,
    Auth(p): Auth,
    Json(body): Json<LinkBody>,
) -> ApiResult<StatusCode> {
    p.require_writable()?;
    check_one_of("kind", &body.kind, &LINK_KINDS)?;
    let mut tx = write_tx(&state.db).await?;
    let source = resolve_key(&mut tx, &body.source).await?;
    let target = resolve_key(&mut tx, &body.target).await?;
    // A link changes both tickets' history, so both need it.
    p.require_ticket(&mut tx, source, Role::Member).await?;
    p.require_ticket(&mut tx, target, Role::Member).await?;
    if source == target {
        return Err(AppError::invalid("A ticket cannot link to itself"));
    }
    let symmetric = body.kind == "relates";
    let existing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ticket_links
          WHERE kind = ? AND ((source_id = ? AND target_id = ?) OR (? AND source_id = ? AND target_id = ?))",
    )
    .bind(&body.kind)
    .bind(source)
    .bind(target)
    .bind(symmetric)
    .bind(target)
    .bind(source)
    .fetch_one(&mut *tx)
    .await?;
    if existing > 0 {
        return Err(AppError::conflict("duplicate", "These tickets are already linked that way"));
    }

    if body.kind == "blocks" {
        // The check always runs against the TRUE full graph, regardless of
        // what the caller can see — otherwise an edge that looks safe only
        // because part of the real graph was hidden could actually create
        // a real cycle. Only the error MESSAGE is filtered by visibility.
        let graph = Graph::new(&blocks_edges(&mut tx).await?);
        if let Some(cycle) = graph.cycle_if_added(source, target) {
            return Err(cycle_error(&mut tx, &p, &cycle).await?);
        }
        if let Some(limit) = max_height(&mut tx).await? {
            let chain = graph.longest_through(source, target);
            if chain.len() > limit {
                return Err(height_error(&mut tx, &p, &chain, limit).await?);
            }
        }
    }

    let now = now_ms();
    sqlx::query("INSERT INTO ticket_links (source_id, target_id, kind, created_at) VALUES (?, ?, ?, ?)")
        .bind(source)
        .bind(target)
        .bind(&body.kind)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    record(&mut tx, source, target, &body.kind, p.user_id, "linked").await?;
    tx.commit().await?;
    state.events.emit(
        "link.changed",
        None,
        &[body.source.to_ascii_uppercase(), body.target.to_ascii_uppercase()],
    );
    Ok(StatusCode::CREATED)
}

/// A cycle/height-limit error naming the full chain, unless it passes
/// through a project the caller can't see — then the chain is left out
/// entirely rather than naming tickets that don't officially exist to them.
async fn cycle_error(
    conn: &mut sqlx::SqliteConnection,
    p: &crate::auth::Principal,
    cycle: &[i64],
) -> ApiResult<AppError> {
    if p.sees_all(conn, cycle).await? {
        let keys = keys_for(conn, cycle).await?;
        Ok(AppError::conflict("cycle", format!("That would create a dependency cycle: {}", keys.join(" → ")))
            .with_detail(json!({ "chain": keys })))
    } else {
        Ok(AppError::conflict(
            "cycle",
            "That would create a dependency cycle through a ticket you don't have access to. Not created.",
        ))
    }
}

async fn height_error(
    conn: &mut sqlx::SqliteConnection,
    p: &crate::auth::Principal,
    chain: &[i64],
    limit: usize,
) -> ApiResult<AppError> {
    if p.sees_all(conn, chain).await? {
        let keys = keys_for(conn, chain).await?;
        Ok(AppError::conflict(
            "height_exceeded",
            format!(
                "That would make a dependency chain of {} tickets, over the limit of {limit}: {}",
                chain.len(),
                keys.join(" → ")
            ),
        )
        .with_detail(json!({ "chain": keys, "limit": limit })))
    } else {
        Ok(AppError::conflict(
            "height_exceeded",
            format!(
                "That would make a dependency chain of {} tickets, over the limit of {limit}, passing through a project you don't have access to. Not created.",
                chain.len()
            ),
        ))
    }
}

/// Also used by `comments::create` to log the `relates` links it makes from
/// ticket-key mentions.
pub(super) async fn record(
    conn: &mut sqlx::SqliteConnection,
    source: i64,
    target: i64,
    kind: &str,
    actor: Option<i64>,
    action: &str,
) -> ApiResult<()> {
    let keys = keys_for(conn, &[source, target]).await?;
    let now = now_ms();
    for (id, outward, other) in [(source, true, &keys[1]), (target, false, &keys[0])] {
        let label = link_label(kind, outward);
        let (old, new) =
            if action == "linked" { (None, Some(other.clone())) } else { (Some(other.clone()), None) };
        log_activity(conn, id, actor, action, Some(label), old, new).await?;
        sqlx::query("UPDATE tickets SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

pub async fn delete(
    State(state): State<AppState>,
    Auth(p): Auth,
    Query(q): Query<LinkBody>,
) -> ApiResult<StatusCode> {
    p.require_writable()?;
    check_one_of("kind", &q.kind, &LINK_KINDS)?;
    let mut tx = write_tx(&state.db).await?;
    let mut source = resolve_key(&mut tx, &q.source).await?;
    let mut target = resolve_key(&mut tx, &q.target).await?;
    p.require_ticket(&mut tx, source, Role::Member).await?;
    p.require_ticket(&mut tx, target, Role::Member).await?;
    let mut removed =
        sqlx::query("DELETE FROM ticket_links WHERE source_id = ? AND target_id = ? AND kind = ?")
            .bind(source)
            .bind(target)
            .bind(&q.kind)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if removed == 0 && q.kind == "relates" {
        removed = sqlx::query("DELETE FROM ticket_links WHERE source_id = ? AND target_id = ? AND kind = ?")
            .bind(target)
            .bind(source)
            .bind(&q.kind)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        std::mem::swap(&mut source, &mut target);
    }
    if removed == 0 {
        return Err(AppError::not_found("Link"));
    }
    record(&mut tx, source, target, &q.kind, p.user_id, "unlinked").await?;
    tx.commit().await?;
    state.events.emit("link.changed", None, &[q.source.to_ascii_uppercase(), q.target.to_ascii_uppercase()]);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct GraphQuery {
    project: Option<String>,
    /// Include tickets that have no dependency links.
    #[serde(default)]
    all: bool,
}

#[derive(Serialize)]
pub struct GraphEdge {
    source: i64,
    target: i64,
}

#[derive(Serialize)]
pub struct HierarchyEdge {
    parent: i64,
    child: i64,
}

#[derive(Serialize)]
pub struct GraphView {
    nodes: Vec<TicketSummary>,
    edges: Vec<GraphEdge>,
    hierarchy: Vec<HierarchyEdge>,
    /// Ticket ids on the longest dependency chain the caller can see.
    longest_chain: Vec<i64>,
    max_height: Option<usize>,
}

/// A project's dependency graph, scoped to what the caller can see. Tickets
/// from OTHER projects that a project's tickets depend on are included only
/// when the caller can also see that other project — never a dangling edge
/// into a project that, to this caller, doesn't exist.
pub async fn graph(
    State(state): State<AppState>,
    Auth(p): Auth,
    Query(q): Query<GraphQuery>,
) -> ApiResult<Json<GraphView>> {
    let mut conn = state.db.acquire().await?;
    let vis = p.visibility(&mut conn).await?;
    let edges = if p.is_site_admin() {
        blocks_edges(&mut conn).await?
    } else {
        blocks_edges_visible(&mut conn, &p).await?
    };

    let scope: Vec<i64> = match &q.project {
        Some(key) => {
            let pid = project_id_by_key(&mut conn, key).await?;
            p.require(&mut conn, pid, Role::Viewer).await?;
            sqlx::query_scalar("SELECT id FROM tickets WHERE project_id = ?")
                .bind(pid)
                .fetch_all(&mut *conn)
                .await?
        }
        None => {
            let mut all = Vec::new();
            let projects: Vec<i64> =
                sqlx::query_scalar("SELECT id FROM projects").fetch_all(&mut *conn).await?;
            for pid in projects {
                if vis.sees(pid) {
                    let mut ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM tickets WHERE project_id = ?")
                        .bind(pid)
                        .fetch_all(&mut *conn)
                        .await?;
                    all.append(&mut ids);
                }
            }
            all
        }
    };
    let in_scope: HashSet<i64> = scope.iter().copied().collect();
    let mut ids: BTreeSet<i64> = if q.all { scope.iter().copied().collect() } else { BTreeSet::new() };
    for &(s, t) in &edges {
        if in_scope.contains(&s) || in_scope.contains(&t) {
            ids.insert(s);
            ids.insert(t);
        }
    }
    let ids_vec: Vec<i64> = ids.iter().copied().collect();
    let found = summaries(&mut conn, &ids_vec).await?;
    let mut nodes: Vec<TicketSummary> = found.into_values().collect();
    nodes.sort_by_key(|n| (n.project_id, n.number));

    let hierarchy: Vec<HierarchyEdge> = nodes
        .iter()
        .filter_map(|n| {
            n.parent_id.filter(|p| ids.contains(p)).map(|p| HierarchyEdge { parent: p, child: n.id })
        })
        .collect();
    let longest_chain = Graph::new(&edges).longest_chain();
    let edges = edges
        .into_iter()
        .filter(|(s, t)| ids.contains(s) && ids.contains(t))
        .map(|(source, target)| GraphEdge { source, target })
        .collect();
    Ok(Json(GraphView { nodes, edges, hierarchy, longest_chain, max_height: max_height(&mut conn).await? }))
}
