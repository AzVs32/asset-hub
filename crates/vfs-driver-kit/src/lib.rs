//! Typed instance records over existing SQLite tables; no storage handles are opened.

use ah_vfs::{
    application::dto::InstanceQuery,
    domain::{DriverError, InstanceId, StorageError, StorageErrorKind},
};
use sqlx::{
    Row, Sqlite, SqlitePool,
    query::Query,
    sqlite::{SqliteArguments, SqliteRow},
};
use std::marker::PhantomData;

/// Insert statement passed to a typed configuration binder; values stay bound parameters.
pub type SqliteInsert<'a> = Query<'a, Sqlite, SqliteArguments>;

/// Driver-owned mapping of validated configuration to an existing SQLite layout.
/// Identifiers are trusted static declarations, additionally checked by the store.
/// Decode validates every persisted field without contacting backend storage.
/// Bind appends exactly COLUMNS.len() values, excluding the instance ID.
pub trait InstanceConfigRow: Send + Sync + Sized {
    const TABLE: &'static str;
    const COLUMNS: &'static [&'static str];
    const SCHEMA: &'static str;
    fn decode(row: &SqliteRow) -> Result<Self, DriverError>;
    fn bind<'a>(&'a self, query: SqliteInsert<'a>) -> Result<SqliteInsert<'a>, DriverError>;
}

/// A validated identity paired with driver-owned, strongly typed configuration.
pub struct StoredInstance<C> {
    id: InstanceId,
    config: C,
}
impl<C> StoredInstance<C> {
    pub fn new(id: InstanceId, config: C) -> Self {
        Self { id, config }
    }
    pub fn id(&self) -> &InstanceId {
        &self.id
    }
    pub fn config(&self) -> &C {
        &self.config
    }
}

/// A decoded live page with an exclusive ID cursor only when more records exist.
pub struct StoredInstancePage<C> {
    pub instances: Vec<StoredInstance<C>>,
    pub next_after: Option<InstanceId>,
}

pub use ah_vfs::domain::new_instance_id;

/// Generic SQLite persistence preserving each driver's original table and columns.
/// File and memory pools share the same implementation. Pool ownership stays with
/// the host; initialize creates missing tables without replacing existing records.
pub struct InstanceStore<C> {
    pool: SqlitePool,
    _config: PhantomData<fn() -> C>,
}
impl<C: InstanceConfigRow> InstanceStore<C> {
    /// Validate static identifiers and initialize the existing table without replacing data.
    /// Only validated identifiers are interpolated; all record values stay bound.
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, DriverError> {
        for name in std::iter::once(C::TABLE).chain(C::COLUMNS.iter().copied()) {
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err(StorageError::new(StorageErrorKind::InvalidData).into());
            }
        }
        let mut tx = pool.begin().await.map_err(database_error)?;
        sqlx::raw_sql(C::SCHEMA)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        tx.commit().await.map_err(database_error)?;
        Ok(Self {
            pool,
            _config: PhantomData,
        })
    }
    fn columns() -> String {
        std::iter::once("id")
            .chain(C::COLUMNS.iter().copied())
            .collect::<Vec<_>>()
            .join(", ")
    }
    fn decode(row: SqliteRow) -> Result<StoredInstance<C>, DriverError> {
        let raw: String = row.try_get("id").map_err(database_error)?;
        let id = InstanceId::parse(&raw)
            .map_err(|source| StorageError::with_source(StorageErrorKind::InvalidData, source))?;
        Ok(StoredInstance::new(id, C::decode(&row)?))
    }
    /// Read a registration. Database failures are never converted to absence.
    pub async fn get(&self, id: &InstanceId) -> Result<Option<StoredInstance<C>>, DriverError> {
        let sql = format!("SELECT {} FROM {} WHERE id = ?", Self::columns(), C::TABLE);
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?
            .map(Self::decode)
            .transpose()
    }
    /// Decode the entire fetched page, including lookahead, before truncating.
    pub async fn list(&self, query: &InstanceQuery) -> Result<StoredInstancePage<C>, DriverError> {
        let sql = format!(
            "SELECT {} FROM {} WHERE (? IS NULL OR id COLLATE BINARY > ?) ORDER BY id COLLATE BINARY LIMIT ?",
            Self::columns(),
            C::TABLE
        );
        let after = query.after().map(InstanceId::as_str);
        let mut instances = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(after)
            .bind(after)
            .bind((query.limit().get() + 1) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?
            .into_iter()
            .map(Self::decode)
            .collect::<Result<Vec<_>, _>>()?;
        let more = instances.len() > query.limit().get();
        instances.truncate(query.limit().get());
        let next_after = more
            .then(|| instances.last().map(|row| row.id.clone()))
            .flatten();
        Ok(StoredInstancePage {
            instances,
            next_after,
        })
    }
    /// Allocate an ID and insert one validated configuration. No backend opens occur.
    pub async fn insert(&self, config: C) -> Result<StoredInstance<C>, DriverError> {
        let record = StoredInstance::new(new_instance_id()?, config);
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            C::TABLE,
            Self::columns(),
            vec!["?"; C::COLUMNS.len() + 1].join(", ")
        );
        record
            .config
            .bind(sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(record.id.as_str()))?
            .execute(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(record)
    }
    /// Delete by ID without decoding configuration. False confirms prior absence.
    pub async fn remove(&self, id: &InstanceId) -> Result<bool, DriverError> {
        let sql = format!("DELETE FROM {} WHERE id = ?", C::TABLE);
        Ok(sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(database_error)?
            .rows_affected()
            != 0)
    }
}

pub use ah_vfs::infrastructure::sqlite_error::{database_error, io_error_kind};
