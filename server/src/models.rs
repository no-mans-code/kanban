use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteConnection};

use crate::error::{ApiResult, AppError};

pub const TYPES: [&str; 5] = ["epic", "story", "task", "bug", "subtask"];
pub const PRIORITIES: [&str; 5] = ["highest", "high", "medium", "low", "lowest"];
pub const CATEGORIES: [&str; 3] = ["todo", "in_progress", "done"];
pub const LINK_KINDS: [&str; 4] = ["blocks", "relates", "duplicates", "clones"];

pub const PALETTE: [&str; 10] = [
    "#6366f1", "#22c55e", "#f59e0b", "#ec4899", "#06b6d4", "#a855f7", "#ef4444", "#14b8a6", "#f97316",
    "#84cc16",
];

/// Jira's hierarchy: epic > story/task/bug > subtask. Levels strictly
/// decrease, so the parent tree can never contain a cycle.
pub fn valid_parent(parent_type: &str, child_type: &str) -> bool {
    match parent_type {
        "epic" => matches!(child_type, "story" | "task" | "bug"),
        "story" | "task" | "bug" => child_type == "subtask",
        _ => false,
    }
}

pub fn check_one_of(field: &str, value: &str, allowed: &[&str]) -> ApiResult<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(AppError::invalid(format!("{field} must be one of: {}", allowed.join(", "))))
    }
}

pub fn clean_title(title: &str) -> ApiResult<String> {
    let t = title.trim();
    if t.is_empty() {
        return Err(AppError::invalid("Title cannot be empty"));
    }
    if t.chars().count() > 300 {
        return Err(AppError::invalid("Title is limited to 300 characters"));
    }
    Ok(t.to_string())
}

/// Distinguishes an absent field (None) from an explicit null (Some(None)).
pub fn double_option<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

fn id_list<S: Serializer>(csv: &str, s: S) -> Result<S::Ok, S::Error> {
    let ids: Vec<i64> = csv.split(',').filter_map(|p| p.parse().ok()).collect();
    ids.serialize(s)
}

#[derive(Serialize, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub color: String,
    pub active: bool,
    pub created_at: i64,
    /// human | agent
    pub kind: String,
    pub is_admin: bool,
    /// Whether the account can sign in with a password (never the hash itself).
    pub has_password: bool,
}

pub const USER_SELECT: &str = "SELECT id, username, display_name, color, active, created_at, kind, is_admin,
        password_hash IS NOT NULL AS has_password
   FROM users";

#[derive(Serialize, FromRow)]
pub struct Project {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub description: String,
    pub created_at: i64,
}

#[derive(Serialize, FromRow)]
pub struct Status {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub category: String,
    pub position: i64,
    pub wip_limit: Option<i64>,
}

#[derive(Serialize, FromRow)]
pub struct Label {
    pub id: i64,
    pub name: String,
    pub color: String,
}

#[derive(Serialize, FromRow)]
pub struct Comment {
    pub id: i64,
    pub ticket_id: i64,
    pub author_id: Option<i64>,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize, FromRow)]
pub struct Activity {
    pub id: i64,
    pub ticket_id: i64,
    pub actor_id: Option<i64>,
    pub action: String,
    pub field: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub created_at: i64,
}

/// The lightweight shape used by board, list and graph views. The
/// description is deliberately left out; it is only in the detail view.
#[derive(Serialize, FromRow, Clone)]
pub struct TicketSummary {
    pub id: i64,
    pub key: String,
    pub project_id: i64,
    pub number: i64,
    #[serde(rename = "type")]
    pub ticket_type: String,
    pub title: String,
    pub status_id: i64,
    pub status_name: String,
    pub status_category: String,
    pub priority: String,
    pub assignee_id: Option<i64>,
    pub reporter_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub parent_key: Option<String>,
    pub rank: f64,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
    pub due_date: Option<i64>,
    #[serde(serialize_with = "id_list")]
    pub label_ids: String,
    pub child_count: i64,
    pub done_child_count: i64,
    /// Has at least one `blocks` predecessor that is not done yet.
    pub is_blocked: bool,
    pub comment_count: i64,
}

