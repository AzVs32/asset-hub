use ah_vfs::{
    application::{
        dto::InstanceConfig,
        port::{DriverFuture, InstanceRegistry},
    },
    domain::InstanceId,
    testing::{RegistryFixture, assert_registry_contract},
};
use std::sync::Arc;
mod common;

struct Fixture {
    registry: Arc<dyn InstanceRegistry>,
    pool: sqlx::SqlitePool,
}

impl RegistryFixture for Fixture {
    fn registry(&self) -> Arc<dyn InstanceRegistry> {
        self.registry.clone()
    }
    fn config(&self) -> InstanceConfig {
        InstanceConfig::from([
            ("capacity_size".into(), 1.into()),
            ("capacity_unit".into(), "MiB".into()),
        ])
    }
    fn disable_storage(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            // Memory has no external storage resource to disable.
            Ok(())
        })
    }
    fn corrupt(&self, id: InstanceId) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query("UPDATE vfs_instance_records SET config_json = '{}' WHERE instance_id = ?")
                .bind(id.as_str())
                .execute(&self.pool)
                .await
                .map_err(ah_vfs::infrastructure::sqlite_error::database_error)?;
            Ok(())
        })
    }
    fn close_database(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            self.pool.close().await;
            Ok(())
        })
    }
}

#[tokio::test]
async fn memory_registry_conforms_in_file_and_memory_databases()
-> Result<(), Box<dyn std::error::Error>> {
    for file in [true, false] {
        let directory = tempfile::tempdir()?;
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(if file {
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(directory.path().join("records.sqlite"))
                    .create_if_missing(true)
            } else {
                "sqlite::memory:".parse()?
            })
            .await?;
        let driver = vfs_driver_memory::MemoryDriver::new()?;
        assert_registry_contract(&Fixture {
            registry: Arc::new(common::registry(&driver, pool.clone()).await?),
            pool,
        })
        .await?;
    }
    Ok(())
}
