pub mod config;
mod directory_index;
pub mod migration;
pub mod sqlite;
pub mod storage;

use asset_core::{
    CoreError,
    directory::port::{
        DirectoryProjection, DirectoryReadModel, DirectoryRelocationStore, DirectoryStore,
    },
    idempotency::port::IdempotencyRepository,
    resource::port::{
        ResourceContentReplacementStore, ResourceDeletionStore, ResourceMaintenanceReadModel,
        ResourceReadModel, ResourceRelocationStore, ResourceStore, UploadSessionStore,
    },
    storage::port::{
        BlobHealth, ContentObjectStore, ContentReader, ContentStagingStore, DirectoryStorage,
        StorageScanner,
    },
};
use config::{BlobBackend, BlobConfig, DatabaseBackend, DatabaseConfig};
use directory_index::InMemoryDirectoryIndex;
use sqlite::{
    SqliteDatabase, SqliteDirectoryStore, SqliteIdempotencyRepository,
    SqliteResourceContentReplacementStore, SqliteResourceStore, SqliteUploadSessionStore,
};
use std::sync::Arc;
use std::time::Instant;
use storage::{FileSystemScanner, OpenDalBlobStorage};

/// 根据配置的后端选型初始化具体基础设施适配器。
///
/// 当前支持 SQLite 数据库和本地 Blob 存储；Core service 由 `asset-runtime` 装配。
pub struct AssetInfrastructure {
    resource_store: Arc<SqliteResourceStore>,
    directory_store: Arc<SqliteDirectoryStore>,
    directory_index: Arc<InMemoryDirectoryIndex>,
    upload_session_store: Arc<SqliteUploadSessionStore>,
    content_replacement_store: Arc<SqliteResourceContentReplacementStore>,
    idempotency_repository: Arc<SqliteIdempotencyRepository>,
    /// 对象存储适配器。
    blob_storage: Arc<OpenDalBlobStorage>,
    storage_scanner: Arc<FileSystemScanner>,
}

impl AssetInfrastructure {
    /// 使用给定的数据库和 Blob 配置创建基础设施组合。
    pub async fn new(
        database_config: DatabaseConfig,
        blob_config: BlobConfig,
    ) -> Result<Self, CoreError> {
        database_config
            .validate()
            .map_err(CoreError::configuration)?;
        let blob_config = blob_config.normalize().map_err(CoreError::configuration)?;
        let (blob_storage, storage_scanner) = match blob_config.backend {
            BlobBackend::Local => (
                Arc::new(OpenDalBlobStorage::from_local_root(
                    &blob_config.local.root,
                )?),
                Arc::new(FileSystemScanner::new(blob_config.local.root.clone())),
            ),
        };
        let sqlite_started = Instant::now();
        let database = match database_config.backend {
            DatabaseBackend::Sqlite => {
                let sqlite_path = database_config.sqlite_path_in(blob_config.local_root());
                SqliteDatabase::connect(&sqlite_path, database_config.sqlite.max_connections)
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
        let upload_session_store = Arc::new(SqliteUploadSessionStore::new(database.pool().clone()));
        let content_replacement_store = Arc::new(SqliteResourceContentReplacementStore::new(
            database.pool().clone(),
        ));
        let idempotency_repository =
            Arc::new(SqliteIdempotencyRepository::new(database.pool().clone()));
        Ok(Self {
            resource_store,
            directory_store,
            directory_index,
            upload_session_store,
            content_replacement_store,
            idempotency_repository,
            blob_storage,
            storage_scanner,
        })
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

    pub fn resource_deletion_store(&self) -> Arc<dyn ResourceDeletionStore> {
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

    pub fn directory_read_model(&self) -> Arc<dyn DirectoryReadModel> {
        self.directory_index.clone()
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

    pub fn upload_session_store(&self) -> Arc<dyn UploadSessionStore> {
        self.upload_session_store.clone()
    }

    pub fn content_replacement_store(&self) -> Arc<dyn ResourceContentReplacementStore> {
        self.content_replacement_store.clone()
    }

    pub fn idempotency_repository(&self) -> Arc<dyn IdempotencyRepository> {
        self.idempotency_repository.clone()
    }
}
