//! Core 依赖的 Host Port。
//!
//! - 资源持久化：`ResourceRepository`、`ResourceQuery`
//! - 内容存储：`BlobStorage`、`DirectoryStorage`、`StorageScanner`
//! - 运行时注册与执行：kind/action registry、action executor
//! - 身份与审计：用户仓储、密码哈希、安全审计仓储
//!
//! Port 只描述 Core 所需语义；OpenDAL、sqlx、Wasm runtime 等具体类型只能出现在
//! infrastructure adapter 中。所有公开端口统一从本模块 re-export。

mod blob_storage;
mod directory_storage;
mod password_hasher;
mod resource_action_executor;
mod resource_action_registry;
mod resource_kind_registry;
mod resource_query;
mod resource_repository;
mod security_audit_repository;
mod storage_scanner;
mod user_repository;

// 内容存储与扫描。
pub use blob_storage::{
    BlobByteStream, BlobStorage, BlobWriteResult, RESERVED_BLOB_STORAGE_PREFIX,
};
pub use directory_storage::DirectoryStorage;
pub use storage_scanner::{
    ScannedBlob, ScannedStorageEntry, StoragePrefix, StorageScanStream, StorageScanner,
};

// 资源写模型与读模型。
pub use resource_query::{ListResources, ResourcePage, ResourceQuery};
pub use resource_repository::ResourceRepository;

// kind/action 运行时。
pub use resource_action_executor::{
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest,
};
pub use resource_action_registry::ResourceActionRegistry;
pub use resource_kind_registry::{ResourceKindDefinition, ResourceKindRegistry};

// 身份与审计。
pub use password_hasher::PasswordHasher;
pub use security_audit_repository::SecurityAuditRepository;
pub use user_repository::UserRepository;