pub const SUMMARY_SELECT: &str = "
SELECT t.id,
       p.key || '-' || t.number AS key,
       t.project_id, t.number, t.type AS ticket_type, t.title,
       t.status_id, s.name AS status_name, s.category AS status_category,
       t.priority, t.assignee_id, t.reporter_id, t.parent_id,
       (SELECT pp.key || '-' || pt.number FROM tickets pt JOIN projects pp ON pp.id = pt.project_id
         WHERE pt.id = t.parent_id) AS parent_key,
       t.rank, t.created_at, t.updated_at, t.resolved_at, t.due_date,
       COALESCE((SELECT group_concat(tl.label_id) FROM ticket_labels tl WHERE tl.ticket_id = t.id), '')
         AS label_ids,
       (SELECT count(*) FROM tickets c WHERE c.parent_id = t.id) AS child_count,
       (SELECT count(*) FROM tickets c JOIN statuses cs ON cs.id = c.status_id
         WHERE c.parent_id = t.id AND cs.category = 'done') AS done_child_count,
       EXISTS (SELECT 1 FROM ticket_links l
                 JOIN tickets b ON b.id = l.source_id
                 JOIN statuses bs ON bs.id = b.status_id
                WHERE l.target_id = t.id AND l.kind = 'blocks' AND bs.category <> 'done') AS is_blocked,
       (SELECT count(*) FROM comments cm WHERE cm.ticket_id = t.id) AS comment_count
  FROM tickets t
  JOIN projects p ON p.id = t.project_id
  JOIN statuses s ON s.id = t.status_id";

pub async fn summary(conn: &mut SqliteConnection, id: i64) -> ApiResult<TicketSummary> {
    let mut qb = QueryBuilder::<Sqlite>::new(SUMMARY_SELECT);
    qb.push(" WHERE t.id = ").push_bind(id);
    qb.build_query_as::<TicketSummary>()
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::not_found("Ticket"))
}

pub async fn summaries(conn: &mut SqliteConnection, ids: &[i64]) -> ApiResult<HashMap<i64, TicketSummary>> {
    let mut out = HashMap::with_capacity(ids.len());
    // SQLite caps bound parameters; chunk well under the limit.
    for chunk in ids.chunks(500) {
        let mut qb = QueryBuilder::<Sqlite>::new(SUMMARY_SELECT);
        qb.push(" WHERE t.id IN (");
        let mut sep = qb.separated(", ");
        for id in chunk {
            sep.push_bind(*id);
        }
        qb.push(")");
        for s in qb.build_query_as::<TicketSummary>().fetch_all(&mut *conn).await? {
            out.insert(s.id, s);
        }
    }
    Ok(out)
}

/// `"ABC-12"` -> ticket id.
pub async fn resolve_key(conn: &mut SqliteConnection, key: &str) -> ApiResult<i64> {
    let missing = || AppError::not_found(format!("Ticket {key}"));
    let (project, number) = key.rsplit_once('-').ok_or_else(missing)?;
    let number: i64 = number.parse().map_err(|_| missing())?;
    sqlx::query_scalar(
        "SELECT t.id FROM tickets t JOIN projects p ON p.id = t.project_id
          WHERE p.key = ? AND t.number = ?",
    )
    .bind(project.to_ascii_uppercase())
    .bind(number)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(missing)
}

