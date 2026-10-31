//! One SQLite repository for persistent files and isolated in-memory databases.

use crate::application::dto::MountFilter;
use crate::application::error::MountRepositoryError;
use crate::application::port::{MountRepository, MountRepositoryFuture};
use crate::domain::{DriverInstanceRef, DriverKey, InstanceId, Mount, MountId, VirtualPath};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow};
use sqlx::{Connection, Row, SqliteConnection, SqlitePool};
use std::{error::Error, io, path::Path};

/// Persists one mount namespace in the `vfs_mounts` table.
///
/// File and memory modes share the same schema, queries and error mapping.
/// Mount nesting and required-root rules remain in the domain and application
/// layers. This adapter stores only mount definitions, not driver settings.
pub struct SqliteMountRepository {
    pool: SqlitePool,
    // Keep named memory databases alive even if a pooled connection is replaced.
    _memory_keepalive: Option<SqliteConnection>,
}

impl SqliteMountRepository {
    /// Open or create a SQLite file, then initialize the mount table.
    ///
    /// The caller resolves relative paths and creates parent directories.
    /// Use [`Self::in_memory`] for a database without a backing file.
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, MountRepositoryError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() || path == Path::new(":memory:") {
            return Err(backend(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a nonempty SQLite file path; use in_memory for memory storage",
            )));
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .map_err(backend)?;
        Self::from_pool(pool).await
    }

    /// Create a fresh, isolated SQLite memory database without writing a file.
    ///
    /// A dedicated connection retains the database for this repository's
    /// lifetime. Pooled connections use the same uniquely named memory database.
    pub async fn in_memory() -> Result<Self, MountRepositoryError> {
        let options: SqliteConnectOptions = "sqlite::memory:".parse().map_err(backend)?;
        let keepalive = SqliteConnection::connect_with(&options)
            .await
            .map_err(backend)?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await
            .map_err(backend)?;
        let mut repository = Self::from_pool(pool).await?;
        repository._memory_keepalive = Some(keepalive);
        Ok(repository)
    }

    /// Initialize mount storage using an application-owned SQLite pool.
    ///
    /// Other tables in the same database are preserved. The host owns pool
    /// configuration and shutdown; closing the pool also stops this repository.
    /// For memory pools, the host must keep the database alive across connection
    /// replacement, or use [`Self::in_memory`].
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, MountRepositoryError> {
        let mut transaction = pool.begin().await.map_err(backend)?;
        sqlx::raw_sql(include_str!("schema.sql"))
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        transaction.commit().await.map_err(backend)?;
        Ok(Self {
            pool,
            _memory_keepalive: None,
        })
    }
}

