-- 资源主表：保存资源聚合的基础属性、生命周期状态和对象内容引用。
CREATE TABLE IF NOT EXISTS resources (
    -- 资源标识。
    id TEXT PRIMARY KEY NOT NULL,
    -- 资源文件名；与 directory 共同构成资源及其 Blob 的唯一规范路径。
    name TEXT NOT NULL,
    -- 规范化资源目录路径，与 Blob 存储目录一致；空字符串表示根目录。
    directory TEXT NOT NULL DEFAULT '',
    -- 资源类型标识，通常使用 namespace:typename 格式。
    kind TEXT NOT NULL,
    -- 资源生命周期状态，例如 active 或 archived。
    status TEXT NOT NULL,
    -- 可选的资源描述，最多 1024 个字符。
    description TEXT,
    -- 可选的对象内容属性，包含大小、MIME 类型和服务端计算的单个校验和；
    content_json TEXT,
    -- 资源创建时间，使用 RFC 3339 文本表示。
    created_at TEXT NOT NULL,
    -- 资源最后更新时间，使用 RFC 3339 文本表示。
    updated_at TEXT NOT NULL,
    -- 软删除时间；为空表示资源未被删除。
    deleted_at TEXT,
    -- 限制描述长度，避免持久化不符合领域约束的数据。
    CHECK (description IS NULL OR length(description) <= 1024)
);

-- 标签字典表：每个已完成领域归一化的标签文本只保存一次。
CREATE TABLE IF NOT EXISTS tags (
    -- SQLite 内部标签标识，不向领域模型和外部接口暴露。
    id INTEGER PRIMARY KEY,
    -- 标签文本；使用二进制排序规则，与领域层的大小写敏感语义保持一致。
    name TEXT NOT NULL COLLATE BINARY UNIQUE,
    -- 标签不能为空且最多包含 64 个字符。
    CHECK (length(name) > 0 AND length(name) <= 64)
);

-- 资源标签关联表：保存 Resource 与标签字典之间的无序多对多关系。
CREATE TABLE IF NOT EXISTS resource_tags (
    -- 标签所属的资源标识。
    resource_id TEXT NOT NULL,
    -- 指向标签字典的内部标识。
    tag_id INTEGER NOT NULL,
    -- 同一资源不能重复关联同一个标签。
    PRIMARY KEY (resource_id, tag_id),
    -- 删除资源时级联删除其全部标签关联。
    FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE CASCADE,
    -- 删除标签字典项时级联删除对应关联。
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
-- 对于“多对多中间表”，使用 `WITHOUT ROWID` 可节省存储空间、避免回表。
) WITHOUT ROWID;

-- 用户可见资源目录表：每条记录都必须对应存储侧的真实目录或目录标记。
-- 根目录使用空路径隐式表示，不写入本表；内部 `.asset-hub` 命名空间也不写入本表。
CREATE TABLE IF NOT EXISTS directories (
    -- 完整规范化目录路径；仅保存非根、非内部目录。
    path TEXT PRIMARY KEY NOT NULL,
    -- 直接父目录的规范化路径。
    parent_path TEXT NOT NULL,
    -- 当前目录的单段名称。
    name TEXT NOT NULL,
    -- 目录创建时间，使用 RFC 3339 文本表示。
    created_at TEXT NOT NULL,
    -- 目录最后更新时间，使用 RFC 3339 文本表示。
    updated_at TEXT NOT NULL
);

-- 加速按资源类型筛选资源。
CREATE INDEX IF NOT EXISTS idx_resources_kind
ON resources(kind);

-- 加速按逻辑目录筛选资源。
CREATE INDEX IF NOT EXISTS idx_resources_directory
ON resources(directory);

-- 加速目录内按更新时间排序或增量读取资源。
CREATE INDEX IF NOT EXISTS idx_resources_directory_updated_at
ON resources(directory, updated_at);

-- 保证同一目录下未软删除资源的名称唯一；已删除资源不参与唯一性约束。
CREATE UNIQUE INDEX IF NOT EXISTS idx_resources_directory_name_active
ON resources(directory, name)
WHERE deleted_at IS NULL;

-- 加速按资源更新时间排序或执行增量扫描。
CREATE INDEX IF NOT EXISTS idx_resources_updated_at
ON resources(updated_at);

-- 加速按标签反查资源。
CREATE INDEX IF NOT EXISTS idx_resource_tags_tag_id
ON resource_tags(tag_id, resource_id);

-- 加速列出指定父目录下的直接子目录。
CREATE INDEX IF NOT EXISTS idx_directories_parent_path
ON directories(parent_path);

-- 保证同一父目录下的直接子目录名称唯一。
CREATE UNIQUE INDEX IF NOT EXISTS idx_directories_parent_name
ON directories(parent_path, name);
