//! Host-owned SQLite instance records.
use crate::{
    application::{
        dto::{InstanceQuery, RemoveInstanceOutcome},
        port::{DriverFuture, InstanceRecord, InstanceRecordPage, InstanceRecordRepository},
    },
    domain::{
        DriverError, DriverInstanceRef, DriverKey, InstanceId, StorageError, StorageErrorKind,
    },
    infrastructure::sqlite_error::database_error,
};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};

pub struct SqliteInstanceRecordRepository {
    pool: SqlitePool,
}
impl SqliteInstanceRecordRepository {
    /// Initialize the host record table without modifying other application data.
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, DriverError> {
        sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS vfs_instance_records (
            driver_key TEXT COLLATE BINARY NOT NULL,
            instance_id TEXT COLLATE BINARY NOT NULL,
            config_version INTEGER NOT NULL CHECK(config_version > 0),
            revision INTEGER NOT NULL CHECK(revision > 0),
            config_json TEXT NOT NULL,
            created_at INTEGER NOT NULL CHECK(created_at >= 0),
            updated_at INTEGER NOT NULL CHECK(updated_at >= created_at),
            PRIMARY KEY(driver_key, instance_id)
        ) STRICT;",
        )
        .execute(&pool)
        .await
        .map_err(database_error)?;
        Ok(Self { pool })
    }
}
fn invalid() -> DriverError {
    StorageError::new(StorageErrorKind::InvalidData).into()
}
fn decode(row: SqliteRow) -> Result<InstanceRecord, DriverError> {
    let key: String = row.try_get("driver_key").map_err(database_error)?;
    let id: String = row.try_get("instance_id").map_err(database_error)?;
    let version: i64 = row.try_get("config_version").map_err(database_error)?;
    let revision: i64 = row.try_get("revision").map_err(database_error)?;
    let created: i64 = row.try_get("created_at").map_err(database_error)?;
    let updated: i64 = row.try_get("updated_at").map_err(database_error)?;
    InstanceRecord::new(
        DriverInstanceRef::new(
            DriverKey::parse(&key).map_err(|_| invalid())?,
            InstanceId::parse(&id).map_err(|_| invalid())?,
        ),
        u32::try_from(version).map_err(|_| invalid())?,
        u64::try_from(revision).map_err(|_| invalid())?,
        row.try_get("config_json").map_err(database_error)?,
        u64::try_from(created).map_err(|_| invalid())?,
        u64::try_from(updated).map_err(|_| invalid())?,
    )
}
impl InstanceRecordRepository for SqliteInstanceRecordRepository {
    fn get(&self, instance: DriverInstanceRef) -> DriverFuture<'_, Option<InstanceRecord>> {
        Box::pin(async move {
            sqlx::query(
                "SELECT * FROM vfs_instance_records WHERE driver_key = ? AND instance_id = ?",
            )
            .bind(instance.driver_key().as_str())
            .bind(instance.instance_id().as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?
            .map(decode)
            .transpose()
        })
    }
    fn list(
        &self,
        driver: DriverKey,
        query: InstanceQuery,
    ) -> DriverFuture<'_, InstanceRecordPage> {
        Box::pin(async move {
            let after = query.after().map(InstanceId::as_str);
            let mut records = sqlx::query("SELECT * FROM vfs_instance_records WHERE driver_key = ? AND (? IS NULL OR instance_id COLLATE BINARY > ?) ORDER BY instance_id COLLATE BINARY LIMIT ?")
                .bind(driver.as_str()).bind(after).bind(after).bind((query.limit().get()+1) as i64)
                .fetch_all(&self.pool).await.map_err(database_error)?.into_iter().map(decode).collect::<Result<Vec<_>, _>>()?;
            let more = records.len() > query.limit().get();
            records.truncate(query.limit().get());
            let next_after = more
                .then(|| records.last().map(|r| r.instance().instance_id().clone()))
                .flatten();
            Ok(InstanceRecordPage {
                records,
                next_after,
            })
        })
    }
    fn insert(&self, record: InstanceRecord) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query("INSERT INTO vfs_instance_records (driver_key, instance_id, config_version, revision, config_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
                .bind(record.instance().driver_key().as_str())
                .bind(record.instance().instance_id().as_str())
                .bind(i64::from(record.config_version()))
                .bind(record.revision() as i64)
                .bind(record.config_json())
                .bind(record.created_at() as i64)
                .bind(record.updated_at() as i64)
                .execute(&self.pool)
                .await
                .map_err(database_error)?;
            Ok(())
        })
    }
    fn update(&self, record: InstanceRecord, expected_revision: u64) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            let result = sqlx::query("UPDATE vfs_instance_records SET config_version = ?, revision = ?, config_json = ?, updated_at = ? WHERE driver_key = ? AND instance_id = ? AND revision = ?")
                .bind(i64::from(record.config_version())).bind(record.revision() as i64).bind(record.config_json()).bind(record.updated_at() as i64)
                .bind(record.instance().driver_key().as_str()).bind(record.instance().instance_id().as_str()).bind(i64::try_from(expected_revision).map_err(|_| invalid())?)
                .execute(&self.pool).await.map_err(database_error)?;
            if result.rows_affected() != 1 {
                return Err(StorageError::new(StorageErrorKind::Conflict).into());
            }
            Ok(())
        })
    }
    fn remove(&self, instance: DriverInstanceRef) -> DriverFuture<'_, RemoveInstanceOutcome> {
        Box::pin(async move {
            let result = sqlx::query(
                "DELETE FROM vfs_instance_records WHERE driver_key = ? AND instance_id = ?",
            )
            .bind(instance.driver_key().as_str())
            .bind(instance.instance_id().as_str())
            .execute(&self.pool)
            .await
            .map_err(database_error)?;
            Ok(if result.rows_affected() == 0 {
                RemoveInstanceOutcome::AlreadyAbsent
            } else {
                RemoveInstanceOutcome::Removed
            })
        })
    }
}
