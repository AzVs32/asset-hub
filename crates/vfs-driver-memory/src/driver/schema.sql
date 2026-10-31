CREATE TABLE IF NOT EXISTS memory_instances (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) > 0),
    capacity_size INTEGER NOT NULL CHECK (capacity_size > 0),
    capacity_unit TEXT NOT NULL CHECK (capacity_unit IN ('B', 'KiB', 'MiB', 'GiB', 'TiB'))
) STRICT;
