-- 目录是独立聚合。固定 nil UUID 表示全局根目录；其余节点通过 parent_id 组成邻接树。
CREATE TABLE directories (
    id TEXT PRIMARY KEY NOT NULL,
    parent_id TEXT,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    FOREIGN KEY (parent_id) REFERENCES directories(id) ON DELETE RESTRICT,
    CHECK (
        (id = '00000000-0000-0000-0000-000000000000' AND parent_id IS NULL AND name = '')
        OR
        (id <> '00000000-0000-0000-0000-000000000000' AND parent_id IS NOT NULL AND length(name) > 0)
    ),
    CHECK (kind LIKE '%:%')
);

-- 迁移直接建立唯一全局根节点。它是普通持久化聚合，而不是查询时伪造的隐式路径。
INSERT INTO directories (
    id, parent_id, name, kind, created_at, updated_at
) VALUES (
    '00000000-0000-0000-0000-000000000000',
    NULL,
    '',
    'core:directory',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);

CREATE INDEX idx_directories_parent_id
ON directories(parent_id);

CREATE UNIQUE INDEX idx_directories_parent_name
ON directories(parent_id, name);
