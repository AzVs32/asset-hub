-- 安全审计事件表：记录认证、授权及受保护 HTTP 请求的执行结果。
CREATE TABLE security_audit_events (
    -- 单调递增的本地审计事件标识。
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 事件发生时间，使用 RFC 3339 文本表示。
    occurred_at TEXT NOT NULL,
    -- 发起操作的用户标识；未认证请求可能为空。
    actor_user_id TEXT,
    -- 事件发生时的用户名快照，避免用户信息变化影响历史审计。
    actor_username TEXT,
    -- 稳定的审计事件类型。
    event_type TEXT NOT NULL,
    -- HTTP 请求方法。
    method TEXT NOT NULL,
    -- HTTP 请求路径。
    path TEXT NOT NULL,
    -- HTTP 响应状态码。
    status_code INTEGER NOT NULL,
    -- 事件结果，只允许成功或失败。
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
    -- 可选的操作目标描述，例如资源、用户或目录标识。
    target TEXT
);

-- 加速按时间倒序读取审计事件，并用事件 ID 保证相同时间下顺序稳定。
CREATE INDEX idx_security_audit_events_occurred_at
ON security_audit_events(occurred_at DESC, id DESC);

-- 加速查询指定用户最近产生的审计事件。
CREATE INDEX idx_security_audit_events_actor
ON security_audit_events(actor_user_id, occurred_at DESC);
