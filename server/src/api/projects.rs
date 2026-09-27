use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::auth::{Auth, Principal, Role};
use crate::error::{ApiResult, AppError};
use crate::models::{CATEGORIES, Project, Status, check_one_of, log_activity, project_id_by_key};
use crate::{AppState, now_ms, write_tx};

const DEFAULT_STATUSES: [(&str, &str); 4] =
    [("To Do", "todo"), ("In Progress", "in_progress"), ("In Review", "in_progress"), ("Done", "done")];

#[derive(Serialize)]
pub struct ProjectDetail {
    #[serde(flatten)]
    project: Project,
    statuses: Vec<Status>,
    /// The caller's role here: viewer | member | admin.
    role: &'static str,
}

#[derive(Serialize)]
pub struct ProjectItem {
    #[serde(flatten)]
    project: Project,
    role: &'static str,
}

async fn load_statuses(conn: &mut SqliteConnection, project_id: i64) -> ApiResult<Vec<Status>> {
    Ok(sqlx::query_as::<_, Status>(
        "SELECT id, project_id, name, category, position FROM statuses
          WHERE project_id = ? ORDER BY position, id",
    )
    .bind(project_id)
    .fetch_all(&mut *conn)
    .await?)
}

/// Caller must already be known to have `role` here (checked by the caller).
async fn load_detail(conn: &mut SqliteConnection, project_id: i64, role: Role) -> ApiResult<ProjectDetail> {
    let project = sqlx::query_as::<_, Project>(
        "SELECT id, key, name, description, created_at FROM projects WHERE id = ?",
    )
    .bind(project_id)
    .fetch_one(&mut *conn)
    .await?;
    let statuses = load_statuses(conn, project_id).await?;
    Ok(ProjectDetail { project, statuses, role: role.as_str() })
}

