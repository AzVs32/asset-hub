-- Resource 聚合只引用目录稳定 ID。目录路径由目录树查询派生，不在本表重复保存。
CREATE TABLE resources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    directory_id TEXT NOT NULL,
    content_json TEXT,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    FOREIGN KEY (directory_id) REFERENCES directories(id) ON DELETE RESTRICT
);

CREATE INDEX idx_resources_directory_id
ON resources(directory_id);

CREATE INDEX idx_resources_directory_updated_at
ON resources(directory_id, updated_at);

CREATE UNIQUE INDEX idx_resources_directory_name
ON resources(directory_id, name);

CREATE INDEX idx_resources_updated_at
ON resources(updated_at);
