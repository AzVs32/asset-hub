//! 基础设施适配器的配置类型。
//!
//! 本模块拥有数据库和 Blob 适配器使用的具体配置类型。这些类型由
//! `asset-runtime::AssetConfig` 组合到 `[asset]` 分区，本模块自身不声明或注册
//! `asset_config::ConfigSection`。

mod blob;
mod database;

pub use blob::{BlobBackend, BlobConfig, LocalBlobConfig, LocalBlobSyncConfig};
pub use database::{DatabaseBackend, DatabaseConfig, SqliteDatabaseConfig};

#[cfg(test)]
mod tests;
