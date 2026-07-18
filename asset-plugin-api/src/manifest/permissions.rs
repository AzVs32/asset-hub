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
}

/// Plugin permission declaration. V3 uses `allow`; the V2 resource/content shape is accepted on
/// input and normalized to the fine-grained set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
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
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PermissionsDocument {
    V3(V3Permissions),
    V2(V2Permissions),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V3Permissions {
    #[serde(default)]
    allow: BTreeSet<PluginPermission>,
    #[serde(default)]
    network: NetworkPermission,
    #[serde(default)]
    filesystem: FilesystemPermission,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V2Permissions {
    resource: ReadWritePermission,
    content: ReadWritePermission,
    #[serde(default)]
    network: NetworkPermission,
    #[serde(default)]
    filesystem: FilesystemPermission,
}

impl<'de> Deserialize<'de> for PluginPermissions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = PermissionsDocument::deserialize(deserializer)?;
        Ok(match document {
            PermissionsDocument::V3(value) => Self {
                allow: value.allow,
                network: value.network,
                filesystem: value.filesystem,
            },
            PermissionsDocument::V2(value) => {
                let mut allow = BTreeSet::new();
                if value.resource.read {
                    allow.insert(PluginPermission::ResourceRead);
                }
                if value.resource.write {
                    allow.insert(PluginPermission::ResourceWrite);
                }
                if value.content.read {
                    allow.insert(PluginPermission::ContentRead);
                }
                if value.content.write {
                    allow.insert(PluginPermission::ContentReplace);
                }
                Self {
                    allow,
                    network: value.network,
                    filesystem: value.filesystem,
                }
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadWritePermission {
    pub read: bool,
    pub write: bool,
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
