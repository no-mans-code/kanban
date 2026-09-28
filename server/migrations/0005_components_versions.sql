-- Per-project taxonomies for organizing tickets, like labels but scoped to
-- one project instead of shared board-wide.
CREATE TABLE components (
    id         INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name       TEXT NOT NULL COLLATE NOCASE,
    UNIQUE (project_id, name)
);

CREATE TABLE versions (
    id         INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name       TEXT NOT NULL COLLATE NOCASE,
    UNIQUE (project_id, name)
);

ALTER TABLE tickets ADD COLUMN component_id INTEGER REFERENCES components(id) ON DELETE SET NULL;
ALTER TABLE tickets ADD COLUMN fix_version_id INTEGER REFERENCES versions(id) ON DELETE SET NULL;
CREATE INDEX tickets_component ON tickets (component_id);
CREATE INDEX tickets_fix_version ON tickets (fix_version_id);
