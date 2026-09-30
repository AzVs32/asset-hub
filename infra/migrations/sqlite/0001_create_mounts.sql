CREATE TABLE mounts (
    id TEXT NOT NULL PRIMARY KEY,
    virtual_path TEXT NOT NULL,
    driver_kind TEXT NOT NULL,
    driver_path TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
);

CREATE UNIQUE INDEX mounts_enabled_path ON mounts (virtual_path) WHERE enabled = 1;
CREATE INDEX mounts_path ON mounts (virtual_path);
