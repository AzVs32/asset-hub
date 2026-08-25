//! Manifest 权限声明及作用域值对象。
//!
//! 权限只表达插件申请的能力边界；Host 仍需结合 Action access、当前用户授权和执行策略
//! 做最终判定。

use super::{ManifestValidationCode, ManifestValidationError};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;

/// Fine-grained host capabilities requested by a plugin.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum PluginPermission {
    #[serde(rename = "resource.read")]
    ResourceRead,
    #[serde(rename = "resource.create")]
    ResourceCreate,
    #[serde(rename = "resource.delete")]
    ResourceDelete,
    #[serde(rename = "resource.content.read")]
    ResourceContentRead,
    #[serde(rename = "resource.content.replace")]
    ResourceContentReplace,
    #[serde(rename = "directory.read")]
    DirectoryRead,
    #[serde(rename = "directory.children.list")]
    DirectoryChildrenList,
    #[serde(rename = "directory.resources.list")]
    DirectoryResourcesList,
    #[serde(rename = "directory.write")]
    DirectoryWrite,
    #[serde(rename = "directory.delete")]
    DirectoryDelete,
    #[serde(rename = "directory.create_child")]
    DirectoryCreateChild,
}

/// Fine-grained permissions requested by a plugin manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginPermissions {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub allow: BTreeSet<PluginPermission>,
    #[serde(default)]
    pub network: NetworkPermission,
    #[serde(default)]
    pub filesystem: FilesystemPermission,
}

impl PluginPermissions {
    pub fn allows(&self, permission: PluginPermission) -> bool {
        self.allow.contains(&permission)
    }

    pub fn resource_read(&self) -> bool {
        self.allows(PluginPermission::ResourceRead)
    }

    pub fn resource_create(&self) -> bool {
        self.allows(PluginPermission::ResourceCreate)
    }

    pub fn resource_delete(&self) -> bool {
        self.allows(PluginPermission::ResourceDelete)
    }

    pub fn resource_content_read(&self) -> bool {
        self.allows(PluginPermission::ResourceContentRead)
    }

    pub fn resource_content_replace(&self) -> bool {
        self.allows(PluginPermission::ResourceContentReplace)
    }

    pub fn directory_read(&self) -> bool {
        self.allows(PluginPermission::DirectoryRead)
    }
    pub fn directory_children_list(&self) -> bool {
        self.allows(PluginPermission::DirectoryChildrenList)
    }
    pub fn directory_resources_list(&self) -> bool {
        self.allows(PluginPermission::DirectoryResourcesList)
    }
    pub fn directory_write(&self) -> bool {
        self.allows(PluginPermission::DirectoryWrite)
    }
    pub fn directory_delete(&self) -> bool {
        self.allows(PluginPermission::DirectoryDelete)
    }
    pub fn directory_create_child(&self) -> bool {
        self.allows(PluginPermission::DirectoryCreateChild)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum NetworkPermission {
    Flag(bool),
    Scoped(NetworkScope),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NetworkScope {
    #[serde(deserialize_with = "deserialize_nonempty_strings")]
    hosts: Vec<String>,
}

impl Default for NetworkPermission {
    fn default() -> Self {
        Self::Flag(false)
    }
}

impl NetworkPermission {
    pub fn scoped(
        hosts: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ManifestValidationError> {
        let hosts = canonical_scope_values(hosts, true)?;
        Ok(Self::Scoped(NetworkScope { hosts }))
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Flag(value) => *value,
            Self::Scoped(scope) => !scope.hosts.is_empty(),
        }
    }
    pub fn has_scope(&self) -> bool {
        matches!(self, Self::Scoped(_))
    }
    pub fn hosts(&self) -> &[String] {
        match self {
            Self::Flag(_) => &[],
            Self::Scoped(scope) => &scope.hosts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FilesystemPermission {
    Flag(bool),
    Scoped(FilesystemScope),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesystemScope {
    #[serde(deserialize_with = "deserialize_strings")]
    read: Vec<String>,
    #[serde(deserialize_with = "deserialize_strings")]
    write: Vec<String>,
}

fn deserialize_strings<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<String>::deserialize(deserializer)?;
    if values
        .iter()
        .any(|value| value.is_empty() || value.trim() != value)
    {
        return Err(serde::de::Error::custom(
            "permission scopes must be non-empty canonical strings",
        ));
    }
    Ok(values)
}

fn deserialize_nonempty_strings<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = deserialize_strings(deserializer)?;
    if values.is_empty() {
        return Err(serde::de::Error::custom(
            "permission scope must contain at least one value",
        ));
    }
    Ok(values)
}

impl Default for FilesystemPermission {
    fn default() -> Self {
        Self::Flag(false)
    }
}

impl FilesystemPermission {
    pub fn scoped(
        read_paths: impl IntoIterator<Item = impl Into<String>>,
        write_paths: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ManifestValidationError> {
        let read = canonical_scope_values(read_paths, false)?;
        let write = canonical_scope_values(write_paths, false)?;
        if read.is_empty() && write.is_empty() {
            return Err(ManifestValidationError::new(
                ManifestValidationCode::InvalidValue,
                "$.permissions.filesystem",
                "filesystem scope must contain at least one read or write path",
            ));
        }
        Ok(Self::Scoped(FilesystemScope { read, write }))
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Flag(value) => *value,
            Self::Scoped(scope) => !scope.read.is_empty() || !scope.write.is_empty(),
        }
    }
    pub fn has_scope(&self) -> bool {
        matches!(self, Self::Scoped(_))
    }
    pub fn read_paths(&self) -> &[String] {
        match self {
            Self::Flag(_) => &[],
            Self::Scoped(scope) => &scope.read,
        }
    }
    pub fn write_paths(&self) -> &[String] {
        match self {
            Self::Flag(_) => &[],
            Self::Scoped(scope) => &scope.write,
        }
    }
}

fn canonical_scope_values(
    values: impl IntoIterator<Item = impl Into<String>>,
    require_nonempty: bool,
) -> Result<Vec<String>, ManifestValidationError> {
    let values = values.into_iter().map(Into::into).collect::<Vec<_>>();
    if values
        .iter()
        .any(|value| value.is_empty() || value.trim() != value)
        || require_nonempty && values.is_empty()
    {
        return Err(ManifestValidationError::new(
            ManifestValidationCode::InvalidValue,
            "$.permissions",
            "permission scopes must contain canonical non-empty strings",
        ));
    }
    Ok(values)
}

#[cfg(test)]
mod tests;
