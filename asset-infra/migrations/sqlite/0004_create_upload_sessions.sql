-- 上传会话：持久化新建或替换目标及已接收偏移，支持服务重启后继续分片上传和最终化。
CREATE TABLE upload_sessions (
    -- UUID v7 上传会话标识。
    id TEXT PRIMARY KEY NOT NULL,
    -- create_resource 或 replace_content，决定最终化时创建资源还是替换已有资源内容。
    purpose TEXT NOT NULL,
    -- 新建时为预分配的 Resource ID；替换时为已有 Resource ID。
    resource_id TEXT NOT NULL,
    -- 替换内容时要求的 Resource revision；新建资源时为空。
    expected_revision INTEGER,
    -- Optional durable link to the idempotent create-upload request that created this session.
    idempotency_key TEXT UNIQUE,
    -- 最终 Resource 的文件名。
    name TEXT NOT NULL,
    -- 目标目录稳定身份；最终 StorageKey 在发布时由当前目录投影解析。
    directory_id TEXT NOT NULL,
    -- 客户端声明的 MIME 类型；未提供时允许为空。
    mime_type TEXT,
    -- 客户端声明的文件总字节数，仅用于完整性校验，不作为上传大小上限。
    expected_size INTEGER NOT NULL,
    -- 已持久化的连续字节数；服务重启后可与临时文件的实际大小进行校准。
    offset INTEGER NOT NULL,
    -- uploading、finalizing、completed 或 failed。
    status TEXT NOT NULL,
    -- 客户端对本地原文件计算出的 SHA-256；用于端到端完整性校验和安全恢复上传会话。
    expected_checksum_value TEXT NOT NULL,
    -- finalization 对服务端暂存文件计算出的 SHA-256；持久化后服务重启无需重复计算。
    actual_checksum_value TEXT,
    -- 后台 finalization 的最近一次失败原因；重新提交 complete 时清空。
    failure TEXT,
    -- 会话创建时间，RFC 3339。
    created_at TEXT NOT NULL,
    -- 最近一次写入或状态变更时间，RFC 3339。
    updated_at TEXT NOT NULL,

    -- 声明大小必须非负，且已接收偏移不能超过声明大小。
    CHECK (expected_size >= 0),
    CHECK (offset >= 0 AND offset <= expected_size),
    CHECK (status IN ('uploading', 'finalizing', 'completed', 'failed')),
    CHECK (purpose IN ('create_resource', 'replace_content')),
    CHECK (
        (purpose = 'create_resource' AND expected_revision IS NULL)
        OR (purpose = 'replace_content' AND expected_revision > 0)
    ),
    -- SHA-256 使用 64 位小写十六进制；精确格式同时由领域对象校验。
    CHECK (length(expected_checksum_value) = 64),
    CHECK (actual_checksum_value IS NULL OR length(actual_checksum_value) = 64),
    -- 只有客户端期望摘要和服务端实际摘要完全一致，上传才能完成。
    CHECK (
        status != 'completed'
        OR (
            actual_checksum_value IS NOT NULL
            AND actual_checksum_value = expected_checksum_value
        )
    ),
    CHECK (status != 'failed' OR failure IS NOT NULL),
    FOREIGN KEY (directory_id) REFERENCES directories(id) ON DELETE RESTRICT
);

-- 加速服务启动时恢复尚未完成的后台 finalization。
CREATE INDEX idx_upload_sessions_status_updated
ON upload_sessions(status, updated_at);
