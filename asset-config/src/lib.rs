//! Asset Hub 可扩展的强类型配置装配模块。
//!
//! 本模块只负责配置来源、配置分区注册和强类型加载。各业务、基础设施与终端模块通过实现并
//! 注册自己的 [`ConfigSection`] 增加配置，本模块不拥有任何具体应用配置。

mod error;
// 对外：读取加载结果、重新输出 TOML
mod loaded;
// 对外：程序如何注册、加载配置
mod registry;
// 对外：模块如何声明配置
mod section;

pub use error::ConfigError;
pub use loaded::LoadedConfig;
pub use registry::ConfigRegistry;
pub use section::ConfigSection;

#[cfg(test)]
mod tests;
