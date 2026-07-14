use serde::{Deserialize, Serialize};

/// Plugin permission declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPermissions {
    pub resource: ReadWritePermission,
    pub content: ReadWritePermission,
    #[serde(default)]
    pub network: NetworkPermission,
    #[serde(default)]
    pub filesystem: FilesystemPermission,
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
    D: serde::Deserializer<'de>,
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
    D: serde::Deserializer<'de>,
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
