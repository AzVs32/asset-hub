//! Asset Hub 的语言无关插件契约的 Rust 映射。

/// Cargo package version. This is independent from Manifest and Plugin API versions.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod abi;
pub mod manifest;
pub mod protocol;
