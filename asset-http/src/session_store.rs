use crate::settings::DEFAULT_SESSION_SQLITE_PATH;
use async_trait::async_trait;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tower_sessions::session_store::ExpiredDeletion;
use tower_sessions_sqlx_store::SqliteStore;
use tower_sessions_sqlx_store::sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions,
};
use tower_sessions_sqlx_store::sqlx::{self, SqlitePool};

const SESSION_TABLE: &str = "http_sessions";
const SESSION_SQLITE_MAX_CONNECTIONS: u32 = 5;
const SESSION_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Owns the HTTP-specific SQLite store and its expired-session cleanup task.
pub struct HttpSessionRuntime {
    store: SqliteStore,
    #[cfg(test)]
    pool: SqlitePool,
    health: SessionStoreHealth,
    cleanup_task: JoinHandle<()>,
}

impl HttpSessionRuntime {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Self::open(DEFAULT_SESSION_SQLITE_PATH).await
    }

    /// Opens the fixed-format HTTP session store at an explicit host-provided path.
    ///
    /// The `asset-http` executable uses [`Self::new`]. This entry point lets embedders and tests
    /// select an isolated filesystem location without changing the executable configuration.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "HTTP session SQLite path must not be empty",
            )
            .into());
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let connect_options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(SESSION_SQLITE_MAX_CONNECTIONS)
            .connect_with(connect_options)
            .await?;
        let store = SqliteStore::new(pool.clone()).with_table_name(SESSION_TABLE)?;
        store.migrate().await?;

        let cleanup_store = store.clone();
        let cleanup_task = tokio::spawn(async move {
            if let Err(error) = cleanup_store
                .continuously_delete_expired(SESSION_CLEANUP_INTERVAL)
                .await
            {
                tracing::error!(%error, "HTTP session cleanup task stopped");
            }
        });

        Ok(Self {
            store,
            #[cfg(test)]
            pool: pool.clone(),
            health: SessionStoreHealth {
                inner: Arc::new(SqliteSessionStoreHealth { pool }),
            },
            cleanup_task,
        })
    }

    pub fn store(&self) -> SqliteStore {
        self.store.clone()
    }

    pub fn health(&self) -> SessionStoreHealth {
        self.health.clone()
    }
}

impl Drop for HttpSessionRuntime {
    fn drop(&mut self) {
        self.cleanup_task.abort();
    }
}

/// Cloneable health probe for the HTTP-owned session database.
#[derive(Clone)]
pub struct SessionStoreHealth {
    inner: Arc<dyn SessionStoreHealthCheck>,
}

impl SessionStoreHealth {
    pub async fn check(&self) -> Result<(), String> {
        self.inner.check().await
    }
}

#[async_trait]
trait SessionStoreHealthCheck: Send + Sync {
    async fn check(&self) -> Result<(), String>;
}

struct SqliteSessionStoreHealth {
    pool: SqlitePool,
}

#[async_trait]
impl SessionStoreHealthCheck for SqliteSessionStoreHealth {
    async fn check(&self) -> Result<(), String> {
        sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use time::OffsetDateTime;
    use tower_sessions::session::Record;
    use tower_sessions::session_store::SessionStore;

    #[tokio::test]
    async fn creates_an_isolated_session_schema_with_its_own_pool_configuration() {
        let root = tempdir().unwrap();
        let path = root.path().join("nested/http-session.sqlite");
        let runtime = HttpSessionRuntime::open(&path).await.unwrap();

        assert!(path.is_file());
        assert_eq!(
            runtime.pool.options().get_max_connections(),
            SESSION_SQLITE_MAX_CONNECTIONS
        );
        let tables = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
        )
        .fetch_all(&runtime.pool)
        .await
        .unwrap();
        assert!(tables.iter().any(|table| table == SESSION_TABLE));
        assert!(!tables.iter().any(|table| table == "resources"));
        runtime.health().check().await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_runtime_boundaries() {
        let error = match HttpSessionRuntime::open("").await {
            Ok(_) => panic!("an empty session database path must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("path must not be empty"));
    }

    #[tokio::test]
    async fn sessions_survive_recreating_the_http_runtime() {
        let root = tempdir().unwrap();
        let path = root.path().join("http-session.sqlite");
        let session_id = {
            let runtime = HttpSessionRuntime::open(&path).await.unwrap();
            let store = runtime.store();
            let mut record = Record {
                id: Default::default(),
                data: Default::default(),
                expiry_date: OffsetDateTime::now_utc() + time::Duration::hours(1),
            };
            store.create(&mut record).await.unwrap();
            record.id
        };

        let reopened = HttpSessionRuntime::open(&path).await.unwrap();
        assert!(reopened.store().load(&session_id).await.unwrap().is_some());
    }
}
