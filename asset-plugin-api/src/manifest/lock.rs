//! 插件包生成的完整性锁定文档。
//!
//! 锁文件以包内相对路径为键记录所有插件产物摘要，并绑定到指定 Manifest 版本和插件
//! ID；摘要计算与文件加载属于 Host 基础设施职责，不在本模块中实现。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Generated integrity data for a plugin package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifestLock {
    pub manifest_version: u32,
    pub plugin_id: String,
    pub integrity: BTreeMap<PathBuf, String>,
}
