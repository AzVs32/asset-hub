CREATE TABLE IF NOT EXISTS resources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    directory TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    content_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS resource_metadata_summaries (
    resource_id TEXT PRIMARY KEY NOT NULL,
    description TEXT,
    FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE CASCADE,
    CHECK (description IS NULL OR length(description) <= 1024)
);

CREATE TABLE IF NOT EXISTS resource_metadata_tags (
    resource_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY (resource_id, tag),
    UNIQUE (resource_id, position),
    FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE CASCADE,
    CHECK (position >= 0 AND position < 64),
    CHECK (length(tag) > 0 AND length(tag) <= 64)
);

CREATE TABLE IF NOT EXISTS resource_kind_metadata (
    resource_id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE CASCADE,
    CHECK (schema_version > 0),
    CHECK (json_valid(payload_json)),
    CHECK (json_type(payload_json) = 'object')
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

CREATE INDEX IF NOT EXISTS idx_resource_metadata_tags_tag
ON resource_metadata_tags(tag, resource_id);

CREATE INDEX IF NOT EXISTS idx_resource_kind_metadata_kind
ON resource_kind_metadata(kind);

CREATE INDEX IF NOT EXISTS idx_directories_parent_path
ON directories(parent_path);

CREATE UNIQUE INDEX IF NOT EXISTS idx_directories_parent_name
ON directories(parent_path, name);
