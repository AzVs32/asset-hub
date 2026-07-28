//! 插件包生成的完整性锁定文档。
//!
//! 锁文件记录 Wasm 与 Web 资产摘要，绑定到指定 Manifest 版本和插件 ID；摘要计算与
//! 文件加载属于 Host 基础设施职责，不在本模块中实现。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Generated integrity data for a plugin package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifestLock {
    pub manifest_version: u32,
    pub plugin_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<PluginRuntimeLock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<PluginWebLock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRuntimeLock {
    pub wasm_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginWebLock {
    pub integrity: BTreeMap<PathBuf, String>,
}
