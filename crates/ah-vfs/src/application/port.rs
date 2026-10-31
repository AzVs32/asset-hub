//! Interfaces implemented by infrastructure adapters.

mod driver;
mod mount_repository;
mod storage;

pub use driver::{Driver, DriverFuture};
pub use mount_repository::{MountRepository, MountRepositoryFuture};
pub use storage::validate_storage_capabilities;
pub use storage::{Storage, StorageFileReader, StorageFileWriter, StorageFuture};

mod storage_attributes;
pub use storage_attributes::{StoragePermissions, StorageXattrs};

mod instance_registry;
mod open_context;
pub use instance_registry::{InstanceManagement, InstanceRegistry};
pub use open_context::OpenContext;

mod instance_provisioner;
mod instance_record_repository;
mod secret_cipher;
pub use super::host_instance_registry::record::{InstanceRecord, InstanceRecordPage};
pub use instance_provisioner::{InstanceProvisioner, PreparedConfig};
pub use instance_record_repository::InstanceRecordRepository;
pub use secret_cipher::{SecretCipher, SecretContext};
