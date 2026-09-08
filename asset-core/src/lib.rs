//! Asset Hub 的内部领域内核。
//!
//! 该 crate 只供当前 workspace 的应用与基础设施适配器使用。
//! Core 仅依赖领域模型和端口定义。

pub mod directory;
pub mod error;
pub mod idempotency;
pub mod resource;
pub mod storage;
pub mod workflow;

mod utils;

pub use error::{CoreError, DirectoryError, IdempotencyError, ResourceError, StorageError};
