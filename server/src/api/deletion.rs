//! Deleting a ticket is a decision, not a side effect. The plan endpoint
//! shows everything a deletion would touch; the delete endpoint refuses to
//! guess and requires an explicit choice for the ticket's children and for
//! the tickets waiting on it.

use std::collections::{BTreeSet, HashMap};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqliteConnection;

use super::Actor;
use super::settings::{blocks_edges, max_height};
use super::tickets::link_label;
use crate::dag::Graph;
use crate::error::{ApiResult, AppError};
use crate::models::{TicketSummary, keys_for, log_activity, resolve_key, summaries, summary, valid_parent};
use crate::{AppState, now_ms, write_tx};

#[derive(Serialize)]
pub struct Dependent {
    #[serde(flatten)]
    ticket: TicketSummary,
    /// Dropping the dependency leaves it with no unfinished blocker.
    becomes_ready_if_dropped: bool,
}

#[derive(Serialize)]
pub struct OtherLink {
    label: &'static str,
    ticket: TicketSummary,
}

#[derive(Serialize)]
pub struct DeletePlan {
    ticket: TicketSummary,
    parent: Option<TicketSummary>,
    /// Direct children, and their own children (an epic's stories' subtasks).
    children: Vec<TicketSummary>,
    grandchildren: Vec<TicketSummary>,
    /// Tickets this one waits for.
    blockers: Vec<TicketSummary>,
    /// Tickets waiting for this one.
    dependents: Vec<Dependent>,
    /// Tickets waiting for one of its children; only affected if the children are deleted.
    children_dependents: Vec<TicketSummary>,
    other_links: Vec<OtherLink>,
    children_options: Vec<&'static str>,
    dependents_options: Vec<&'static str>,
}

fn sorted(map: &HashMap<i64, TicketSummary>, ids: &[i64]) -> Vec<TicketSummary> {
    let mut out: Vec<TicketSummary> = ids.iter().filter_map(|id| map.get(id).cloned()).collect();
    out.sort_by_key(|t| (t.project_id, t.number));
    out.dedup_by_key(|t| t.id);
    out
}

async fn ids(conn: &mut SqliteConnection, sql: &'static str, id: i64) -> ApiResult<Vec<i64>> {
    Ok(sqlx::query_scalar(sql).bind(id).fetch_all(&mut *conn).await?)
}

