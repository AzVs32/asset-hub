-- Resource 聚合只引用目录稳定 ID。目录路径由目录树查询派生，不在本表重复保存。
CREATE TABLE resources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    directory_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    content_json TEXT,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,

    FOREIGN KEY (directory_id) REFERENCES directories(id) ON DELETE RESTRICT
);

-- 在内容替换移动公开 Blob 之前写入的持久化意图。
-- 每个 Resource 最多一个待处理的替换，防止二次编辑覆盖恢复路径。
CREATE TABLE resource_content_replacements (
    id TEXT PRIMARY KEY NOT NULL,
    resource_id TEXT NOT NULL UNIQUE,
    expected_revision INTEGER NOT NULL CHECK (expected_revision > 0),
    target_key TEXT NOT NULL,
    staged_key TEXT NOT NULL,
    backup_key TEXT NOT NULL,
    replacement_content_json TEXT NOT NULL,

    FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE RESTRICT
);

CREATE INDEX idx_resource_content_replacements_resource
ON resource_content_replacements(resource_id);

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