/// Only the projects the caller can see, each with the caller's role there.
/// A project nobody is a member of, and that isn't the caller's, simply
/// doesn't appear — that's what keeps agents off work that isn't theirs.
pub async fn list(State(state): State<AppState>, Auth(p): Auth) -> ApiResult<Json<Vec<ProjectItem>>> {
    let mut conn = state.db.acquire().await?;
    let projects = sqlx::query_as::<_, Project>(
        "SELECT id, key, name, description, created_at FROM projects ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::new();
    for project in projects {
        if let Some(role) = p.role_in(&mut conn, project.id).await? {
            out.push(ProjectItem { project, role: role.as_str() });
        }
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct CreateProject {
    key: String,
    name: String,
    #[serde(default)]
    description: String,
}

fn clean_key(key: &str) -> ApiResult<String> {
    let k = key.trim().to_ascii_uppercase();
    let mut chars = k.chars();
    let ok = (2..=10).contains(&k.len())
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if !ok {
        return Err(AppError::invalid(
            "Project key must be 2-10 characters: a letter, then letters or digits",
        ));
    }
    Ok(k)
}

fn clean_name(name: &str, what: &str) -> ApiResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 80 {
        return Err(AppError::invalid(format!("{what} must be 1-80 characters")));
    }
    Ok(n.to_string())
}

/// Creating a project is deliberately site-admin-only: it keeps the roster
/// of projects something the master/manager curates, not something any
/// agent can spawn on a whim.
pub async fn create(
    State(state): State<AppState>,
    Auth(p): Auth,
    Json(body): Json<CreateProject>,
) -> ApiResult<(StatusCode, Json<ProjectDetail>)> {
    p.require_site_admin()?;
    let key = clean_key(&body.key)?;
    let name = clean_name(&body.name, "Project name")?;
    let mut tx = write_tx(&state.db).await?;
    let id = sqlx::query("INSERT INTO projects (key, name, description, created_at) VALUES (?, ?, ?, ?)")
        .bind(&key)
        .bind(&name)
        .bind(body.description.trim())
        .bind(now_ms())
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    for (pos, (status, category)) in DEFAULT_STATUSES.iter().enumerate() {
        sqlx::query("INSERT INTO statuses (project_id, name, category, position) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(status)
            .bind(category)
            .bind(pos as i64)
            .execute(&mut *tx)
            .await?;
    }
    // Whoever creates a project administers it, so it isn't orphaned the
    // moment the site admin's own token is narrowed to something else.
    if let Some(user) = p.user_id {
        sqlx::query("INSERT INTO project_members (project_id, user_id, role) VALUES (?, ?, 'admin')")
            .bind(id)
            .bind(user)
            .execute(&mut *tx)
            .await?;
    }
    let detail = load_detail(&mut tx, id, Role::Admin).await?;
    tx.commit().await?;
    state.events.emit("project.changed", Some(id), &[]);
    Ok((StatusCode::CREATED, Json(detail)))
}

pub async fn get(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(key): Path<String>,
) -> ApiResult<Json<ProjectDetail>> {
    let mut conn = state.db.acquire().await?;
    let id = project_id_by_key(&mut conn, &key).await?;
    let role = p.require(&mut conn, id, Role::Viewer).await?;
    Ok(Json(load_detail(&mut conn, id, role).await?))
}

#[derive(Deserialize)]
pub struct UpdateProject {
    name: Option<String>,
    description: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(key): Path<String>,
    Json(body): Json<UpdateProject>,
) -> ApiResult<Json<ProjectDetail>> {
    p.require_writable()?;
    let mut tx = write_tx(&state.db).await?;
    let id = project_id_by_key(&mut tx, &key).await?;
    p.require(&mut tx, id, Role::Admin).await?;
    if let Some(name) = body.name {
        sqlx::query("UPDATE projects SET name = ? WHERE id = ?")
            .bind(clean_name(&name, "Project name")?)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(description) = body.description {
        sqlx::query("UPDATE projects SET description = ? WHERE id = ?")
            .bind(description.trim())
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let detail = load_detail(&mut tx, id, Role::Admin).await?;
    tx.commit().await?;
    state.events.emit("project.changed", Some(id), &[]);
    Ok(Json(detail))
}

pub async fn list_statuses(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(key): Path<String>,
) -> ApiResult<Json<Vec<Status>>> {
    let mut conn = state.db.acquire().await?;
    let id = project_id_by_key(&mut conn, &key).await?;
    p.require(&mut conn, id, Role::Viewer).await?;
    Ok(Json(load_statuses(&mut conn, id).await?))
}

#[derive(Deserialize)]
pub struct CreateStatus {
    name: String,
    category: String,
}

pub async fn create_status(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(key): Path<String>,
    Json(body): Json<CreateStatus>,
) -> ApiResult<(StatusCode, Json<Vec<Status>>)> {
    p.require_writable()?;
    let name = clean_name(&body.name, "Status name")?;
    check_one_of("category", &body.category, &CATEGORIES)?;
    let mut tx = write_tx(&state.db).await?;
    let project_id = project_id_by_key(&mut tx, &key).await?;
    p.require(&mut tx, project_id, Role::Admin).await?;
    sqlx::query(
        "INSERT INTO statuses (project_id, name, category, position)
         VALUES (?, ?, ?, (SELECT COALESCE(MAX(position), -1) + 1 FROM statuses WHERE project_id = ?))",
    )
    .bind(project_id)
    .bind(&name)
    .bind(&body.category)
    .bind(project_id)
    .execute(&mut *tx)
    .await?;
    let statuses = load_statuses(&mut tx, project_id).await?;
    tx.commit().await?;
    state.events.emit("project.changed", Some(project_id), &[]);
    Ok((StatusCode::CREATED, Json(statuses)))
}

/// A status in a project the caller can at least view; otherwise, as far as
/// the caller is concerned, it doesn't exist.
async fn status_row(conn: &mut SqliteConnection, p: &Principal, id: i64) -> ApiResult<Status> {
    let status = sqlx::query_as::<_, Status>(
        "SELECT id, project_id, name, category, position FROM statuses WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| AppError::not_found("Status"))?;
    if p.role_in(conn, status.project_id).await?.is_none() {
        return Err(AppError::not_found("Status"));
    }
    Ok(status)
}

/// Keep `resolved_at` consistent after a status's category changes.
async fn sync_resolved(conn: &mut SqliteConnection, status_id: i64, category: &str) -> ApiResult<()> {
    if category == "done" {
        sqlx::query("UPDATE tickets SET resolved_at = ? WHERE status_id = ? AND resolved_at IS NULL")
            .bind(now_ms())
            .bind(status_id)
            .execute(&mut *conn)
            .await?;
    } else {
        sqlx::query("UPDATE tickets SET resolved_at = NULL WHERE status_id = ?")
            .bind(status_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct UpdateStatus {
    name: Option<String>,
    category: Option<String>,
}

pub async fn update_status(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(id): Path<i64>,
    Json(body): Json<UpdateStatus>,
) -> ApiResult<Json<Vec<Status>>> {
    p.require_writable()?;
    let mut tx = write_tx(&state.db).await?;
    let status = status_row(&mut tx, &p, id).await?;
    p.require(&mut tx, status.project_id, Role::Admin).await?;
    if let Some(name) = body.name {
        sqlx::query("UPDATE statuses SET name = ? WHERE id = ?")
            .bind(clean_name(&name, "Status name")?)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(category) = body.category {
        check_one_of("category", &category, &CATEGORIES)?;
        if category != status.category {
            sqlx::query("UPDATE statuses SET category = ? WHERE id = ?")
                .bind(&category)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sync_resolved(&mut tx, id, &category).await?;
        }
    }
    let statuses = load_statuses(&mut tx, status.project_id).await?;
    tx.commit().await?;
    state.events.emit("project.changed", Some(status.project_id), &[]);
    Ok(Json(statuses))
}

#[derive(Deserialize)]
pub struct DeleteStatusQuery {
    move_to: Option<i64>,
}

pub async fn delete_status(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(id): Path<i64>,
    Query(query): Query<DeleteStatusQuery>,
) -> ApiResult<Json<Vec<Status>>> {
    p.require_writable()?;
    let actor = p.user_id;
    let mut tx = write_tx(&state.db).await?;
    let status = status_row(&mut tx, &p, id).await?;
    p.require(&mut tx, status.project_id, Role::Admin).await?;
    let siblings: i64 = sqlx::query_scalar("SELECT count(*) FROM statuses WHERE project_id = ?")
        .bind(status.project_id)
        .fetch_one(&mut *tx)
        .await?;
    if siblings <= 1 {
        return Err(AppError::conflict("last_status", "A project needs at least one status"));
    }
    let tickets: Vec<i64> = sqlx::query_scalar("SELECT id FROM tickets WHERE status_id = ? ORDER BY rank")
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    if !tickets.is_empty() {
        let Some(target_id) = query.move_to else {
            return Err(AppError::conflict(
                "status_in_use",
                format!("{} tickets use this status; choose a status to move them to", tickets.len()),
            ));
        };
        let target = status_row(&mut tx, &p, target_id).await?;
        if target.project_id != status.project_id || target.id == id {
            return Err(AppError::invalid("Tickets must move to another status in the same project"));
        }
        let now = now_ms();
        let resolved = (target.category == "done").then_some(now);
        let mut rank: f64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(rank), 0.0) FROM tickets WHERE status_id = ?")
                .bind(target.id)
                .fetch_one(&mut *tx)
                .await?;
        for ticket in tickets {
            rank += 1024.0;
            sqlx::query(
                "UPDATE tickets SET status_id = ?, rank = ?, updated_at = ?,
                        resolved_at = CASE WHEN ? IS NULL THEN NULL ELSE COALESCE(resolved_at, ?) END
                  WHERE id = ?",
            )
            .bind(target.id)
            .bind(rank)
            .bind(now)
            .bind(resolved)
            .bind(resolved)
            .bind(ticket)
            .execute(&mut *tx)
            .await?;
            log_activity(
                &mut tx,
                ticket,
                actor,
                "updated",
                Some("status"),
                Some(status.name.clone()),
                Some(target.name.clone()),
            )
            .await?;
        }
    }
    sqlx::query("DELETE FROM statuses WHERE id = ?").bind(id).execute(&mut *tx).await?;
    renumber(&mut tx, status.project_id).await?;
    let statuses = load_statuses(&mut tx, status.project_id).await?;
    tx.commit().await?;
    state.events.emit("project.changed", Some(status.project_id), &[]);
    Ok(Json(statuses))
}

async fn renumber(conn: &mut SqliteConnection, project_id: i64) -> ApiResult<()> {
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM statuses WHERE project_id = ? ORDER BY position, id")
            .bind(project_id)
            .fetch_all(&mut *conn)
            .await?;
    for (pos, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE statuses SET position = ? WHERE id = ?")
            .bind(pos as i64)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct Reorder {
    ids: Vec<i64>,
}

pub async fn reorder_statuses(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(key): Path<String>,
    Json(body): Json<Reorder>,
) -> ApiResult<Json<Vec<Status>>> {
    p.require_writable()?;
    let mut tx = write_tx(&state.db).await?;
    let project_id = project_id_by_key(&mut tx, &key).await?;
    p.require(&mut tx, project_id, Role::Admin).await?;
    let current: HashSet<i64> = load_statuses(&mut tx, project_id).await?.iter().map(|s| s.id).collect();
    let given: HashSet<i64> = body.ids.iter().copied().collect();
    if given != current || given.len() != body.ids.len() {
        return Err(AppError::invalid("ids must list every status of the project exactly once"));
    }
    for (pos, id) in body.ids.iter().enumerate() {
        sqlx::query("UPDATE statuses SET position = ? WHERE id = ?")
            .bind(pos as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let statuses = load_statuses(&mut tx, project_id).await?;
    tx.commit().await?;
    state.events.emit("project.changed", Some(project_id), &[]);
    Ok(Json(statuses))
}

// ---------------------------------------------------------------- members

#[derive(Serialize, sqlx::FromRow)]
pub struct Member {
    user_id: i64,
    username: String,
    display_name: String,
    color: String,
    kind: String,
    role: String,
}

async fn members(conn: &mut SqliteConnection, project_id: i64) -> ApiResult<Vec<Member>> {
    Ok(sqlx::query_as::<_, Member>(
        "SELECT u.id AS user_id, u.username, u.display_name, u.color, u.kind, m.role
           FROM project_members m JOIN users u ON u.id = m.user_id
          WHERE m.project_id = ?
          ORDER BY CASE m.role WHEN 'admin' THEN 0 WHEN 'member' THEN 1 ELSE 2 END, u.display_name",
    )
    .bind(project_id)
    .fetch_all(&mut *conn)
    .await?)
}

pub async fn list_members(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path(key): Path<String>,
) -> ApiResult<Json<Vec<Member>>> {
    let mut conn = state.db.acquire().await?;
    let id = project_id_by_key(&mut conn, &key).await?;
    p.require(&mut conn, id, Role::Viewer).await?;
    Ok(Json(members(&mut conn, id).await?))
}

#[derive(Deserialize)]
pub struct SetMember {
    role: String,
}

/// Add someone to a project, or change their role. Project admins (or a
/// site admin) only — this is exactly the knob that limits which projects
/// an agent's own token can reach.
pub async fn set_member(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path((key, user_id)): Path<(String, i64)>,
    Json(body): Json<SetMember>,
) -> ApiResult<Json<Vec<Member>>> {
    p.require_writable()?;
    let role =
        Role::parse(&body.role).ok_or_else(|| AppError::invalid("role must be viewer, member or admin"))?;
    let mut tx = write_tx(&state.db).await?;
    let id = project_id_by_key(&mut tx, &key).await?;
    p.require(&mut tx, id, Role::Admin).await?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
    exists.ok_or_else(|| AppError::not_found("User"))?;
    if role != Role::Admin {
        last_admin_check(&mut tx, id, user_id).await?;
    }
    sqlx::query(
        "INSERT INTO project_members (project_id, user_id, role) VALUES (?, ?, ?)
         ON CONFLICT (project_id, user_id) DO UPDATE SET role = excluded.role",
    )
    .bind(id)
    .bind(user_id)
    .bind(role.as_str())
    .execute(&mut *tx)
    .await?;
    let list = members(&mut tx, id).await?;
    tx.commit().await?;
    state.events.emit("project.changed", Some(id), &[]);
    Ok(Json(list))
}

pub async fn remove_member(
    State(state): State<AppState>,
    Auth(p): Auth,
    Path((key, user_id)): Path<(String, i64)>,
) -> ApiResult<Json<Vec<Member>>> {
    p.require_writable()?;
    let mut tx = write_tx(&state.db).await?;
    let id = project_id_by_key(&mut tx, &key).await?;
    p.require(&mut tx, id, Role::Admin).await?;
    last_admin_check(&mut tx, id, user_id).await?;
    sqlx::query("DELETE FROM project_members WHERE project_id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let list = members(&mut tx, id).await?;
    tx.commit().await?;
    state.events.emit("project.changed", Some(id), &[]);
    Ok(Json(list))
}

/// A project must keep at least one member admin, or only a site admin
/// could manage it again.
async fn last_admin_check(conn: &mut SqliteConnection, project_id: i64, leaving: i64) -> ApiResult<()> {
    let is_admin: Option<String> = sqlx::query_scalar(
        "SELECT role FROM project_members WHERE project_id = ? AND user_id = ? AND role = 'admin'",
    )
    .bind(project_id)
    .bind(leaving)
    .fetch_optional(&mut *conn)
    .await?;
    if is_admin.is_none() {
        return Ok(());
    }
    let others: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM project_members WHERE project_id = ? AND role = 'admin' AND user_id <> ?",
    )
    .bind(project_id)
    .bind(leaving)
    .fetch_one(&mut *conn)
    .await?;
    if others == 0 {
        return Err(AppError::conflict(
            "last_project_admin",
            "A project needs at least one admin; add another first",
        ));
    }
    Ok(())
}
