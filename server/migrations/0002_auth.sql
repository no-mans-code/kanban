-- Accounts: humans sign in with a password, agents only use API tokens.
ALTER TABLE users ADD COLUMN kind TEXT NOT NULL DEFAULT 'human' CHECK (kind IN ('human', 'agent'));
ALTER TABLE users ADD COLUMN is_admin INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN password_hash TEXT;

-- Visibility is membership: a project you're not in doesn't exist for you.
CREATE TABLE project_members (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role       TEXT NOT NULL CHECK (role IN ('viewer', 'member', 'admin')),
    PRIMARY KEY (project_id, user_id)
) WITHOUT ROWID;
CREATE INDEX project_members_user ON project_members (user_id);

-- Only a SHA-256 of each token is stored; the token itself is shown once.
CREATE TABLE api_tokens (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    hint         TEXT NOT NULL,
    hash         TEXT NOT NULL UNIQUE,
    read_only    INTEGER NOT NULL DEFAULT 0,
    all_projects INTEGER NOT NULL DEFAULT 1,
    created_by   INTEGER REFERENCES users(id),
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER,
    last_used_at INTEGER,
    revoked_at   INTEGER
);
CREATE INDEX api_tokens_user ON api_tokens (user_id);

-- Narrows a token to some of its owner's projects (all_projects = 0).
CREATE TABLE api_token_projects (
    token_id   INTEGER NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    PRIMARY KEY (token_id, project_id)
) WITHOUT ROWID;

CREATE TABLE sessions (
    hash       TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) WITHOUT ROWID;

-- Upgrading must not hide anything: everyone already on the board keeps
-- seeing every existing project.
INSERT INTO project_members (project_id, user_id, role)
SELECT p.id, u.id, 'member' FROM projects p CROSS JOIN users u;
