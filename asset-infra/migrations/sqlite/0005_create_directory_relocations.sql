-- Directory rename/move is a cross-resource workflow: persist the desired database state before
-- atomically renaming the local filesystem subtree. Startup recovery completes either side after a
-- process interruption.
CREATE TABLE directory_relocations (
    directory_id TEXT PRIMARY KEY NOT NULL,
    source_path TEXT NOT NULL,
    destination_path TEXT NOT NULL,
    FOREIGN KEY (directory_id) REFERENCES directories(id) ON DELETE RESTRICT,
    CHECK (source_path <> destination_path)
);

CREATE TABLE directory_relocation_updates (
    relocation_directory_id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    directory_id TEXT NOT NULL,
    expected_revision INTEGER NOT NULL CHECK (expected_revision > 0),
    parent_id TEXT,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > expected_revision),
    PRIMARY KEY (relocation_directory_id, position),
    UNIQUE (relocation_directory_id, directory_id),
    FOREIGN KEY (relocation_directory_id) REFERENCES directory_relocations(directory_id)
        ON DELETE CASCADE,
    CHECK (kind LIKE '%:%')
);
