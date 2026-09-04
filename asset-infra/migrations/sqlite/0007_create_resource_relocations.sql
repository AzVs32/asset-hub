-- 资源重命名/移动将聚合 CAS 与本地 Blob 移动协调起来。期望聚合先持久化，
-- 以便启动恢复完成进程在任一侧中断后的收尾。
CREATE TABLE resource_relocations (
    resource_id TEXT PRIMARY KEY NOT NULL,
    expected_revision INTEGER NOT NULL CHECK (expected_revision > 0),
    source_key TEXT NOT NULL,
    destination_key TEXT NOT NULL,
    name TEXT NOT NULL,
    directory_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    content_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > expected_revision),
    FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE RESTRICT,
    CHECK (source_key <> destination_key)
);
