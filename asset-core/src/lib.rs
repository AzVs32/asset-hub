//! Asset Hub 的内部领域内核。
//!
//! 该 crate 只供当前 workspace 的应用与基础设施适配器使用。
//! Core 仅依赖领域模型和 Host Port，不依赖插件协议或运行时。

pub mod domain;
mod error;
pub mod port;
pub mod service;

mod utils;

pub use error::{CoreError, DirectoryError, ResourceError, UserError};
