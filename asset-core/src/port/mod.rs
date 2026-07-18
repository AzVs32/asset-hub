//! 基础设施适配器需要实现的端口。
//!
//! 端口只通过本模块的精选 re-export 暴露，具体实现文件不是独立 API 路径。

mod access_policy_repository;
mod blob_storage;
mod directory_storage;
mod password_hasher;
mod resource_action_executor;
mod resource_action_registry;
mod resource_kind_registry;
mod resource_query;
mod resource_repository;
mod storage_scanner;
mod user_repository;

pub use access_policy_repository::AccessPolicyRepository;
pub use blob_storage::{
    BlobByteStream, BlobStorage, BlobWriteResult, RESERVED_BLOB_STORAGE_PREFIX,
};
pub use directory_storage::DirectoryStorage;
pub use password_hasher::PasswordHasher;
pub use resource_action_executor::{
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRequest,
};
pub use resource_action_registry::ResourceActionRegistry;
pub use resource_kind_registry::{ResourceKindDefinition, ResourceKindRegistry};
pub use resource_query::{ListResources, ResourcePage, ResourceQuery};
pub use resource_repository::ResourceRepository;
pub use storage_scanner::{ScannedBlob, StoragePrefix, StorageScanner};
pub use user_repository::UserRepository;
