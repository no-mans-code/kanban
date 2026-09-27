CREATE TABLE users (
    id           INTEGER PRIMARY KEY,
    username     TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name TEXT NOT NULL,
    color        TEXT NOT NULL,
    active       INTEGER NOT NULL DEFAULT 1,
    created_at   INTEGER NOT NULL
);

CREATE TABLE projects (
    id          INTEGER PRIMARY KEY,
    key         TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    next_number INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL
);

CREATE TABLE statuses (
    id         INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name       TEXT NOT NULL COLLATE NOCASE,
    category   TEXT NOT NULL CHECK (category IN ('todo', 'in_progress', 'done')),
    position   INTEGER NOT NULL,
    UNIQUE (project_id, name)
);

CREATE TABLE labels (
    id    INTEGER PRIMARY KEY,
    name  TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color TEXT NOT NULL
);

CREATE TABLE tickets (
    id          INTEGER PRIMARY KEY,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    number      INTEGER NOT NULL,
    type        TEXT NOT NULL CHECK (type IN ('epic', 'story', 'task', 'bug', 'subtask')),
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status_id   INTEGER NOT NULL REFERENCES statuses(id),
    priority    TEXT NOT NULL DEFAULT 'medium'
                CHECK (priority IN ('highest', 'high', 'medium', 'low', 'lowest')),
    assignee_id INTEGER REFERENCES users(id),
    reporter_id INTEGER REFERENCES users(id),
    parent_id   INTEGER REFERENCES tickets(id) ON DELETE SET NULL,
    rank        REAL NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    resolved_at INTEGER,
    UNIQUE (project_id, number)
);
CREATE INDEX tickets_board ON tickets (project_id, status_id, rank);
CREATE INDEX tickets_parent ON tickets (parent_id);
CREATE INDEX tickets_assignee ON tickets (assignee_id);

CREATE TABLE ticket_labels (
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    label_id  INTEGER NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    PRIMARY KEY (ticket_id, label_id)
) WITHOUT ROWID;
CREATE INDEX ticket_labels_label ON ticket_labels (label_id);

CREATE TABLE ticket_watchers (
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (ticket_id, user_id)
) WITHOUT ROWID;

CREATE TABLE comments (
    id         INTEGER PRIMARY KEY,
    ticket_id  INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    author_id  INTEGER REFERENCES users(id),
    body       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX comments_ticket ON comments (ticket_id, id);

-- source -> target. For 'blocks', target cannot start until source is done;
-- only 'blocks' edges form the dependency DAG.
CREATE TABLE ticket_links (
    source_id  INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    target_id  INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('blocks', 'relates', 'duplicates', 'clones')),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (source_id, target_id, kind),
    CHECK (source_id <> target_id)
) WITHOUT ROWID;
CREATE INDEX ticket_links_target ON ticket_links (target_id, kind);

CREATE TABLE activity (
    id         INTEGER PRIMARY KEY,
    ticket_id  INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    actor_id   INTEGER REFERENCES users(id),
    action     TEXT NOT NULL,
    field      TEXT,
    old_value  TEXT,
    new_value  TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX activity_ticket ON activity (ticket_id, id);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

-- rowid = tickets.id. Maintained by the server, not by triggers, because the
-- comments column is an aggregate over another table.
CREATE VIRTUAL TABLE ticket_fts USING fts5 (
    title, description, comments,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);
