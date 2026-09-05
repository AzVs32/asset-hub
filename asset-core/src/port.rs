//! Core 依赖的 Host Port。
//!
//! - 资源持久化：`ResourceStore`、`ResourceReadModel`、`ResourceRelocationStore`
//! - 目录持久化与查询：`DirectoryStore`、`DirectoryQuery`、`DirectoryIndex`
//! - 内容存储：`ContentReader`、`ContentStagingStore`、`ContentObjectStore`、`BlobHealth`、
//!   `DirectoryStorage`、`StorageScanner`
//! - 运行时注册：kind registry
//! - 身份：用户仓储、密码哈希
//! - 幂等：`IdempotencyRepository`
//!
//! Port 只描述 Core 所需语义；OpenDAL、sqlx、Wasm runtime 等具体类型只能出现在
//! infrastructure adapter 中。所有公开端口统一从本模块 re-export。

mod directory;
mod idempotency;
mod identity;
mod resource;
mod storage;
mod upload;

pub use directory::{
    DirectoryIndex, DirectoryKindRegistry, DirectoryLocation, DirectoryProjection, DirectoryQuery,
    DirectoryRelocation, DirectoryRelocationStore, DirectoryRevisionUpdate, DirectoryStore,
    LocatedDirectory,
};
pub use idempotency::{IdempotencyAcquire, IdempotencyRepository};
pub use identity::{LocatedUser, PasswordHasher, UserQuery, UserRepository};
pub use resource::{
    ListResources, LocatedResource, ResourceContentReplacementRepository, ResourceKindRegistry,
    ResourceMaintenanceReadModel, ResourcePage, ResourceReadModel, ResourceRelocation,
    ResourceRelocationStore, ResourceStore,
};
pub use storage::{
    BlobByteStream, BlobHealth, ContentObjectStore, ContentReader, ContentStagingStore,
    DirectoryStorage, RESERVED_BLOB_STORAGE_PREFIX, ScannedBlob, ScannedStorageEntry, StagedBlob,
    StoragePrefix, StorageScanStream, StorageScanner,
};
pub use upload::UploadSessionRepository;
