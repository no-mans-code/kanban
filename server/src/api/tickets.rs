use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderName, StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteConnection};

use super::Actor;
use crate::error::{ApiResult, AppError};
use crate::models::{
    Activity, PRIORITIES, SUMMARY_SELECT, TYPES, TicketSummary, check_one_of, clean_title, double_option,
    log_activity, project_id_by_key, reindex, resolve_key, summaries, summary, ticket_key, user_name,
    valid_parent, watch,
};
use crate::{AppState, now_ms, write_tx};

/// Gap between neighbouring ranks when a column is (re)numbered.
const RANK_STEP: f64 = 1024.0;

// ---------------------------------------------------------------- list

#[derive(Deserialize)]
pub struct ListQuery {
    project: Option<String>,
    status_id: Option<i64>,
    /// A user id, or "none" for unassigned.
    assignee: Option<String>,
    #[serde(rename = "type")]
    ticket_type: Option<String>,
    priority: Option<String>,
    label_id: Option<i64>,
    parent: Option<String>,
    watcher: Option<i64>,
    q: Option<String>,
    /// rank (default) | updated | created | priority | key
    sort: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// Page size cap. Callers page with `offset` and read `X-Total-Count`.
pub const MAX_PAGE: i64 = 5000;

/// Turn free text into a safe FTS5 prefix query. Non-word characters are
/// dropped, which matches how the unicode61 tokenizer splits text anyway and
/// means user input can never inject FTS5 syntax.
fn fts_query(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// The WHERE clause shared by the page query and the count query.
fn push_filters(qb: &mut QueryBuilder<Sqlite>, q: &ListQuery, parent_id: Option<i64>) -> ApiResult<()> {
    qb.push(" WHERE 1 = 1");
    if let Some(project) = &q.project {
        qb.push(" AND p.key = ").push_bind(project.to_ascii_uppercase());
    }
    if let Some(status) = q.status_id {
        qb.push(" AND t.status_id = ").push_bind(status);
    }
    match q.assignee.as_deref() {
        None | Some("") => {}
        Some("none") => {
            qb.push(" AND t.assignee_id IS NULL");
        }
        Some(id) => {
            let id: i64 =
                id.parse().map_err(|_| AppError::invalid("assignee must be a user id or 'none'"))?;
            qb.push(" AND t.assignee_id = ").push_bind(id);
        }
    }
    if let Some(t) = &q.ticket_type {
        qb.push(" AND t.type = ").push_bind(t.clone());
    }
    if let Some(p) = &q.priority {
        qb.push(" AND t.priority = ").push_bind(p.clone());
    }
    if let Some(label) = q.label_id {
        qb.push(" AND EXISTS (SELECT 1 FROM ticket_labels x WHERE x.ticket_id = t.id AND x.label_id = ")
            .push_bind(label)
            .push(")");
    }
    if let Some(parent) = parent_id {
        qb.push(" AND t.parent_id = ").push_bind(parent);
    }
    if let Some(user) = q.watcher {
        qb.push(" AND EXISTS (SELECT 1 FROM ticket_watchers w WHERE w.ticket_id = t.id AND w.user_id = ")
            .push_bind(user)
            .push(")");
    }
    if let Some(text) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        qb.push(" AND ((p.key || '-' || t.number) = ").push_bind(text.to_ascii_uppercase());
        if let Some(fts) = fts_query(text) {
            qb.push(" OR t.id IN (SELECT rowid FROM ticket_fts WHERE ticket_fts MATCH ")
                .push_bind(fts)
                .push(")");
        }
        qb.push(")");
    }
    Ok(())
}

/// One page of matching tickets. `X-Total-Count` carries the number of
/// matches across all pages, so a client can tell it has everything.
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> ApiResult<([(HeaderName, String); 1], Json<Vec<TicketSummary>>)> {
    // Every order ends in a unique column so pages never overlap or skip.
    let order = match q.sort.as_deref().unwrap_or("rank") {
        "rank" => " ORDER BY s.position, t.rank, t.id",
        "updated" => " ORDER BY t.updated_at DESC, t.id DESC",
        "created" => " ORDER BY t.created_at DESC, t.id DESC",
        "key" => " ORDER BY p.key, t.number",
        "priority" => {
            " ORDER BY CASE t.priority WHEN 'highest' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2
                                       WHEN 'low' THEN 3 ELSE 4 END, t.updated_at DESC, t.id DESC"
        }
        _ => return Err(AppError::invalid("sort must be rank, updated, created, priority or key")),
    };
    let mut conn = state.db.acquire().await?;
    let parent_id = match &q.parent {
        Some(k) => Some(resolve_key(&mut conn, k).await?),
        None => None,
    };

    let mut count = QueryBuilder::<Sqlite>::new(
        "SELECT count(*) FROM tickets t
           JOIN projects p ON p.id = t.project_id
           JOIN statuses s ON s.id = t.status_id",
    );
    push_filters(&mut count, &q, parent_id)?;
    let total: i64 = count.build_query_scalar().fetch_one(&mut *conn).await?;

    let mut qb = QueryBuilder::<Sqlite>::new(SUMMARY_SELECT);
    push_filters(&mut qb, &q, parent_id)?;
    qb.push(order);
    qb.push(" LIMIT ").push_bind(q.limit.unwrap_or(2000).clamp(1, MAX_PAGE));
    qb.push(" OFFSET ").push_bind(q.offset.unwrap_or(0).max(0));
    let rows = qb.build_query_as::<TicketSummary>().fetch_all(&mut *conn).await?;
    Ok(([(HeaderName::from_static("x-total-count"), total.to_string())], Json(rows)))
}

// ---------------------------------------------------------------- detail

#[derive(Serialize)]
pub struct LinkView {
    kind: String,
    /// "outward": this ticket is the link's source.
    direction: &'static str,
    label: &'static str,
    ticket: TicketSummary,
}

#[derive(Serialize)]
pub struct TicketDetail {
    #[serde(flatten)]
    pub summary: TicketSummary,
    pub description: String,
    pub watcher_ids: Vec<i64>,
    pub children: Vec<TicketSummary>,
    pub links: Vec<LinkView>,
}

pub(crate) fn link_label(kind: &str, outward: bool) -> &'static str {
    match (kind, outward) {
        ("blocks", true) => "blocks",
        ("blocks", false) => "is blocked by",
        ("duplicates", true) => "duplicates",
        ("duplicates", false) => "is duplicated by",
        ("clones", true) => "clones",
        ("clones", false) => "is cloned by",
        _ => "relates to",
    }
}