impl MountRepository for SqliteMountRepository {
    fn get(&self, id: MountId) -> MountRepositoryFuture<'_, Option<Mount>> {
        Box::pin(async move {
            sqlx::query(
                "SELECT id, virtual_path, driver_key, instance_id, enabled \
                 FROM vfs_mounts WHERE id = ?",
            )
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?
            .map(decode)
            .transpose()
        })
    }

    fn list(&self, filter: MountFilter) -> MountRepositoryFuture<'_, Vec<Mount>> {
        Box::pin(async move {
            let key = filter.get_driver_key().map(DriverKey::as_str);
            let enabled = filter.get_enabled();
            sqlx::query(
                "SELECT id, virtual_path, driver_key, instance_id, enabled \
                 FROM vfs_mounts \
                 WHERE (? IS NULL OR driver_key = ?) \
                   AND (? IS NULL OR enabled = ?) \
                 ORDER BY virtual_path COLLATE BINARY",
            )
            .bind(key)
            .bind(key)
            .bind(enabled)
            .bind(enabled)
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?
            .into_iter()
            .map(decode)
            .collect()
        })
    }

    fn insert(&self, mount: Mount) -> MountRepositoryFuture<'_, ()> {
        Box::pin(async move {
            let result = sqlx::query(
                "INSERT INTO vfs_mounts (id, virtual_path, driver_key, instance_id, enabled) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(mount.id().to_string())
            .bind(mount.virtual_path().as_str())
            .bind(mount.driver_instance().driver_key().as_str())
            .bind(mount.driver_instance().instance_id().as_str())
            .bind(mount.enabled())
            .execute(&self.pool)
            .await;
            match result {
                Ok(_) => Ok(()),
                Err(error) => match error.as_database_error().and_then(|error| error.code()) {
                    // SQLite extended result codes for primary-key and unique violations.
                    Some(code) if code == "1555" => {
                        Err(MountRepositoryError::DuplicateId(mount.id()))
                    }
                    Some(code) if code == "2067" => Err(MountRepositoryError::PathOccupied(
                        mount.virtual_path().clone(),
                    )),
                    _ => Err(backend(error)),
                },
            }
        })
    }

    fn remove(&self, id: MountId) -> MountRepositoryFuture<'_, ()> {
        Box::pin(async move {
            let result = sqlx::query("DELETE FROM vfs_mounts WHERE id = ?")
                .bind(id.to_string())
                .execute(&self.pool)
                .await
                .map_err(backend)?;
            require_found(result.rows_affected(), id)
        })
    }

    fn set_enabled(&self, id: MountId, enabled: bool) -> MountRepositoryFuture<'_, ()> {
        Box::pin(async move {
            let result = sqlx::query("UPDATE vfs_mounts SET enabled = ? WHERE id = ?")
                .bind(enabled)
                .bind(id.to_string())
                .execute(&self.pool)
                .await
                .map_err(backend)?;
            require_found(result.rows_affected(), id)
        })
    }
}

fn require_found(affected: u64, id: MountId) -> Result<(), MountRepositoryError> {
    if affected == 0 {
        Err(MountRepositoryError::NotFound(id))
    } else {
        Ok(())
    }
}

fn decode(row: SqliteRow) -> Result<Mount, MountRepositoryError> {
    let id: String = row.try_get("id").map_err(invalid_data)?;
    let path: String = row.try_get("virtual_path").map_err(invalid_data)?;
    let key: String = row.try_get("driver_key").map_err(invalid_data)?;
    let instance: String = row.try_get("instance_id").map_err(invalid_data)?;
    let enabled: i64 = row.try_get("enabled").map_err(invalid_data)?;
    let virtual_path = VirtualPath::parse(&path).map_err(invalid_data)?;
    if virtual_path.as_str() != path || !matches!(enabled, 0 | 1) {
        return Err(invalid_data(io::Error::new(
            io::ErrorKind::InvalidData,
            "stored mount path is not canonical or enabled is not a boolean",
        )));
    }
    Ok(Mount::builder()
        .id(id.parse::<MountId>().map_err(invalid_data)?)
        .virtual_path(virtual_path)
        .driver_instance(DriverInstanceRef::new(
            DriverKey::parse(&key).map_err(invalid_data)?,
            InstanceId::parse(&instance).map_err(invalid_data)?,
        ))
        .enabled(enabled == 1)
        .build())
}

fn backend(error: impl Error + Send + Sync + 'static) -> MountRepositoryError {
    MountRepositoryError::Backend(Box::new(error))
}

fn invalid_data(error: impl Error + Send + Sync + 'static) -> MountRepositoryError {
    MountRepositoryError::InvalidData(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_data_survives_replacing_the_pooled_connection() -> Result<(), Box<dyn Error>> {
        let repository = SqliteMountRepository::in_memory().await?;
        let file: String =
            sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
                .fetch_one(&repository.pool)
                .await?;
        assert!(file.is_empty());
        let id = MountId::new();
        repository
            .insert(
                Mount::builder()
                    .id(id)
                    .virtual_path(VirtualPath::root())
                    .driver_instance(DriverInstanceRef::new(
                        DriverKey::parse("builtin.local")?,
                        InstanceId::parse("root")?,
                    ))
                    .enabled(true)
                    .build(),
            )
            .await?;
        repository.pool.acquire().await?.close().await?;
        assert!(repository.get(id).await?.is_some());
        Ok(())
    }
}
