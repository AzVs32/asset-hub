-- 外部重试写命令的持久业务幂等。每个 key 标识一个逻辑命令；
-- 存储的请求哈希用于区分「重放」与「冲突复用」。
CREATE TABLE idempotency_records (
    key TEXT PRIMARY KEY NOT NULL,
    request_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('in_progress', 'completed')),
    execution_id TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    result_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT
);

-- Completed rows are looked up by primary key. This supports stale in-progress inspection and
-- operational maintenance without making the key lookup path less selective.
CREATE INDEX idx_idempotency_records_in_progress_lease
    ON idempotency_records (lease_expires_at)
    WHERE status = 'in_progress';
