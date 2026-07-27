use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;

/// Fine-grained host capabilities requested by a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PluginPermission {
    #[serde(rename = "resource.read")]
    ResourceRead,
    #[serde(rename = "resource.write")]
    ResourceWrite,
    #[serde(rename = "content.read")]
    ContentRead,
    #[serde(rename = "content.replace")]
    ContentReplace,
    #[serde(rename = "derived_asset.write")]
    DerivedAssetWrite,
    #[serde(rename = "directory.read")]
    DirectoryRead,
    #[serde(rename = "directory.children.list")]
    DirectoryChildrenList,
    #[serde(rename = "directory.resources.list")]
    DirectoryResourcesList,
    #[serde(rename = "directory.write")]
    DirectoryWrite,
    #[serde(rename = "directory.create_child")]
    DirectoryCreateChild,
}

/// Fine-grained permissions requested by a plugin manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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

    pub fn resource_write(&self) -> bool {
        self.allows(PluginPermission::ResourceWrite)
    }

    pub fn content_read(&self) -> bool {
        self.allows(PluginPermission::ContentRead)
    }

    pub fn content_replace(&self) -> bool {
        self.allows(PluginPermission::ContentReplace)
    }

    pub fn derived_asset_write(&self) -> bool {
        self.allows(PluginPermission::DerivedAssetWrite)
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
    pub fn directory_create_child(&self) -> bool {
        self.allows(PluginPermission::DirectoryCreateChild)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NetworkPermission {
    Flag(bool),
    Scoped(NetworkScope),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilesystemPermission {
    Flag(bool),
    Scoped(FilesystemScope),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests;
