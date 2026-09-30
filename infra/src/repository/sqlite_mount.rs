use std::io;
use std::path::Path;
use std::time::Duration;

use asset_vfs::driver::{DriverKind, DriverPath};
use asset_vfs::error::VfsError;
use asset_vfs::mount::{Mount, MountError, MountId, MountRepository, MountTransaction};
use asset_vfs::namespace::VirtualPath;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteRow};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations/sqlite");

/// A file-backed SQLite repository for mount definitions.
///
/// The pool owns the database connections. Multiple instances may open the
/// same file; SQLite transactions enforce uniqueness across them.
pub struct SqliteMountRepository {
    pool: SqlitePool,
}

impl SqliteMountRepository {
    /// Opens or creates a SQLite database at `path` with the current schema.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, VfsError> {
        if path.as_ref().as_os_str().is_empty() {
            return Err(MountError::backend(io::Error::new(
                io::ErrorKind::InvalidInput,
                "SQLite database path must not be empty",
            ))
            .into());
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePool::connect_with(options)
            .await
            .map_err(MountError::backend)?;
        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(MountError::backend(error).into());
        }
        Ok(Self { pool })
    }
}

#[async_trait::async_trait]
impl MountRepository for SqliteMountRepository {
    async fn insert(&self, mount: &Mount) -> Result<(), VfsError> {
        let mut transaction = self.begin().await?;
        transaction.insert(mount).await?;
        transaction.commit().await
    }

    async fn remove(&self, id: MountId) -> Result<bool, VfsError> {
        let result = sqlx::query("DELETE FROM mounts WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(MountError::backend)?;
        Ok(result.rows_affected() != 0)
    }

    async fn get(&self, id: MountId) -> Result<Option<Mount>, VfsError> {
        let row = sqlx::query(
            "SELECT id, virtual_path, driver_kind, driver_path, enabled
             FROM mounts WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(MountError::backend)?;
        row.map(|row| RawMount::from_row(&row)?.decode())
            .transpose()
            .map_err(Into::into)
    }

    async fn list(&self) -> Result<Vec<Mount>, VfsError> {
        let rows = sqlx::query(
            "SELECT id, virtual_path, driver_kind, driver_path, enabled
             FROM mounts ORDER BY virtual_path, enabled DESC, id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(MountError::backend)?;
        rows.iter()
            .map(|row| RawMount::from_row(row)?.decode())
            .collect::<Result<Vec<_>, MountError>>()
            .map_err(Into::into)
    }

    async fn begin(&self) -> Result<Box<dyn MountTransaction>, VfsError> {
        let transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(MountError::backend)?;
        Ok(Box::new(SqliteMountTransaction { transaction }))
    }
}

struct SqliteMountTransaction {
    transaction: Transaction<'static, Sqlite>,
}

impl SqliteMountTransaction {
    async fn write(&mut self, mount: &Mount, update: bool) -> Result<bool, VfsError> {
        let id = mount.id().to_string();
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM mounts WHERE id = ?1")
            .bind(&id)
            .fetch_optional(&mut *self.transaction)
            .await
            .map_err(MountError::backend)?;
        match (update, exists.is_some()) {
            (false, true) => return Err(MountError::DuplicateId(mount.id()).into()),
            (true, false) => return Ok(false),
            _ => {}
        }
        if mount.enabled() {
            let conflict: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM mounts WHERE virtual_path = ?1 AND enabled = 1 AND id != ?2",
            )
            .bind(mount.virtual_path().as_str())
            .bind(&id)
            .fetch_optional(&mut *self.transaction)
            .await
            .map_err(MountError::backend)?;
            if conflict.is_some() {
                return Err(MountError::DuplicatePath(mount.virtual_path().clone()).into());
            }
        }
        let statement = if update {
            "UPDATE mounts SET virtual_path = ?2, driver_kind = ?3, driver_path = ?4, enabled = ?5 WHERE id = ?1"
        } else {
            "INSERT INTO mounts (id, virtual_path, driver_kind, driver_path, enabled) VALUES (?1, ?2, ?3, ?4, ?5)"
        };
        sqlx::query(statement)
            .bind(id)
            .bind(mount.virtual_path().as_str())
            .bind(mount.driver().as_str())
            .bind(mount.driver_path().as_str())
            .bind(i64::from(mount.enabled()))
            .execute(&mut *self.transaction)
            .await
            .map_err(MountError::backend)?;
        Ok(true)
    }
}

#[async_trait::async_trait]
impl MountTransaction for SqliteMountTransaction {
    async fn list(&mut self) -> Result<Vec<Mount>, VfsError> {
        let rows = sqlx::query("SELECT id, virtual_path, driver_kind, driver_path, enabled FROM mounts ORDER BY virtual_path, enabled DESC, id")
            .fetch_all(&mut *self.transaction).await.map_err(MountError::backend)?;
        rows.iter()
            .map(|row| RawMount::from_row(row)?.decode())
            .collect::<Result<Vec<_>, MountError>>()
            .map_err(Into::into)
    }

    async fn insert(&mut self, mount: &Mount) -> Result<(), VfsError> {
        self.write(mount, false).await.map(|_| ())
    }

    async fn update(&mut self, mount: &Mount) -> Result<bool, VfsError> {
        self.write(mount, true).await
    }

    async fn commit(self: Box<Self>) -> Result<(), VfsError> {
        self.transaction
            .commit()
            .await
            .map_err(MountError::backend)?;
        Ok(())
    }
}

struct RawMount {
    id: String,
    virtual_path: String,
    driver_kind: String,
    driver_path: String,
    enabled: i64,
}

impl RawMount {
    fn from_row(row: &SqliteRow) -> Result<Self, MountError> {
        Ok(Self {
            id: row.try_get("id").map_err(MountError::backend)?,
            virtual_path: row.try_get("virtual_path").map_err(MountError::backend)?,
            driver_kind: row.try_get("driver_kind").map_err(MountError::backend)?,
            driver_path: row.try_get("driver_path").map_err(MountError::backend)?,
            enabled: row.try_get("enabled").map_err(MountError::backend)?,
        })
    }

    fn decode(self) -> Result<Mount, MountError> {
        let id = self
            .id
            .parse::<MountId>()
            .map_err(|_| MountError::InvalidStoredMount { column: "id" })?;
        let virtual_path = VirtualPath::try_from(self.virtual_path.as_str()).map_err(|_| {
            MountError::InvalidStoredMount {
                column: "virtual_path",
            }
        })?;
        let driver_kind = DriverKind::try_from(self.driver_kind.as_str()).map_err(|_| {
            MountError::InvalidStoredMount {
                column: "driver_kind",
            }
        })?;
        let enabled = match self.enabled {
            0 => false,
            1 => true,
            _ => {
                return Err(MountError::InvalidStoredMount { column: "enabled" });
            }
        };

        Ok(Mount::new(
            id,
            virtual_path,
            driver_kind,
            DriverPath::new(self.driver_path),
            enabled,
        ))
    }
}
