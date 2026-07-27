//! Core 依赖的 Host Port。
//!
//! - 资源持久化：`ResourceRepository`、`ResourceQuery`
//! - 目录持久化：`DirectoryRepository`
//! - 内容存储：`BlobStorage`、`DirectoryStorage`、`StorageScanner`
//! - 运行时注册与执行：kind/action registry、action executor
//! - 身份与审计：用户仓储、密码哈希、安全审计仓储
//!
//! Port 只描述 Core 所需语义；OpenDAL、sqlx、Wasm runtime 等具体类型只能出现在
//! infrastructure adapter 中。所有公开端口统一从本模块 re-export。

mod audit;
mod directory;
mod directory_kind;
mod identity;
mod resource;
mod storage;

pub use audit::SecurityAuditRepository;
pub use directory::{DirectoryLocation, DirectoryRepository};
pub use directory_kind::{DirectoryKindDefinition, DirectoryKindRegistry};
pub use identity::{LocatedUser, PasswordHasher, UserQuery, UserRepository};
pub use resource::{
    ListResources, LocatedResource, ResourceActionExecutor, ResourceActionOutput,
    ResourceActionRegistry, ResourceActionRequest, ResourceKindDefinition, ResourceKindRegistry,
    ResourcePage, ResourceQuery, ResourceRepository,
};
pub use storage::{
    BlobByteStream, BlobStorage, BlobWriteResult, DirectoryStorage, RESERVED_BLOB_STORAGE_PREFIX,
    ScannedBlob, ScannedStorageEntry, StoragePrefix, StorageScanStream, StorageScanner,
};
