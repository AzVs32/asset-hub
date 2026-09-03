-- 内容替换的持久化恢复意图：在移动公开 Blob 之前写入，供启动恢复完成或回滚。
-- 每个 Resource 最多一个待处理替换，避免二次编辑覆盖恢复路径。
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
