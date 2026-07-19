-- 安全审计事件表：记录由 HTTP、CLI 等应用入口发起的敏感业务操作及执行结果。
CREATE TABLE security_audit_events (
    -- 单调递增的本地审计事件标识。
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 事件发生时间，使用 RFC 3339 文本表示。
    occurred_at TEXT NOT NULL,
    -- 已认证 Asset Hub 用户的标识；无需登录或认证失败的操作必须为空。
    actor_user_id TEXT,
    -- 操作来源；
    source TEXT NOT NULL CHECK (source IN ('http', 'cli')),
    -- 与操作来源无关的稳定业务事件类型。
    event_type TEXT NOT NULL,
    -- 事件结果，只允许成功或失败。
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
    -- 可选的操作目标描述，例如资源、用户或目录标识；登录尝试可保存输入的目标用户名。
    target TEXT
);

-- 加速按时间倒序读取审计事件，并用事件 ID 保证相同时间下顺序稳定。
CREATE INDEX idx_security_audit_events_occurred_at
ON security_audit_events(occurred_at DESC, id DESC);

-- 加速查询指定已认证用户最近产生的审计事件；匿名事件不会命中该索引查询条件。
CREATE INDEX idx_security_audit_events_actor
ON security_audit_events(actor_user_id, occurred_at DESC);
