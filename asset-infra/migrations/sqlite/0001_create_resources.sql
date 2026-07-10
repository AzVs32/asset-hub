CREATE TABLE IF NOT EXISTS resources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    directory TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    content_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS directories (
    path TEXT PRIMARY KEY NOT NULL,
    parent_path TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_resources_kind
ON resources(kind);

CREATE INDEX IF NOT EXISTS idx_resources_directory
ON resources(directory);

CREATE INDEX IF NOT EXISTS idx_resources_directory_updated_at
ON resources(directory, updated_at);

CREATE INDEX IF NOT EXISTS idx_resources_content_key
ON resources(json_extract(content_json, '$.key'));

CREATE UNIQUE INDEX IF NOT EXISTS idx_resources_directory_name_active
ON resources(directory, name)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_resources_updated_at
ON resources(updated_at);

CREATE INDEX IF NOT EXISTS idx_directories_parent_path
ON directories(parent_path);

CREATE UNIQUE INDEX IF NOT EXISTS idx_directories_parent_name
ON directories(parent_path, name);
