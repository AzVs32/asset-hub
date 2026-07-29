-- 上传会话：在 Resource 创建前持久化目标元数据和已接收偏移，支持服务重启后继续分片上传。
CREATE TABLE upload_sessions (
    -- UUID v7 上传会话标识。
    id TEXT PRIMARY KEY NOT NULL,
    -- finalization 开始前预先分配的 Resource ID；Resource 记录仅在内容发布成功后创建。
    resource_id TEXT NOT NULL UNIQUE,
    -- 会话所有者；仅所有者可继续、完成或放弃上传。
    owner_id TEXT NOT NULL,
    -- 最终 Resource 的文件名。
    name TEXT NOT NULL,
    -- 解析到当前工作区后的目标目录路径；与 name 共同确定最终 StorageKey。
    directory TEXT NOT NULL,
    -- 最终 Resource 使用的已注册资源类型。
    kind TEXT NOT NULL,
    -- 最终 Resource 的标签，以 JSON 数组持久化。
    tags_json TEXT NOT NULL,
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
    CHECK (status != 'failed' OR failure IS NOT NULL)
);

-- 加速服务启动时恢复尚未完成的后台 finalization。
CREATE INDEX idx_upload_sessions_status_updated
ON upload_sessions(status, updated_at);
