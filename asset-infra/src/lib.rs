pub mod action;
mod builtin_catalog;
pub mod config;
mod directory_index;
pub mod kind;
pub mod migration;
pub mod password;
pub mod plugin;
mod plugin_manifest;
pub mod sqlite;
pub mod storage;

/// Shared filesystem boundary for installing, uninstalling, and loading plugin packages.
pub mod plugin_package {
    pub use crate::plugin_manifest::{
        InstalledPluginPackage, LoadedPlugin, MAX_PLUGIN_LOCK_BYTES, MAX_PLUGIN_MANIFEST_BYTES,
        MAX_PLUGIN_WASM_BYTES, MAX_PLUGIN_WEB_BYTES, PluginCatalog, generate_plugin_manifest_lock,
        install_plugin_package, load_verified_plugin_package, uninstall_plugin_package,
    };
}

use asset_core::{
    CoreError, port::BlobStorage, port::DirectoryIndex, port::DirectoryQuery,
    port::DirectoryRepository, port::DirectoryStorage, port::ResourceContentReplacementRepository,
    port::ResourceQuery, port::ResourceRepository, port::StorageScanner,
    port::UploadSessionRepository, port::UserQuery, port::UserRepository,
};
use config::{AssetInfraConfig, BlobBackend, DatabaseBackend};
use directory_index::InMemoryDirectoryIndex;
use sqlite::{
    SqliteIdentityRepository, SqliteResourceContentReplacementRepository, SqliteResourceRepository,
    SqliteUploadSessionRepository,
};
use std::sync::Arc;
use std::time::Instant;
use storage::{FileSystemScanner, OpenDalBlobStorage};

/// 根据配置的后端选型初始化具体基础设施适配器。
///
/// 当前支持 SQLite 数据库和本地 Blob 存储。插件 catalog、运行时执行器和 Core service
/// 由 `asset-runtime` 按确定顺序装配。
pub struct AssetInfrastructure {
    /// 实际生效的基础设施配置。
    config: AssetInfraConfig,
    /// SQLite 聚合持久化适配器，对外分别实现资源与目录仓储端口。
    resource_repository: Arc<SqliteResourceRepository>,
    directory_index: Arc<InMemoryDirectoryIndex>,
    identity_repository: Arc<SqliteIdentityRepository>,
    upload_session_repository: Arc<SqliteUploadSessionRepository>,
    content_replacement_repository: Arc<SqliteResourceContentReplacementRepository>,
    /// 对象存储适配器。
    blob_storage: Arc<OpenDalBlobStorage>,
    storage_scanner: Arc<FileSystemScanner>,
}

impl AssetInfrastructure {
    /// 使用给定配置创建基础设施组合。
    ///
    /// 调用方可以传入 `AssetInfraConfig::default()` 使用默认本地配置。
    pub async fn new(config: AssetInfraConfig) -> Result<Self, CoreError> {
        let config = config.normalized()?;
        let (blob_storage, storage_scanner) = match config.blob.backend {
            BlobBackend::Local => (
                Arc::new(OpenDalBlobStorage::from_local_root(
                    &config.blob.local.root,
                )?),
                Arc::new(FileSystemScanner::new(config.blob.local.root.clone())),
            ),
        };
        let sqlite_started = Instant::now();
        let resource_repository = match config.database.backend {
            DatabaseBackend::Sqlite => {
                let sqlite_path = config.sqlite_path();
                Arc::new(
                    SqliteResourceRepository::connect(
                        &sqlite_path,
                        config.database.sqlite.max_connections,
                    )
                    .await?,
                )
            }
        };
        tracing::info!(
            elapsed_ms = sqlite_started.elapsed().as_millis(),
            "SQLite initialized"
        );
        let directory_index = Arc::new(InMemoryDirectoryIndex::from_directories(
            resource_repository.load_all().await?,
        )?);
        let identity_repository = Arc::new(SqliteIdentityRepository::new(
            resource_repository.pool().clone(),
        ));
        let upload_session_repository = Arc::new(SqliteUploadSessionRepository::new(
            resource_repository.pool().clone(),
        ));
        let content_replacement_repository = Arc::new(
            SqliteResourceContentReplacementRepository::new(resource_repository.pool().clone()),
        );
        Ok(Self {
            config,
            resource_repository,
            directory_index,
            identity_repository,
            upload_session_repository,
            content_replacement_repository,
            blob_storage,
            storage_scanner,
        })
    }

    /// 返回实际生效的基础设施配置。
    pub fn config(&self) -> &AssetInfraConfig {
        &self.config
    }

    /// 返回资源仓储端口对象。
    pub fn resource_repository(&self) -> Arc<dyn ResourceRepository> {
        self.resource_repository.clone()
    }

    pub fn resource_query(&self) -> Arc<dyn ResourceQuery> {
        self.resource_repository.clone()
    }

    pub fn directory_repository(&self) -> Arc<dyn DirectoryRepository> {
        self.resource_repository.clone()
    }

    pub fn directory_index(&self) -> Arc<dyn DirectoryIndex> {
        self.directory_index.clone()
    }

    pub fn directory_query(&self) -> Arc<dyn DirectoryQuery> {
        self.directory_index.clone()
    }

    pub fn user_repository(&self) -> Arc<dyn UserRepository> {
        self.identity_repository.clone()
    }

    pub fn user_query(&self) -> Arc<dyn UserQuery> {
        self.identity_repository.clone()
    }

    /// 返回对象存储端口对象。
    pub fn blob_storage(&self) -> Arc<dyn BlobStorage> {
        self.blob_storage.clone()
    }

    pub fn directory_storage(&self) -> Arc<dyn DirectoryStorage> {
        self.blob_storage.clone()
    }

    pub fn storage_scanner(&self) -> Arc<dyn StorageScanner> {
        self.storage_scanner.clone()
    }

    pub fn upload_session_repository(&self) -> Arc<dyn UploadSessionRepository> {
        self.upload_session_repository.clone()
    }

    pub fn content_replacement_repository(&self) -> Arc<dyn ResourceContentReplacementRepository> {
        self.content_replacement_repository.clone()
    }
}
