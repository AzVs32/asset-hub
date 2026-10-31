#![cfg(target_os = "windows")]
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
