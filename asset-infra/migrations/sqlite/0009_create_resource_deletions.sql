-- Permanent Resource deletion crosses visible Blob storage and the aggregate row. The intent is
-- deliberately not a foreign key: it must survive the committed Resource deletion until the
-- internal staged Blob has been removed during normal execution or startup recovery.
CREATE TABLE resource_deletions (
    resource_id TEXT PRIMARY KEY NOT NULL,
    expected_revision INTEGER NOT NULL CHECK (expected_revision > 0),
    source_key TEXT NOT NULL,
    deletion_key TEXT NOT NULL,
    CHECK (source_key <> deletion_key)
);
