use asset_core::CoreError;
use sqlx::SqlitePool;

/// SQLite migration 集合。
///
/// `sqlx::migrate!` 会在编译期读取 `asset-infra/migrations/sqlite`，并把迁移文件嵌入
/// 到二进制中。新增迁移后需要重新编译应用。
static SQLITE_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("migrations/sqlite");

/// 在指定 SQLite 连接池上执行尚未应用的迁移。
pub async fn run(pool: &SqlitePool) -> Result<(), CoreError> {
    SQLITE_MIGRATOR
        .run(pool)
        .await
        .map_err(|error| CoreError::repository("sqlite.migrate", error))
}
