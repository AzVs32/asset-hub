use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 资源类型注册表配置。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KindRegistryConfig {
    /// 插件 manifest 文件。每个文件会在启动时加载。
    pub plugin_manifests: Vec<PathBuf>,
}
