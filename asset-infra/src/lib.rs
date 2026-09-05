pub mod action;
pub mod builtin_catalog;
pub mod config;
mod directory_index;
pub mod kind;
pub mod migration;
pub mod password;
pub mod sqlite;
pub mod storage;

use asset_core::{
    CoreError, port::BlobHealth, port::ContentObjectStore, port::ContentReader,
    port::ContentStagingStore, port::DirectoryProjection, port::DirectoryQuery,
    port::DirectoryRelocationStore, port::DirectoryStorage, port::DirectoryStore,
    port::IdempotencyRepository, port::ResourceContentReplacementRepository,
    port::ResourceMaintenanceReadModel, port::ResourceReadModel, port::ResourceRelocationStore,
    port::ResourceStore, port::StorageScanner, port::UploadSessionRepository, port::UserQuery,
    port::UserRepository,
};
use config::{AssetInfraConfig, BlobBackend, DatabaseBackend};
use directory_index::InMemoryDirectoryIndex;
use sqlite::{
    SqliteDatabase, SqliteDirectoryStore, SqliteIdempotencyRepository, SqliteIdentityRepository,
    SqliteResourceContentReplacementRepository, SqliteResourceStore, SqliteUploadSessionRepository,
};
use std::sync::Arc;
use std::time::Instant;
use storage::{FileSystemScanner, OpenDalBlobStorage};

/// 根据配置的后端选型初始化具体基础设施适配器。
///
/// 当前支持 SQLite 数据库和本地 Blob 存储；Core service 由 `asset-runtime` 装配。
pub struct AssetInfrastructure {
    /// 实际生效的基础设施配置。
    config: AssetInfraConfig,
    resource_store: Arc<SqliteResourceStore>,
    directory_store: Arc<SqliteDirectoryStore>,
    directory_index: Arc<InMemoryDirectoryIndex>,
    identity_repository: Arc<SqliteIdentityRepository>,
    upload_session_repository: Arc<SqliteUploadSessionRepository>,
    content_replacement_repository: Arc<SqliteResourceContentReplacementRepository>,
    idempotency_repository: Arc<SqliteIdempotencyRepository>,
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
        let database = match config.database.backend {
            DatabaseBackend::Sqlite => {
                let sqlite_path = config.sqlite_path();
                SqliteDatabase::connect(&sqlite_path, config.database.sqlite.max_connections)
                    .await?
            }
        };
        tracing::info!(
            elapsed_ms = sqlite_started.elapsed().as_millis(),
            "SQLite initialized"
        );
        let resource_store = Arc::new(SqliteResourceStore::new(database.pool().clone()));
        let directory_store = Arc::new(SqliteDirectoryStore::new(database.pool().clone()));
        let directory_index = Arc::new(InMemoryDirectoryIndex::from_directories(
            directory_store.load_all().await?,
        )?);
        let identity_repository = Arc::new(SqliteIdentityRepository::new(database.pool().clone()));
        let upload_session_repository =
            Arc::new(SqliteUploadSessionRepository::new(database.pool().clone()));
        let content_replacement_repository = Arc::new(
            SqliteResourceContentReplacementRepository::new(database.pool().clone()),
        );
        let idempotency_repository =
            Arc::new(SqliteIdempotencyRepository::new(database.pool().clone()));
        Ok(Self {
            config,
            resource_store,
            directory_store,
            directory_index,
            identity_repository,
            upload_session_repository,
            content_replacement_repository,
            idempotency_repository,
            blob_storage,
            storage_scanner,
        })
    }

    /// 返回实际生效的基础设施配置。
    pub fn config(&self) -> &AssetInfraConfig {
        &self.config
    }

    /// 返回资源仓储端口对象。
    pub fn resource_store(&self) -> Arc<dyn ResourceStore> {
        self.resource_store.clone()
    }

    pub fn resource_read_model(&self) -> Arc<dyn ResourceReadModel> {
        self.resource_store.clone()
    }

    pub fn resource_relocation_store(&self) -> Arc<dyn ResourceRelocationStore> {
        self.resource_store.clone()
    }

    pub fn resource_maintenance_read_model(&self) -> Arc<dyn ResourceMaintenanceReadModel> {
        self.resource_store.clone()
    }

    pub fn directory_store(&self) -> Arc<dyn DirectoryStore> {
        self.directory_store.clone()
    }

    pub fn directory_relocation_store(&self) -> Arc<dyn DirectoryRelocationStore> {
        self.directory_store.clone()
    }

    pub fn directory_index(&self) -> Arc<dyn DirectoryProjection> {
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

    /// 返回对象内容只读端口。
    pub fn content_reader(&self) -> Arc<dyn ContentReader> {
        self.blob_storage.clone()
    }

    /// 返回内部暂存对象端口。
    pub fn content_staging_store(&self) -> Arc<dyn ContentStagingStore> {
        self.blob_storage.clone()
    }

    /// 返回对象搬迁与删除端口。
    pub fn content_object_store(&self) -> Arc<dyn ContentObjectStore> {
        self.blob_storage.clone()
    }

    /// 返回对象存储就绪检查端口。
    pub fn blob_health(&self) -> Arc<dyn BlobHealth> {
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

    pub fn idempotency_repository(&self) -> Arc<dyn IdempotencyRepository> {
        self.idempotency_repository.clone()
    }
}
