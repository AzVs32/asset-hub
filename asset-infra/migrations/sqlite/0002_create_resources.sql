-- Resource 聚合只引用目录稳定 ID。目录路径由目录树查询派生，不在本表重复保存。
CREATE TABLE resources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    directory_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    description TEXT,
    content_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,

    FOREIGN KEY (directory_id) REFERENCES directories(id) ON DELETE RESTRICT,
    CHECK (description IS NULL OR length(description) <= 1024)
);

CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL COLLATE BINARY UNIQUE,
    CHECK (length(name) > 0 AND length(name) <= 64)
);

CREATE TABLE resource_tags (
    resource_id TEXT NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (resource_id, tag_id),
    FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE INDEX idx_resources_kind
ON resources(kind);

CREATE INDEX idx_resources_directory_id
ON resources(directory_id);

CREATE INDEX idx_resources_directory_updated_at
ON resources(directory_id, updated_at);

CREATE UNIQUE INDEX idx_resources_directory_name_active
ON resources(directory_id, name)
WHERE deleted_at IS NULL;

CREATE INDEX idx_resources_updated_at
ON resources(updated_at);

CREATE INDEX idx_resource_tags_tag_id
ON resource_tags(tag_id, resource_id);
