//! Manifest 中的浏览器资产声明。
//!
//! 这里只记录插件包内 Web 根目录；Host 负责安全解析路径、校验锁文件并发布资产。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Browser assets loaded from verified plugin packages, grouped by plugin id
/// and package-relative path.
pub type PluginWebAssets = HashMap<String, HashMap<PathBuf, Arc<[u8]>>>;

/// Browser-facing assets contributed by a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginWeb {
    pub root: PathBuf,
}
