CREATE TABLE IF NOT EXISTS local_instances (
    id TEXT PRIMARY KEY NOT NULL,
    root_path TEXT NOT NULL CHECK (length(root_path) > 0)
) STRICT;
