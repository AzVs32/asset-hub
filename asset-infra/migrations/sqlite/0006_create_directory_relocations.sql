-- 目录重命名/移动是跨资源工作流：先持久化期望的数据库状态，再原子重命名本地文件系统子树。
-- 进程中断后，启动恢复可完成两侧中的任意一侧。
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
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > expected_revision),
    PRIMARY KEY (relocation_directory_id, position),
    UNIQUE (relocation_directory_id, directory_id),
    FOREIGN KEY (relocation_directory_id) REFERENCES directory_relocations(directory_id)
        ON DELETE CASCADE
);