pub async fn build_plan(conn: &mut SqliteConnection, id: i64) -> ApiResult<DeletePlan> {
    let ticket = summary(conn, id).await?;
    let children = ids(conn, "SELECT id FROM tickets WHERE parent_id = ?", id).await?;
    let grandchildren = ids(
        conn,
        "SELECT g.id FROM tickets g JOIN tickets c ON c.id = g.parent_id WHERE c.parent_id = ?",
        id,
    )
    .await?;
    let blockers =
        ids(conn, "SELECT source_id FROM ticket_links WHERE target_id = ? AND kind = 'blocks'", id).await?;
    let dependents =
        ids(conn, "SELECT target_id FROM ticket_links WHERE source_id = ? AND kind = 'blocks'", id).await?;
    let children_dependents = ids(
        conn,
        "SELECT l.target_id FROM ticket_links l JOIN tickets c ON c.id = l.source_id
          WHERE c.parent_id = ? AND l.kind = 'blocks'",
        id,
    )
    .await?;
    let others: Vec<(String, i64, bool)> = sqlx::query_as(
        "SELECT kind, target_id, 1 FROM ticket_links WHERE source_id = ? AND kind <> 'blocks'
         UNION ALL
         SELECT kind, source_id, 0 FROM ticket_links WHERE target_id = ? AND kind <> 'blocks'",
    )
    .bind(id)
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;

    let mut all: Vec<i64> = [&children, &grandchildren, &blockers, &dependents, &children_dependents]
        .into_iter()
        .flatten()
        .copied()
        .chain(others.iter().map(|o| o.1))
        .chain(ticket.parent_id)
        .collect();
    all.sort_unstable();
    all.dedup();
    let map = summaries(conn, &all).await?;

    let mut deps = Vec::new();
    for d in sorted(&map, &dependents) {
        let others_unfinished: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM ticket_links l JOIN tickets b ON b.id = l.source_id
               JOIN statuses s ON s.id = b.status_id
              WHERE l.target_id = ? AND l.kind = 'blocks' AND l.source_id <> ? AND s.category <> 'done'",
        )
        .bind(d.id)
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
        let ready = d.status_category != "done" && others_unfinished == 0;
        deps.push(Dependent { ticket: d, becomes_ready_if_dropped: ready });
    }

    let parent = ticket.parent_id.and_then(|p| map.get(&p).cloned());
    let children_v = sorted(&map, &children);
    let mut children_options = Vec::new();
    if !children_v.is_empty() {
        children_options.extend(["delete", "detach", "move"]);
        let all_subtasks = children_v.iter().all(|c| c.ticket_type == "subtask");
        if all_subtasks && parent.as_ref().is_some_and(|p| valid_parent(&p.ticket_type, "task")) {
            children_options.push("promote");
        }
    }
    let mut dependents_options = Vec::new();
    if !deps.is_empty() {
        dependents_options.push("drop");
        if !blockers.is_empty() {
            dependents_options.push("bridge");
        }
        if !children_v.is_empty() {
            dependents_options.push("children");
        }
        dependents_options.push("transfer");
    }

    Ok(DeletePlan {
        ticket,
        parent,
        children: children_v,
        grandchildren: sorted(&map, &grandchildren),
        blockers: sorted(&map, &blockers),
        dependents: deps,
        children_dependents: sorted(&map, &children_dependents),
        other_links: others
            .into_iter()
            .filter_map(|(kind, other, outward)| {
                Some(OtherLink { label: link_label(&kind, outward), ticket: map.get(&other)?.clone() })
            })
            .collect(),
        children_options,
        dependents_options,
    })
}

pub async fn plan(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<Json<DeletePlan>> {
    let mut conn = state.db.acquire().await?;
    let id = resolve_key(&mut conn, &key).await?;
    Ok(Json(build_plan(&mut conn, id).await?))
}

#[derive(Deserialize)]
pub struct Decisions {
    /// delete | detach | move | promote
    children: Option<String>,
    /// drop | bridge | children | transfer
    dependents: Option<String>,
    move_to: Option<String>,
    transfer_to: Option<String>,
}

fn decide<'a>(
    what: &str,
    given: Option<&'a str>,
    options: &[&'static str],
    plan: &DeletePlan,
) -> ApiResult<Option<&'a str>> {
    if options.is_empty() {
        return Ok(None);
    }
    let Some(choice) = given else {
        return Err(AppError::conflict(
            "decision_required",
            format!(
                "{} has {what}. Say what happens to them: {what}={}",
                plan.ticket.key,
                options.join(" | ")
            ),
        )
        .with_detail(serde_json::to_value(plan).unwrap_or_default()));
    };
    if !options.contains(&choice) {
        return Err(AppError::invalid(format!("{what} must be one of: {}", options.join(", "))));
    }
    Ok(Some(choice))
}

async fn set_parent(
    conn: &mut SqliteConnection,
    child: &TicketSummary,
    parent: Option<(i64, &str)>,
    deleted_key: &str,
    actor: Option<i64>,
) -> ApiResult<()> {
    sqlx::query("UPDATE tickets SET parent_id = ? WHERE id = ?")
        .bind(parent.map(|p| p.0))
        .bind(child.id)
        .execute(&mut *conn)
        .await?;
    let new = match parent {
        Some((_, key)) => format!("{key} ({deleted_key} deleted)"),
        None => format!("None ({deleted_key} deleted)"),
    };
    log_activity(conn, child.id, actor, "updated", Some("parent"), Some(deleted_key.to_string()), Some(new))
        .await
}

async fn make_task(conn: &mut SqliteConnection, child: &TicketSummary, actor: Option<i64>) -> ApiResult<()> {
    sqlx::query("UPDATE tickets SET type = 'task' WHERE id = ?").bind(child.id).execute(&mut *conn).await?;
    log_activity(conn, child.id, actor, "updated", Some("type"), Some("subtask".into()), Some("task".into()))
        .await
}

