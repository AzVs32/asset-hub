use asset_plugin_api::{ResourceActionDefinition, ResourceContentMatcher};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 资源类型注册表配置。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KindRegistryConfig {
    /// 由系统配置声明的资源类型。
    pub definitions: Vec<ResourceKindConfig>,
    /// 插件 manifest 文件。每个文件会在启动时加载。
    pub plugin_manifests: Vec<PathBuf>,
}

/// 配置文件中的资源类型定义。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceKindConfig {
    /// 资源类型值，例如 `core:image`。
    pub kind: String,
    /// 可选父类型；父类型必须由内置、配置或插件定义。
    pub parent: Option<String>,
    /// 展示名称；为空时使用 `kind`。
    pub label: Option<String>,
    /// 是否支持对象内容。
    pub supports_content: bool,
    /// 文件自动识别规则。上传时前端可用这些规则自动选择 kind。
    pub detect: ResourceContentMatcher,
    /// 以该 kind 为作用域声明的动作；启动时会统一注册到 `ResourceActionRegistry`。
    pub actions: Vec<ResourceActionDefinition>,
}

impl Default for ResourceKindConfig {
    fn default() -> Self {
        Self {
            kind: String::new(),
            parent: None,
            label: None,
            supports_content: true,
            detect: ResourceContentMatcher::default(),
            actions: Vec::new(),
        }
    }
}
