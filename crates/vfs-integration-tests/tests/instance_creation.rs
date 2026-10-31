use ah_vfs::application::mount_runtime::MountRuntime;
mod common;

use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        dto::{InstanceConfig, InstanceQuery},
        error::InstanceError,
        port::Driver,
        service::InstanceService,
    },
    domain::{DriverError, DriverInstanceRef, InstanceConfigErrorKind, StorageErrorKind},
    infrastructure::mount_repository::SqliteMountRepository,
};
use common::*;
use serde_json::json;
use std::{error::Error, sync::Arc};
use vfs_driver_memory::{CapacityUnit, MemoryDriver};

struct Fixture {
    driver: Arc<dyn Driver>,
    service: InstanceService,
    pool: sqlx::SqlitePool,
    root: tempfile::TempDir,
    memory: bool,
}

impl Fixture {
    async fn new(memory: bool) -> TestResult<Self> {
        let pool = pool().await?;
        let driver: Arc<dyn Driver> = if memory {
            Arc::new(MemoryDriver::from_pool(pool.clone()).await?)
        } else {
            Arc::new(vfs_driver_local::LocalDriver::from_pool(pool.clone()).await?)
        };
        let repository = Arc::new(SqliteMountRepository::from_pool(pool.clone()).await?);
        let mut catalog = DriverCatalog::new();
        ah_vfs::testing::register_driver(&mut catalog, driver.clone())?;
        Ok(Self {
            driver,
            service: InstanceService::new(Arc::new(MountRuntime::new(
                repository,
                Arc::new(catalog),
            ))),
            pool,
            root: tempfile::tempdir()?,
            memory,
        })
    }

    fn config(&self) -> TestResult<InstanceConfig> {
        if self.memory {
            Ok(memory_config(8, CapacityUnit::MiB))
        } else {
            local_config(self.root.path())
        }
    }

    async fn assert_empty(&self) -> TestResult {
        assert!(
            self.driver
                .instances()
                .registry()?
                .list(InstanceQuery::default())
                .await?
                .instances
                .is_empty()
        );
        Ok(())
    }
}

#[tokio::test]
async fn create_discovery_and_views_use_only_generic_service_contracts() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        let key = f.driver.info().key();
        let schema = f
            .service
            .instance_config_schema(key)?
            .ok_or("creation schema")?;
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        let config = f.config()?;
        for field in config.keys() {
            assert!(schema["properties"].get(field).is_some());
            assert!(
                schema["required"]
                    .as_array()
                    .ok_or("required array")?
                    .contains(&json!(field))
            );
        }
        let created = f.service.create_instance(key, config.clone()).await?;
        let reference = DriverInstanceRef::new(key.clone(), created.id.clone());
        assert_eq!(
            f.service.get_instance(&reference).await?,
            Some(created.clone())
        );
        assert_eq!(
            f.service
                .list_instances(key, InstanceQuery::default())
                .await?
                .instances,
            vec![created.clone()]
        );
        let second = f.service.create_instance(key, config).await?;
        assert_ne!(created.id, second.id);
        if memory {
            assert_eq!(created.details["capacity_size"], 8);
            assert_eq!(created.details["capacity_unit"], "MiB");
        } else {
            assert_eq!(
                created.details["root_path"],
                json!(std::fs::canonicalize(f.root.path())?.to_str())
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn missing_unknown_and_mistyped_parameters_fail_before_registration() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        let fields: &[&str] = if memory {
            &["capacity_size", "capacity_unit"]
        } else {
            &["root_path"]
        };
        for field in fields {
            for value in [
                None,
                Some(json!(null)),
                Some(json!({"secret": "private-input-value"})),
            ] {
                let mut config = f.config()?.into_object();
                let kind = if let Some(value) = value {
                    config.insert((*field).into(), value);
                    InstanceConfigErrorKind::InvalidType
                } else {
                    config.remove(*field);
                    InstanceConfigErrorKind::MissingField
                };
                let error = f
                    .service
                    .create_instance(f.driver.info().key(), config.into())
                    .await
                    .err()
                    .ok_or("invalid config")?;
                assert!(!error.to_string().contains("private-input-value"));
                assert!(
                    matches!(error, InstanceError::Driver(DriverError::InvalidInstanceConfig { field: actual, kind: actual_kind }) if actual == *field && actual_kind == kind)
                );
            }
        }
        let mut config = f.config()?.into_object();
        config.insert("unexpected".into(), "private-input-value".into());
        assert!(
            matches!(f.driver.instances().registry()?.create(config.into()).await,
            Err(DriverError::InvalidInstanceConfig { field, kind: InstanceConfigErrorKind::UnknownField }) if field == "unexpected")
        );
        f.assert_empty().await?;
    }
    Ok(())
}

#[tokio::test]
async fn memory_rejects_invalid_numeric_ranges_and_units_without_lossy_conversion() -> TestResult {
    let f = Fixture::new(true).await?;
    for size in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(u64::MAX),
        json!(i64::MAX),
    ] {
        let mut config = f.config()?.into_object();
        config.insert("capacity_size".into(), size);
        assert!(
            matches!(f.driver.instances().registry()?.create(config.into()).await,
            Err(DriverError::InvalidInstanceConfig { field, kind: InstanceConfigErrorKind::InvalidValue }) if field == "capacity_size")
        );
    }
    let mut config = f.config()?.into_object();
    config.insert("capacity_unit".into(), "MB".into());
    assert!(
        matches!(f.driver.instances().registry()?.create(config.into()).await,
        Err(DriverError::InvalidInstanceConfig { field, kind: InstanceConfigErrorKind::InvalidValue }) if field == "capacity_unit")
    );
    f.assert_empty().await?;
    Ok(())
}

#[tokio::test]
async fn local_rejects_invalid_paths_and_preserves_backend_error_categories() -> TestResult {
    let f = Fixture::new(false).await?;
    for path in ["", "invalid\0path"] {
        assert!(
            matches!(f.driver.instances().registry()?.create(InstanceConfig::from([("root_path".into(), path.into())])).await,
            Err(DriverError::InvalidInstanceConfig { field, kind: InstanceConfigErrorKind::InvalidValue }) if field == "root_path")
        );
    }
    let file = f.root.path().join("file");
    std::fs::write(&file, "unchanged")?;
    for (path, kind) in [
        (file, StorageErrorKind::NotDir),
        (f.root.path().join("missing"), StorageErrorKind::NotFound),
    ] {
        let error = f
            .driver
            .instances()
            .registry()?
            .create(local_config(&path)?)
            .await
            .err()
            .ok_or("invalid root")?;
        assert!(matches!(error, DriverError::Storage(error) if error.kind() == kind));
    }
    f.assert_empty().await?;
    Ok(())
}

#[tokio::test]
async fn creation_persistence_failures_preserve_domain_errors_and_sources() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        f.pool.close().await;
        let error = f
            .service
            .create_instance(f.driver.info().key(), f.config()?)
            .await
            .err()
            .ok_or("closed database")?;
        let storage = error.storage_error().ok_or("storage error")?;
        assert_eq!(storage.kind(), StorageErrorKind::Unavailable);
        assert!(storage.source().is_some());
    }
    Ok(())
}
