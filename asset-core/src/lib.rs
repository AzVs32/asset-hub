//! Asset Hub 的内部领域内核。
//!
//! 该 crate 只供当前 workspace 的应用与基础设施适配器使用，不是插件 SDK。
//! 插件开发者应只依赖 `asset-plugin-api`。

pub mod domain;
mod error;
pub mod port;
pub mod service;

mod utils;

pub use error::{CoreError, DirectoryError, ResourceError, UserError};
