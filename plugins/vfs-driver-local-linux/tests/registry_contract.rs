#![cfg(target_os = "linux")]
use ah_vfs::{
    application::{
        dto::InstanceConfig,
        port::{Driver, DriverFuture, InstanceRegistry},
    },
    domain::InstanceId,
    testing::{RegistryFixture, assert_registry_contract},
};
use std::sync::Arc;

struct Fixture {
    registry: Arc<dyn InstanceRegistry>,
    pool: sqlx::SqlitePool,
    root: std::path::PathBuf,
}

impl RegistryFixture for Fixture {
    fn registry(&self) -> Arc<dyn InstanceRegistry> {
        self.registry.clone()
    }
    fn config(&self) -> InstanceConfig {
        InstanceConfig::from([(
            "root_path".into(),
            self.root.to_string_lossy().into_owned().into(),
        )])
    }
    fn disable_storage(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            std::fs::remove_dir(&self.root).map_err(|source| {
                ah_vfs::domain::StorageError::with_source(
                    ah_vfs::domain::StorageErrorKind::Backend,
                    source,
                )
            })?;
            Ok(())
        })
    }
    fn corrupt(&self, id: InstanceId) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query("UPDATE local_instances SET root_path = 'relative' WHERE id = ?")
                .bind(id.as_str())
                .execute(&self.pool)
                .await
                .map_err(vfs_driver_kit::database_error)?;
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
async fn local_registry_conforms_in_file_and_memory_databases()
-> Result<(), Box<dyn std::error::Error>> {
    for file in [true, false] {
        let directory = tempfile::tempdir()?;
        let root = directory.path().join("root");
        std::fs::create_dir(&root)?;
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
        let driver = vfs_driver_local_linux::LinuxLocalDriver::from_pool(pool.clone()).await?;
        assert_registry_contract(&Fixture {
            registry: driver.instances().registry()?,
            pool,
            root,
        })
        .await?;
    }
    Ok(())
}