pub async fn load_detail(conn: &mut SqliteConnection, id: i64) -> ApiResult<TicketDetail> {
    let summary = summary(conn, id).await?;
    let description: String = sqlx::query_scalar("SELECT description FROM tickets WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    let watcher_ids: Vec<i64> =
        sqlx::query_scalar("SELECT user_id FROM ticket_watchers WHERE ticket_id = ? ORDER BY user_id")
            .bind(id)
            .fetch_all(&mut *conn)
            .await?;
    let mut qb = QueryBuilder::<Sqlite>::new(SUMMARY_SELECT);
    qb.push(" WHERE t.parent_id = ").push_bind(id).push(" ORDER BY t.number");
    let children = qb.build_query_as::<TicketSummary>().fetch_all(&mut *conn).await?;

    let raw: Vec<(String, i64, bool)> = sqlx::query_as(
        "SELECT kind, target_id, 1 FROM ticket_links WHERE source_id = ?
         UNION ALL
         SELECT kind, source_id, 0 FROM ticket_links WHERE target_id = ?",
    )
    .bind(id)
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let others: Vec<i64> = raw.iter().map(|(_, other, _)| *other).collect();
    let found = summaries(conn, &others).await?;
    let mut links: Vec<LinkView> = raw
        .into_iter()
        .filter_map(|(kind, other, outward)| {
            let ticket = found.get(&other)?.clone();
            let label = link_label(&kind, outward);
            Some(LinkView { kind, direction: if outward { "outward" } else { "inward" }, label, ticket })
        })
        .collect();
    links.sort_by(|a, b| (a.label, &a.ticket.key).cmp(&(b.label, &b.ticket.key)));
    Ok(TicketDetail { summary, description, watcher_ids, children, links })
}

pub async fn get(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<Json<TicketDetail>> {
    let mut conn = state.db.acquire().await?;
    let id = resolve_key(&mut conn, &key).await?;
    Ok(Json(load_detail(&mut conn, id).await?))
}

// ---------------------------------------------------------------- shared helpers

#[derive(FromRow)]
struct TicketRow {
    id: i64,
    project_id: i64,
    ticket_type: String,
    title: String,
    description: String,
    status_id: i64,
    priority: String,
    assignee_id: Option<i64>,
    parent_id: Option<i64>,
}

async fn load_row(conn: &mut SqliteConnection, id: i64) -> ApiResult<TicketRow> {
    Ok(sqlx::query_as::<_, TicketRow>(
        "SELECT id, project_id, type AS ticket_type, title, description, status_id, priority,
                assignee_id, parent_id
           FROM tickets WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await?)
}

struct ParentInfo {
    id: i64,
    project_id: i64,
    ticket_type: String,
}

async fn parent_by_id(conn: &mut SqliteConnection, id: i64) -> ApiResult<ParentInfo> {
    let (project_id, ticket_type): (i64, String) =
        sqlx::query_as("SELECT project_id, type FROM tickets WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
    Ok(ParentInfo { id, project_id, ticket_type })
}

fn check_hierarchy(parent: Option<&ParentInfo>, child_type: &str, project_id: i64) -> ApiResult<()> {
    match parent {
        None if child_type == "subtask" => Err(AppError::invalid("A subtask needs a parent ticket")),
        None => Ok(()),
        Some(p) if p.project_id != project_id => {
            Err(AppError::invalid("A parent ticket must be in the same project"))
        }
        Some(p) if !valid_parent(&p.ticket_type, child_type) => Err(AppError::invalid(format!(
            "A {child_type} cannot be a child of a {}. Epics hold stories, tasks and bugs; \
             those hold subtasks.",
            p.ticket_type
        ))),
        Some(_) => Ok(()),
    }
}

/// Returns (name, category) of a status, checking it belongs to the project.
async fn status_in_project(
    conn: &mut SqliteConnection,
    status_id: i64,
    project_id: i64,
) -> ApiResult<(String, String)> {
    sqlx::query_as("SELECT name, category FROM statuses WHERE id = ? AND project_id = ?")
        .bind(status_id)
        .bind(project_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::invalid("That status does not belong to this ticket's project"))
}

async fn change_status(
    conn: &mut SqliteConnection,
    row: &TicketRow,
    new_status: i64,
    actor: Option<i64>,
) -> ApiResult<()> {
    let (new_name, category) = status_in_project(conn, new_status, row.project_id).await?;
    let (old_name, _) = status_in_project(conn, row.status_id, row.project_id).await?;
    let resolved = (category == "done").then(now_ms);
    sqlx::query(
        "UPDATE tickets SET status_id = ?,
                resolved_at = CASE WHEN ? IS NULL THEN NULL ELSE COALESCE(resolved_at, ?) END
          WHERE id = ?",
    )
    .bind(new_status)
    .bind(resolved)
    .bind(resolved)
    .bind(row.id)
    .execute(&mut *conn)
    .await?;
    log_activity(conn, row.id, actor, "updated", Some("status"), Some(old_name), Some(new_name)).await
}

async fn bottom_rank(conn: &mut SqliteConnection, project_id: i64, status_id: i64) -> ApiResult<f64> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(MAX(rank), 0.0) + ? FROM tickets WHERE project_id = ? AND status_id = ?",
    )
    .bind(RANK_STEP)
    .bind(project_id)
    .bind(status_id)
    .fetch_one(&mut *conn)
    .await?)
}

async fn label_names(conn: &mut SqliteConnection, ids: &BTreeSet<i64>) -> ApiResult<String> {
    let mut names = Vec::with_capacity(ids.len());
    for id in ids {
        let name: String = sqlx::query_scalar("SELECT name FROM labels WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| AppError::invalid(format!("Label {id} does not exist")))?;
        names.push(name);
    }
    names.sort();
    Ok(names.join(", "))
}

async fn set_labels(conn: &mut SqliteConnection, ticket_id: i64, ids: &BTreeSet<i64>) -> ApiResult<()> {
    sqlx::query("DELETE FROM ticket_labels WHERE ticket_id = ?").bind(ticket_id).execute(&mut *conn).await?;
    for id in ids {
        sqlx::query("INSERT INTO ticket_labels (ticket_id, label_id) VALUES (?, ?)")
            .bind(ticket_id)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------- create

fn default_type() -> String {
    "task".into()
}

fn default_priority() -> String {
    "medium".into()
}

#[derive(Deserialize)]
pub struct CreateTicket {
    project: String,
    #[serde(rename = "type", default = "default_type")]
    ticket_type: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_priority")]
    priority: String,
    status_id: Option<i64>,
    assignee_id: Option<i64>,
    parent: Option<String>,
    #[serde(default)]
    label_ids: Vec<i64>,
    #[serde(default)]
    watcher_ids: Vec<i64>,
}

pub async fn create(
    State(state): State<AppState>,
    Actor(actor): Actor,
    Json(body): Json<CreateTicket>,
) -> ApiResult<(StatusCode, Json<TicketDetail>)> {
    let title = clean_title(&body.title)?;
    check_one_of("type", &body.ticket_type, &TYPES)?;
    check_one_of("priority", &body.priority, &PRIORITIES)?;

    let mut tx = write_tx(&state.db).await?;
    let project_id = project_id_by_key(&mut tx, &body.project).await?;
    let status_id = match body.status_id {
        Some(s) => s,
        None => {
            sqlx::query_scalar("SELECT id FROM statuses WHERE project_id = ? ORDER BY position, id LIMIT 1")
                .bind(project_id)
                .fetch_one(&mut *tx)
                .await?
        }
    };
    let (_, category) = status_in_project(&mut tx, status_id, project_id).await?;
    let parent = match &body.parent {
        Some(key) => {
            let id = resolve_key(&mut tx, key).await?;
            Some(parent_by_id(&mut tx, id).await?)
        }
        None => None,
    };
    check_hierarchy(parent.as_ref(), &body.ticket_type, project_id)?;
    if body.assignee_id.is_some() {
        user_name(&mut tx, body.assignee_id).await?;
    }

    let number: i64 = sqlx::query_scalar(
        "UPDATE projects SET next_number = next_number + 1 WHERE id = ? RETURNING next_number - 1",
    )
    .bind(project_id)
    .fetch_one(&mut *tx)
    .await?;
    let rank = bottom_rank(&mut tx, project_id, status_id).await?;
    let now = now_ms();
    let id = sqlx::query(
        "INSERT INTO tickets (project_id, number, type, title, description, status_id, priority,
                              assignee_id, reporter_id, parent_id, rank, created_at, updated_at,
                              resolved_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(project_id)
    .bind(number)
    .bind(&body.ticket_type)
    .bind(&title)
    .bind(&body.description)
    .bind(status_id)
    .bind(&body.priority)
    .bind(body.assignee_id)
    .bind(actor)
    .bind(parent.as_ref().map(|p| p.id))
    .bind(rank)
    .bind(now)
    .bind(now)
    .bind((category == "done").then_some(now))
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();

    let labels: BTreeSet<i64> = body.label_ids.iter().copied().collect();
    label_names(&mut tx, &labels).await?;
    set_labels(&mut tx, id, &labels).await?;
    // Like Jira: the reporter and assignee watch automatically.
    for user in [actor, body.assignee_id].into_iter().chain(body.watcher_ids.iter().map(|w| Some(*w))) {
        watch(&mut tx, id, user).await?;
    }
    log_activity(&mut tx, id, actor, "created", None, None, None).await?;
    reindex(&mut tx, id).await?;
    let detail = load_detail(&mut tx, id).await?;
    tx.commit().await?;
    state.events.emit("ticket.created", Some(project_id), std::slice::from_ref(&detail.summary.key));
    Ok((StatusCode::CREATED, Json(detail)))
}

// ---------------------------------------------------------------- update

#[derive(Deserialize)]
pub struct UpdateTicket {
    title: Option<String>,
    description: Option<String>,
    #[serde(rename = "type")]
    ticket_type: Option<String>,
    priority: Option<String>,
    status_id: Option<i64>,
    #[serde(default, deserialize_with = "double_option")]
    assignee_id: Option<Option<i64>>,
    /// A ticket key, or null to detach from the parent.
    #[serde(default, deserialize_with = "double_option")]
    parent: Option<Option<String>>,
    label_ids: Option<Vec<i64>>,
}

pub async fn update(
    State(state): State<AppState>,
    Actor(actor): Actor,
    Path(key): Path<String>,
    Json(body): Json<UpdateTicket>,
) -> ApiResult<Json<TicketDetail>> {
    let mut tx = write_tx(&state.db).await?;
    let id = resolve_key(&mut tx, &key).await?;
    let row = load_row(&mut tx, id).await?;
    let mut changed = false;
    let mut search_dirty = false;

    if let Some(title) = &body.title {
        let title = clean_title(title)?;
        if title != row.title {
            sqlx::query("UPDATE tickets SET title = ? WHERE id = ?")
                .bind(&title)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            log_activity(&mut tx, id, actor, "updated", Some("title"), Some(row.title.clone()), Some(title))
                .await?;
            changed = true;
            search_dirty = true;
        }
    }
    if let Some(description) = &body.description
        && *description != row.description
    {
        sqlx::query("UPDATE tickets SET description = ? WHERE id = ?")
            .bind(description)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_activity(
            &mut tx,
            id,
            actor,
            "updated",
            Some("description"),
            Some(row.description.clone()),
            Some(description.clone()),
        )
        .await?;
        changed = true;
        search_dirty = true;
    }
    if let Some(priority) = &body.priority {
        check_one_of("priority", priority, &PRIORITIES)?;
        if *priority != row.priority {
            sqlx::query("UPDATE tickets SET priority = ? WHERE id = ?")
                .bind(priority)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            log_activity(
                &mut tx,
                id,
                actor,
                "updated",
                Some("priority"),
                Some(row.priority.clone()),
                Some(priority.clone()),
            )
            .await?;
            changed = true;
        }
    }

    // Type and parent are validated together: each constrains the other.
    let new_type = body.ticket_type.clone().unwrap_or_else(|| row.ticket_type.clone());
    check_one_of("type", &new_type, &TYPES)?;
    let new_parent = match &body.parent {
        None => match row.parent_id {
            Some(p) => Some(parent_by_id(&mut tx, p).await?),
            None => None,
        },
        Some(None) => None,
        Some(Some(k)) => {
            let pid = resolve_key(&mut tx, k).await?;
            Some(parent_by_id(&mut tx, pid).await?)
        }
    };
    let type_changed = new_type != row.ticket_type;
    let parent_changed = new_parent.as_ref().map(|p| p.id) != row.parent_id;
    if type_changed || parent_changed {
        check_hierarchy(new_parent.as_ref(), &new_type, row.project_id)?;
    }
    if type_changed {
        let child_types: Vec<String> = sqlx::query_scalar("SELECT type FROM tickets WHERE parent_id = ?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
        if let Some(bad) = child_types.iter().find(|c| !valid_parent(&new_type, c)) {
            return Err(AppError::invalid(format!("Cannot make this a {new_type}: it has a {bad} under it")));
        }
        sqlx::query("UPDATE tickets SET type = ? WHERE id = ?")
            .bind(&new_type)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_activity(
            &mut tx,
            id,
            actor,
            "updated",
            Some("type"),
            Some(row.ticket_type.clone()),
            Some(new_type.clone()),
        )
        .await?;
        changed = true;
    }
    if parent_changed {
        let old = match row.parent_id {
            Some(p) => ticket_key(&mut tx, p).await?,
            None => "None".into(),
        };
        let new = match &new_parent {
            Some(p) => ticket_key(&mut tx, p.id).await?,
            None => "None".into(),
        };
        sqlx::query("UPDATE tickets SET parent_id = ? WHERE id = ?")
            .bind(new_parent.as_ref().map(|p| p.id))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_activity(&mut tx, id, actor, "updated", Some("parent"), Some(old), Some(new)).await?;
        changed = true;
    }

    if let Some(status) = body.status_id
        && status != row.status_id
    {
        change_status(&mut tx, &row, status, actor).await?;
        let rank = bottom_rank(&mut tx, row.project_id, status).await?;
        sqlx::query("UPDATE tickets SET rank = ? WHERE id = ?").bind(rank).bind(id).execute(&mut *tx).await?;
        changed = true;
    }
    if let Some(assignee) = body.assignee_id
        && assignee != row.assignee_id
    {
        let old = user_name(&mut tx, row.assignee_id).await?;
        let new = user_name(&mut tx, assignee).await?;
        sqlx::query("UPDATE tickets SET assignee_id = ? WHERE id = ?")
            .bind(assignee)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_activity(&mut tx, id, actor, "updated", Some("assignee"), Some(old), Some(new)).await?;
        watch(&mut tx, id, assignee).await?;
        changed = true;
    }
    if let Some(label_ids) = &body.label_ids {
        let new: BTreeSet<i64> = label_ids.iter().copied().collect();
        let old: BTreeSet<i64> =
            sqlx::query_scalar::<_, i64>("SELECT label_id FROM ticket_labels WHERE ticket_id = ?")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .collect();
        if new != old {
            let old_names = label_names(&mut tx, &old).await?;
            let new_names = label_names(&mut tx, &new).await?;
            set_labels(&mut tx, id, &new).await?;
            log_activity(&mut tx, id, actor, "updated", Some("labels"), Some(old_names), Some(new_names))
                .await?;
            changed = true;
        }
    }

    if changed {
        sqlx::query("UPDATE tickets SET updated_at = ? WHERE id = ?")
            .bind(now_ms())
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if search_dirty {
        reindex(&mut tx, id).await?;
    }
    let detail = load_detail(&mut tx, id).await?;
    tx.commit().await?;
    if changed {
        state.events.emit("ticket.updated", Some(row.project_id), std::slice::from_ref(&detail.summary.key));
    }
    Ok(Json(detail))
}

// ---------------------------------------------------------------- move (drag and drop)

#[derive(Deserialize)]
pub struct MoveTicket {
    status_id: i64,
    /// Place directly after this ticket; null places it at the top.
    after: Option<String>,
}

/// Rank for `me` placed after `after` in a column. Ranks are floats so a
/// move touches one row; when the gap between neighbours runs out, the
/// column is renumbered once and the placement retried.
async fn place(
    conn: &mut SqliteConnection,
    project_id: i64,
    status_id: i64,
    me: i64,
    after: Option<i64>,
) -> ApiResult<f64> {
    for _ in 0..2 {
        let lower: Option<f64> = match after {
            Some(a) => Some(
                sqlx::query_scalar("SELECT rank FROM tickets WHERE id = ?")
                    .bind(a)
                    .fetch_one(&mut *conn)
                    .await?,
            ),
            None => None,
        };
        let upper: Option<f64> = match lower {
            Some(l) => sqlx::query_scalar(
                "SELECT MIN(rank) FROM tickets WHERE project_id = ? AND status_id = ? AND id <> ? AND rank > ?",
            )
            .bind(project_id)
            .bind(status_id)
            .bind(me)
            .bind(l)
            .fetch_one(&mut *conn)
            .await?,
            None => sqlx::query_scalar(
                "SELECT MIN(rank) FROM tickets WHERE project_id = ? AND status_id = ? AND id <> ?",
            )
            .bind(project_id)
            .bind(status_id)
            .bind(me)
            .fetch_one(&mut *conn)
            .await?,
        };
        match (lower, upper) {
            (None, None) => return Ok(RANK_STEP),
            (Some(l), None) => return Ok(l + RANK_STEP),
            (None, Some(u)) => return Ok(u - RANK_STEP),
            (Some(l), Some(u)) if u - l > 1e-6 => return Ok((l + u) / 2.0),
            _ => {
                let ids: Vec<i64> = sqlx::query_scalar(
                    "SELECT id FROM tickets WHERE project_id = ? AND status_id = ? AND id <> ? ORDER BY rank, id",
                )
                .bind(project_id)
                .bind(status_id)
                .bind(me)
                .fetch_all(&mut *conn)
                .await?;
                for (i, id) in ids.iter().enumerate() {
                    sqlx::query("UPDATE tickets SET rank = ? WHERE id = ?")
                        .bind((i as f64 + 1.0) * RANK_STEP)
                        .bind(id)
                        .execute(&mut *conn)
                        .await?;
                }
            }
        }
    }
    Err(AppError::conflict("rank", "Could not place the ticket; try again"))
}

pub async fn move_ticket(
    State(state): State<AppState>,
    Actor(actor): Actor,
    Path(key): Path<String>,
    Json(body): Json<MoveTicket>,
) -> ApiResult<Json<TicketSummary>> {
    let mut tx = write_tx(&state.db).await?;
    let id = resolve_key(&mut tx, &key).await?;
    let row = load_row(&mut tx, id).await?;
    let after = match &body.after {
        Some(k) => {
            let a = resolve_key(&mut tx, k).await?;
            let (project, status): (i64, i64) =
                sqlx::query_as("SELECT project_id, status_id FROM tickets WHERE id = ?")
                    .bind(a)
                    .fetch_one(&mut *tx)
                    .await?;
            if a == id || project != row.project_id || status != body.status_id {
                return Err(AppError::invalid("`after` must be another ticket in the target column"));
            }
            Some(a)
        }
        None => None,
    };
    let status_changed = body.status_id != row.status_id;
    if status_changed {
        change_status(&mut tx, &row, body.status_id, actor).await?;
    } else {
        status_in_project(&mut tx, body.status_id, row.project_id).await?;
    }
    let rank = place(&mut tx, row.project_id, body.status_id, id, after).await?;
    sqlx::query("UPDATE tickets SET rank = ? WHERE id = ?").bind(rank).bind(id).execute(&mut *tx).await?;
    if status_changed {
        sqlx::query("UPDATE tickets SET updated_at = ? WHERE id = ?")
            .bind(now_ms())
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let moved = summary(&mut tx, id).await?;
    tx.commit().await?;
    state.events.emit("ticket.moved", Some(row.project_id), std::slice::from_ref(&moved.key));
    Ok(Json(moved))
}

// ---------------------------------------------------------------- watchers + history

#[derive(Deserialize)]
pub struct WatcherBody {
    user_id: i64,
}

async fn watcher_ids(conn: &mut SqliteConnection, ticket_id: i64) -> ApiResult<Vec<i64>> {
    Ok(sqlx::query_scalar("SELECT user_id FROM ticket_watchers WHERE ticket_id = ? ORDER BY user_id")
        .bind(ticket_id)
        .fetch_all(&mut *conn)
        .await?)
}

pub async fn add_watcher(
    State(state): State<AppState>,
    Actor(actor): Actor,
    Path(key): Path<String>,
    Json(body): Json<WatcherBody>,
) -> ApiResult<Json<Vec<i64>>> {
    let mut tx = write_tx(&state.db).await?;
    let id = resolve_key(&mut tx, &key).await?;
    let name = user_name(&mut tx, Some(body.user_id)).await?;
    let added = sqlx::query("INSERT OR IGNORE INTO ticket_watchers (ticket_id, user_id) VALUES (?, ?)")
        .bind(id)
        .bind(body.user_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if added > 0 {
        log_activity(&mut tx, id, actor, "watcher_added", None, None, Some(name)).await?;
    }
    let project_id: i64 = sqlx::query_scalar("SELECT project_id FROM tickets WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let ids = watcher_ids(&mut tx, id).await?;
    tx.commit().await?;
    state.events.emit("ticket.updated", Some(project_id), &[key.to_ascii_uppercase()]);
    Ok(Json(ids))
}

pub async fn remove_watcher(
    State(state): State<AppState>,
    Actor(actor): Actor,
    Path((key, user_id)): Path<(String, i64)>,
) -> ApiResult<Json<Vec<i64>>> {
    let mut tx = write_tx(&state.db).await?;
    let id = resolve_key(&mut tx, &key).await?;
    let removed = sqlx::query("DELETE FROM ticket_watchers WHERE ticket_id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if removed > 0 {
        let name = user_name(&mut tx, Some(user_id)).await?;
        log_activity(&mut tx, id, actor, "watcher_removed", None, Some(name), None).await?;
    }
    let project_id: i64 = sqlx::query_scalar("SELECT project_id FROM tickets WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let ids = watcher_ids(&mut tx, id).await?;
    tx.commit().await?;
    state.events.emit("ticket.updated", Some(project_id), &[key.to_ascii_uppercase()]);
    Ok(Json(ids))
}

pub async fn activity(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> ApiResult<Json<Vec<Activity>>> {
    let mut conn = state.db.acquire().await?;
    let id = resolve_key(&mut conn, &key).await?;
    let rows = sqlx::query_as::<_, Activity>(
        "SELECT id, ticket_id, actor_id, action, field, old_value, new_value, created_at
           FROM activity WHERE ticket_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Json(rows))
}
