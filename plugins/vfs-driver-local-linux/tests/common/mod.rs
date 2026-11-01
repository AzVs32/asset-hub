#![cfg(target_os = "linux")]
#![allow(dead_code)]

use ah_vfs::{
    application::dto::InstanceConfig,
    domain::{DriverError, InstanceConfigErrorKind},
};
use std::path::Path;

pub async fn pool() -> Result<sqlx::SqlitePool, Box<dyn std::error::Error>> {
    Ok(sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?)
}

pub fn local_config(root: &Path) -> Result<InstanceConfig, DriverError> {
    let root = root
        .to_str()
        .ok_or_else(|| DriverError::InvalidInstanceConfig {
            field: "root_path".into(),
            kind: InstanceConfigErrorKind::InvalidValue,
        })?;
    Ok(ah_vfs::application::dto::InstanceConfig::from([(
        "root_path".into(),
        root.into(),
    )]))
}

pub async fn registry(
    driver: &dyn ah_vfs::application::port::Driver,
    pool: sqlx::SqlitePool,
) -> Result<
    ah_vfs::application::host_instance_registry::HostInstanceRegistry,
    Box<dyn std::error::Error>,
> {
    let ah_vfs::application::port::InstanceManagement::HostManaged(provisioner) =
        driver.instances()
    else {
        return Err("expected host-managed driver".into());
    };
    Ok(ah_vfs::application::host_instance_registry::HostInstanceRegistry::new(
        driver.info(), provisioner,
        std::sync::Arc::new(ah_vfs::infrastructure::instance_record_repository::SqliteInstanceRecordRepository::from_pool(pool).await?), None))
}
