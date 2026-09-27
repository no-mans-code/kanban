//! First-run bootstrap and optional demo data. Both go through the real HTTP
//! API in-process, so seeded data passes the same validation as anything else.

use std::collections::HashMap;

use anyhow::{Context, bail};
use axum::Router;
use axum::http::Method;
use serde_json::{Value, json};

use crate::inproc::call;

async fn ok(
    app: &Router,
    method: Method,
    uri: &str,
    actor: Option<i64>,
    body: Value,
) -> anyhow::Result<Value> {
    let (status, value) = call(app, method.clone(), uri, actor, Some(body)).await?;
    if !(200..300).contains(&status) {
        bail!("{method} {uri} failed with {status}: {value}");
    }
    Ok(value)
}

/// Make sure there is at least one user to act as.
pub async fn bootstrap(app: &Router) -> anyhow::Result<()> {
    let (_, users) = call(app, Method::GET, "/api/users", None, None).await?;
    if users.as_array().is_some_and(|u| u.is_empty()) {
        ok(app, Method::POST, "/api/users", None, json!({ "username": "me", "display_name": "Me" })).await?;
    }
    Ok(())
}

async fn user_ids(app: &Router, wanted: &[(&str, &str)]) -> anyhow::Result<HashMap<String, i64>> {
    let (_, users) = call(app, Method::GET, "/api/users", None, None).await?;
    let mut ids: HashMap<String, i64> = users
        .as_array()
        .context("user list")?
        .iter()
        .filter_map(|u| Some((u["username"].as_str()?.to_string(), u["id"].as_i64()?)))
        .collect();
    for (username, display) in wanted {
        if !ids.contains_key(*username) {
            let u = ok(
                app,
                Method::POST,
                "/api/users",
                None,
                json!({ "username": username, "display_name": display }),
            )
            .await?;
            ids.insert(username.to_string(), u["id"].as_i64().context("user id")?);
        }
    }
    Ok(ids)
}