pub async fn delete(
    State(state): State<AppState>,
    Actor(actor): Actor,
    Path(key): Path<String>,
    Query(q): Query<Decisions>,
) -> ApiResult<StatusCode> {
    let mut tx = write_tx(&state.db).await?;
    let id = resolve_key(&mut tx, &key).await?;
    let plan = build_plan(&mut tx, id).await?;
    let children_choice = decide("children", q.children.as_deref(), &plan.children_options, &plan)?;
    let dependents_choice = decide("dependents", q.dependents.as_deref(), &plan.dependents_options, &plan)?;
    let x = &plan.ticket;
    let mut doomed: BTreeSet<i64> = BTreeSet::from([x.id]);
    let mut touched: BTreeSet<i64> = BTreeSet::new();

    // ---- children
    match children_choice {
        Some("delete") => {
            doomed.extend(plan.children.iter().map(|c| c.id));
            doomed.extend(plan.grandchildren.iter().map(|g| g.id));
        }
        Some("detach") => {
            for c in &plan.children {
                // A subtask can't exist without a parent; it becomes a task.
                if c.ticket_type == "subtask" {
                    make_task(&mut tx, c, actor).await?;
                }
                set_parent(&mut tx, c, None, &x.key, actor).await?;
                touched.insert(c.id);
            }
        }
        Some("move") => {
            let target_key =
                q.move_to.as_deref().ok_or_else(|| AppError::invalid("children=move needs move_to=KEY"))?;
            let target_id = resolve_key(&mut tx, target_key).await?;
            let target = summary(&mut tx, target_id).await?;
            if target.id == x.id || plan.children.iter().any(|c| c.id == target.id) {
                return Err(AppError::invalid("move_to must be a ticket outside the one being deleted"));
            }
            if target.project_id != x.project_id {
                return Err(AppError::invalid("Children can only move to a ticket in the same project"));
            }
            for c in &plan.children {
                if !valid_parent(&target.ticket_type, &c.ticket_type) {
                    return Err(AppError::invalid(format!(
                        "{} is a {} and can't go under {}, which is a {}",
                        c.key, c.ticket_type, target.key, target.ticket_type
                    )));
                }
            }
            for c in &plan.children {
                set_parent(&mut tx, c, Some((target.id, &target.key)), &x.key, actor).await?;
                touched.insert(c.id);
            }
            touched.insert(target.id);
        }
        Some("promote") => {
            let grandparent = plan.parent.as_ref().expect("promote is only offered with a parent");
            for c in &plan.children {
                make_task(&mut tx, c, actor).await?;
                set_parent(&mut tx, c, Some((grandparent.id, &grandparent.key)), &x.key, actor).await?;
                touched.insert(c.id);
            }
        }
        _ => {}
    }

    // ---- tickets waiting on this one
    let dependents: Vec<&TicketSummary> =
        plan.dependents.iter().map(|d| &d.ticket).filter(|d| !doomed.contains(&d.id)).collect();
    let new_blockers: Vec<i64> = match dependents_choice {
        Some("bridge") => plan.blockers.iter().map(|b| b.id).filter(|b| !doomed.contains(b)).collect(),
        Some("children") => {
            if children_choice == Some("delete") {
                return Err(AppError::invalid(
                    "dependents=children needs the children to survive; choose another children option",
                ));
            }
            plan.children.iter().map(|c| c.id).collect()
        }
        Some("transfer") => {
            let target_key = q
                .transfer_to
                .as_deref()
                .ok_or_else(|| AppError::invalid("dependents=transfer needs transfer_to=KEY"))?;
            let target = resolve_key(&mut tx, target_key).await?;
            if doomed.contains(&target) {
                return Err(AppError::invalid("transfer_to must be a ticket that isn't being deleted"));
            }
            vec![target]
        }
        _ => Vec::new(),
    };
    if !new_blockers.is_empty() {
        let mut edges: Vec<(i64, i64)> = blocks_edges(&mut tx)
            .await?
            .into_iter()
            .filter(|(s, t)| !doomed.contains(s) && !doomed.contains(t))
            .collect();
        let limit = max_height(&mut tx).await?;
        let mut added = Vec::new();
        for &source in &new_blockers {
            for dependent in &dependents {
                let edge = (source, dependent.id);
                if source == dependent.id || edges.contains(&edge) {
                    continue;
                }
                let graph = Graph::new(&edges);
                if let Some(cycle) = graph.cycle_if_added(source, dependent.id) {
                    let keys = keys_for(&mut tx, &cycle).await?;
                    return Err(AppError::conflict(
                        "cycle",
                        format!(
                            "Nothing was deleted: that would create a dependency cycle: {}",
                            keys.join(" → ")
                        ),
                    )
                    .with_detail(json!({ "chain": keys })));
                }
                if let Some(limit) = limit {
                    let chain = graph.longest_through(source, dependent.id);
                    if chain.len() > limit {
                        let keys = keys_for(&mut tx, &chain).await?;
                        return Err(AppError::conflict(
                            "height_exceeded",
                            format!(
                                "Nothing was deleted: that would make a dependency chain of {} tickets, over the limit of {limit}: {}",
                                chain.len(),
                                keys.join(" → ")
                            ),
                        )
                        .with_detail(json!({ "chain": keys, "limit": limit })));
                    }
                }
                edges.push(edge);
                added.push(edge);
            }
        }
        let now = now_ms();
        for (source, target) in added {
            sqlx::query("INSERT INTO ticket_links (source_id, target_id, kind, created_at) VALUES (?, ?, 'blocks', ?)")
                .bind(source)
                .bind(target)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            let keys = keys_for(&mut tx, &[source, target]).await?;
            let why = format!(" (instead of {}, deleted)", x.key);
            log_activity(
                &mut tx,
                source,
                actor,
                "linked",
                Some("blocks"),
                None,
                Some(format!("{}{why}", keys[1])),
            )
            .await?;
            log_activity(
                &mut tx,
                target,
                actor,
                "linked",
                Some("is blocked by"),
                None,
                Some(format!("{}{why}", keys[0])),
            )
            .await?;
            touched.extend([source, target]);
        }
    }

    // ---- record what the cascade will remove on every surviving ticket
    let mut doomed_keys = HashMap::new();
    for d in &doomed {
        doomed_keys.insert(*d, keys_for(&mut tx, &[*d]).await?.remove(0));
    }
    if let Some(parent) = &plan.parent {
        log_activity(
            &mut tx,
            parent.id,
            actor,
            "child_deleted",
            None,
            Some(format!("{}: {}", x.key, x.title)),
            None,
        )
        .await?;
        touched.insert(parent.id);
    }
    for (&gone, gone_key) in &doomed_keys {
        let links: Vec<(String, i64, bool)> = sqlx::query_as(
            "SELECT kind, target_id, 1 FROM ticket_links WHERE source_id = ?
             UNION ALL
             SELECT kind, source_id, 0 FROM ticket_links WHERE target_id = ?",
        )
        .bind(gone)
        .bind(gone)
        .fetch_all(&mut *tx)
        .await?;
        for (kind, other, gone_is_source) in links {
            if doomed.contains(&other) {
                continue;
            }
            let label = link_label(&kind, !gone_is_source);
            log_activity(
                &mut tx,
                other,
                actor,
                "unlinked",
                Some(label),
                Some(format!("{gone_key} (deleted)")),
                None,
            )
            .await?;
            touched.insert(other);
        }
    }

    let now = now_ms();
    let mut keys: Vec<String> = doomed_keys.values().cloned().collect();
    for t in touched.iter().filter(|t| !doomed.contains(t)) {
        sqlx::query("UPDATE tickets SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(t)
            .execute(&mut *tx)
            .await?;
        keys.extend(keys_for(&mut tx, &[*t]).await?);
    }
    for d in &doomed {
        sqlx::query("DELETE FROM ticket_fts WHERE rowid = ?").bind(d).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM tickets WHERE id = ?").bind(d).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    state.events.emit("ticket.deleted", Some(x.project_id), &keys);
    Ok(StatusCode::NO_CONTENT)
}