pub async fn ticket_key(conn: &mut SqliteConnection, id: i64) -> ApiResult<String> {
    Ok(sqlx::query_scalar(
        "SELECT p.key || '-' || t.number FROM tickets t JOIN projects p ON p.id = t.project_id
          WHERE t.id = ?",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await?)
}

pub async fn keys_for(conn: &mut SqliteConnection, ids: &[i64]) -> ApiResult<Vec<String>> {
    let mut keys = Vec::with_capacity(ids.len());
    for id in ids {
        keys.push(ticket_key(conn, *id).await?);
    }
    Ok(keys)
}

pub async fn project_id_by_key(conn: &mut SqliteConnection, key: &str) -> ApiResult<i64> {
    sqlx::query_scalar("SELECT id FROM projects WHERE key = ?")
        .bind(key.to_ascii_uppercase())
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::not_found(format!("Project {key}")))
}

pub async fn user_name(conn: &mut SqliteConnection, id: Option<i64>) -> ApiResult<String> {
    let Some(id) = id else { return Ok("Unassigned".into()) };
    sqlx::query_scalar("SELECT display_name FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::invalid(format!("User {id} does not exist")))
}

pub async fn log_activity(
    conn: &mut SqliteConnection,
    ticket_id: i64,
    actor: Option<i64>,
    action: &str,
    field: Option<&str>,
    old: Option<String>,
    new: Option<String>,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO activity (ticket_id, actor_id, action, field, old_value, new_value, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(ticket_id)
    .bind(actor)
    .bind(action)
    .bind(field)
    .bind(old)
    .bind(new)
    .bind(crate::now_ms())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Rebuild one ticket's search row from its title, description and comments.
pub async fn reindex(conn: &mut SqliteConnection, ticket_id: i64) -> ApiResult<()> {
    sqlx::query("DELETE FROM ticket_fts WHERE rowid = ?").bind(ticket_id).execute(&mut *conn).await?;
    sqlx::query(
        "INSERT INTO ticket_fts (rowid, title, description, comments)
         SELECT t.id, t.title, t.description,
                COALESCE((SELECT group_concat(body, ' ') FROM comments WHERE ticket_id = t.id), '')
           FROM tickets t WHERE t.id = ?",
    )
    .bind(ticket_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Civil calendar date (year, month, day) for a unix-ms UTC timestamp, via
/// Howard Hinnant's date algorithms.
pub fn civil_date(ms: i64) -> (i64, u32, u32) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

/// `YYYY-MM-DD` for a unix-ms UTC timestamp, for display (e.g. a due date).
pub fn iso_date(ms: i64) -> String {
    let (y, m, d) = civil_date(ms);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Epoch milliseconds at UTC midnight for a `YYYY-MM-DD` date, or `None` if
/// it doesn't parse. Inverse of `civil_date` (Howard Hinnant's algorithms).
///
/// The day-of-month range check below is deliberately loose (1..=31, not
/// per-month) because the days-from-civil arithmetic doesn't reject an
/// out-of-range day either — it just rolls over into the next month (e.g.
/// "2026-02-30" silently becomes 2026-03-02). Catching that requires a
/// round trip: convert forward, then check `civil_date` agrees on exactly
/// the date we were given.
pub fn parse_iso_date(s: &str) -> Option<i64> {
    let (y, rest) = s.split_once('-')?;
    let (m, d) = rest.split_once('-')?;
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y0 = if m <= 2 { y - 1 } else { y };
    let era = y0.div_euclid(400);
    let yoe = y0 - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let ms = days * 86_400_000;
    (civil_date(ms) == (y, m as u32, d as u32)).then_some(ms)
}

pub async fn watch(conn: &mut SqliteConnection, ticket_id: i64, user: Option<i64>) -> ApiResult<()> {
    if let Some(user) = user {
        sqlx::query("INSERT OR IGNORE INTO ticket_watchers (ticket_id, user_id) VALUES (?, ?)")
            .bind(ticket_id)
            .bind(user)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_date_matches_known_days() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(951_782_400_000), "2000-02-29"); // a leap day
        assert_eq!(iso_date(4_102_444_800_000), "2100-01-01");
    }

    #[test]
    fn parse_iso_date_round_trips_through_civil_date() {
        for s in ["1970-01-01", "2000-02-29", "2026-09-27", "2100-01-01", "1999-12-31"] {
            let ms = parse_iso_date(s).unwrap();
            assert_eq!(iso_date(ms), s, "round trip for {s}");
        }
    }

    #[test]
    fn parse_iso_date_rejects_garbage() {
        for s in ["", "not-a-date", "2026-13-01", "2026-01-40", "2026/09/27"] {
            assert!(parse_iso_date(s).is_none(), "{s} should not parse");
        }
    }

    #[test]
    fn parse_iso_date_rejects_days_that_overflow_their_month() {
        // 2026 isn't a leap year, so Feb has 28 days; April has 30, not 31.
        // The days-from-civil math doesn't reject these on its own — it
        // rolls over into the next month instead — so this is the bug the
        // round-trip check in parse_iso_date exists to catch.
        for s in ["2026-02-29", "2026-02-30", "2026-04-31", "2026-00-01", "2026-01-00"] {
            assert!(parse_iso_date(s).is_none(), "{s} should not parse");
        }
        assert!(parse_iso_date("2024-02-29").is_some(), "2024 is a leap year");
    }
}
