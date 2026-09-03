-- Durable business idempotency for externally retried write commands. Each key identifies one
-- logical command; the stored request hash distinguishes a replay from a conflicting reuse.
CREATE TABLE idempotency_records (
    key TEXT PRIMARY KEY NOT NULL,
    request_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('in_progress', 'completed')),
    result_json TEXT,
    created_at TEXT NOT NULL,
    completed_at TEXT
);
