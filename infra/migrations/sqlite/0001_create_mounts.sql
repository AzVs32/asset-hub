CREATE TABLE IF NOT EXISTS mounts (
    id TEXT NOT NULL PRIMARY KEY,
    virtual_path TEXT NOT NULL UNIQUE,
    driver_kind TEXT NOT NULL,
    driver_path TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
);
