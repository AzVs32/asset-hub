-- 外部重试写命令的持久业务幂等。每个 key 标识一个逻辑命令；
-- 存储的请求哈希用于区分「重放」与「冲突复用」。
CREATE TABLE idempotency_records (
    key TEXT PRIMARY KEY NOT NULL,
    request_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('in_progress', 'completed')),
    result_json TEXT,
    created_at TEXT NOT NULL,
    completed_at TEXT
);