/// A small project that exercises every feature: hierarchy, a dependency
/// chain that crosses epics, comments, labels and each status.
pub async fn seed(app: &Router) -> anyhow::Result<()> {
    let (status, _) = call(app, Method::GET, "/api/projects/DEMO", None, None).await?;
    if status == 200 {
        return Ok(());
    }
    let users = user_ids(
        app,
        &[
            ("ada", "Ada Lovelace"),
            ("grace", "Grace Hopper"),
            ("coder", "God (Coder)"),
            ("research", "Research Agent"),
        ],
    )
    .await?;
    let (ada, grace, coder, research) = (users["ada"], users["grace"], users["coder"], users["research"]);

    let project = ok(
        app,
        Method::POST,
        "/api/projects",
        None,
        json!({
            "key": "DEMO", "name": "Demo: Agent Platform",
            "description": "Sample data for exploring the board."
        }),
    )
    .await?;
    let status: HashMap<String, i64> = project["statuses"]
        .as_array()
        .context("statuses")?
        .iter()
        .filter_map(|s| Some((s["name"].as_str()?.to_string(), s["id"].as_i64()?)))
        .collect();

    let (_, existing) = call(app, Method::GET, "/api/labels", None, None).await?;
    let mut labels: HashMap<String, i64> = existing
        .as_array()
        .context("labels")?
        .iter()
        .filter_map(|l| Some((l["name"].as_str()?.to_string(), l["id"].as_i64()?)))
        .collect();
    for name in ["backend", "frontend", "research", "agent"] {
        if !labels.contains_key(name) {
            let l = ok(app, Method::POST, "/api/labels", None, json!({ "name": name })).await?;
            labels.insert(name.into(), l["id"].as_i64().context("label id")?);
        }
    }

    // (type, title, status, assignee, parent index, labels, priority, description)
    type Spec<'a> = (&'a str, &'a str, &'a str, Option<i64>, Option<usize>, &'a [&'a str], &'a str, &'a str);
    let specs: [Spec; 13] = [
        (
            "epic",
            "Replace the orchestrator's ticket store",
            "In Progress",
            Some(ada),
            None,
            &["backend"],
            "high",
            "Move ticket state out of the orchestrator process into this board.\n\n**Done when** the dispatch loop reads and writes only through the board API.",
        ),
        (
            "story",
            "Design the ticket schema",
            "Done",
            Some(ada),
            Some(0),
            &["backend"],
            "high",
            "Tickets, comments, watchers, labels, and a `ticket_links` edge table for the dependency DAG.",
        ),
        ("subtask", "Write the initial migration", "Done", Some(coder), Some(1), &[], "medium", ""),
        ("subtask", "Review indexes for board queries", "Done", Some(grace), Some(1), &[], "low", ""),
        (
            "story",
            "Build the REST API",
            "In Progress",
            Some(coder),
            Some(0),
            &["backend", "agent"],
            "high",
            "CRUD for tickets and comments, drag-and-drop ranking, cycle and height checks on links.",
        ),
        ("subtask", "Cycle detection on new links", "Done", Some(coder), Some(4), &[], "high", ""),
        ("subtask", "Max-height setting", "In Review", Some(coder), Some(4), &[], "medium", ""),
        (
            "story",
            "Build the board UI",
            "To Do",
            Some(grace),
            Some(0),
            &["frontend"],
            "medium",
            "Kanban columns with drag and drop, list view, ticket panel, DAG view, dark and light themes.",
        ),
        (
            "task",
            "Compare open-source Kanban licenses",
            "Done",
            Some(research),
            Some(0),
            &["research", "agent"],
            "medium",
            "Vikunja (AGPL), Kanboard (MIT), Wekan (MIT), Plane (AGPL), Planka (restricted).",
        ),
        (
            "bug",
            "Card jumps back after dropping into an empty column",
            "In Review",
            Some(grace),
            Some(0),
            &["frontend"],
            "highest",
            "Steps:\n1. Drag a card into a column with no cards\n2. It snaps back to where it started",
        ),
        (
            "epic",
            "Extract the Knowledge service",
            "To Do",
            None,
            None,
            &["backend"],
            "medium",
            "Lift `documents.py` into its own service behind a thin client with unchanged signatures.",
        ),
        (
            "task",
            "Put documents.py behind an HTTP API",
            "To Do",
            Some(coder),
            Some(10),
            &["backend", "agent"],
            "medium",
            "",
        ),
        (
            "task",
            "Add vector search with nomic-embed-text",
            "To Do",
            None,
            Some(10),
            &["backend", "research"],
            "low",
            "",
        ),
    ];

    let mut keys: Vec<String> = Vec::new();
    for (ticket_type, title, st, assignee, parent, tags, priority, description) in specs {
        let label_ids: Vec<i64> = tags.iter().map(|t| labels[*t]).collect();
        let t = ok(
            app,
            Method::POST,
            "/api/tickets",
            Some(ada),
            json!({
                "project": "DEMO", "type": ticket_type, "title": title, "description": description,
                "status_id": status[st], "assignee_id": assignee, "priority": priority,
                "parent": parent.map(|i| keys[i].clone()), "label_ids": label_ids,
            }),
        )
        .await?;
        keys.push(t["key"].as_str().context("key")?.to_string());
    }

    // research -> schema -> API -> UI, and API -> knowledge API -> vector search
    for (s, t, kind) in [
        (8, 1, "blocks"),
        (1, 4, "blocks"),
        (4, 7, "blocks"),
        (4, 11, "blocks"),
        (11, 12, "blocks"),
        (9, 7, "relates"),
    ] {
        ok(
            app,
            Method::POST,
            "/api/links",
            Some(ada),
            json!({ "source": keys[s], "target": keys[t], "kind": kind }),
        )
        .await?;
    }

    for (i, who, text) in [
        (
            4,
            grace,
            "Can moves be a single-row update? Renumbering a whole column on every drag would be slow.",
        ),
        (
            4,
            coder,
            "Yes: ranks are floats, a move takes the midpoint of its neighbours, and the column is only renumbered when a gap runs out.",
        ),
        (9, ada, "Reproduced. The drop handler assumes the target column has at least one card."),
        (
            8,
            research,
            "Planka moved to a source-available license in 2.0; it's the only one that restricts commercial hosting.",
        ),
    ] {
        ok(
            app,
            Method::POST,
            &format!("/api/tickets/{}/comments", keys[i]),
            Some(who),
            json!({ "body": text }),
        )
        .await?;
    }
    Ok(())
}
