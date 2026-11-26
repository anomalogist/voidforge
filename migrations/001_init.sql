CREATE TABLE IF NOT EXISTS workspaces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    branch TEXT,
    remotes TEXT NOT NULL DEFAULT '[]',
    is_valid INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_opened_at TEXT
);

CREATE TABLE IF NOT EXISTS packages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    version TEXT,
    description TEXT,
    template_path TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(workspace_id, name)
);

CREATE TABLE IF NOT EXISTS builds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER REFERENCES workspaces(id) ON DELETE SET NULL,
    package_name TEXT NOT NULL,
    operation TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at TEXT,
    duration_seconds INTEGER,
    log_path TEXT
);

CREATE INDEX IF NOT EXISTS idx_builds_status ON builds(status);

CREATE TABLE IF NOT EXISTS build_phases (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    build_id INTEGER NOT NULL REFERENCES builds(id) ON DELETE CASCADE,
    phase TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at TEXT
);

CREATE TABLE IF NOT EXISTS dependency_analysis (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    package_name TEXT NOT NULL,
    analyzer_version TEXT NOT NULL,
    payload TEXT NOT NULL,
    analyzed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS patch_metadata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER REFERENCES workspaces(id) ON DELETE CASCADE,
    package_name TEXT NOT NULL,
    patch_path TEXT NOT NULL,
    backup_path TEXT,
    applied INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS git_operations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER REFERENCES workspaces(id) ON DELETE CASCADE,
    operation TEXT NOT NULL,
    detail TEXT,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS recent_activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
