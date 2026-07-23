# PostgreSQL migrations

PostgreSQL migrations will live in this directory when the PostgreSQL repository
adapter is implemented.

Keep migration version numbers aligned with `../sqlite`, but write SQL for the
target database dialect instead of copying SQLite SQL directly. For example,
SQLite `0001_create_directories.sql` should have a PostgreSQL counterpart with
the same version once PostgreSQL support is added.
