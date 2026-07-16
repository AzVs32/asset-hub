CREATE TABLE security_audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at TEXT NOT NULL,
    actor_user_id TEXT,
    actor_username TEXT,
    event_type TEXT NOT NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    status_code INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
    target TEXT
);

CREATE INDEX idx_security_audit_events_occurred_at
ON security_audit_events(occurred_at DESC, id DESC);

CREATE INDEX idx_security_audit_events_actor
ON security_audit_events(actor_user_id, occurred_at DESC);
