use serde::{Deserialize, Serialize};

/// Plugin permission declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissions {
    pub resource: ReadWritePermission,
    pub content: ReadWritePermission,
    #[serde(default)]
    pub network: NetworkPermission,
    #[serde(default)]
    pub filesystem: FilesystemPermission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadWritePermission {
    pub read: bool,
    pub write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NetworkPermission {
    Flag(bool),
    Scoped { hosts: Vec<String> },
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
            Self::Scoped { hosts } => !hosts.is_empty(),
        }
    }

    pub fn has_scope(&self) -> bool {
        matches!(self, Self::Scoped { .. })
    }

    pub fn hosts(&self) -> &[String] {
        match self {
            Self::Flag(_) => &[],
            Self::Scoped { hosts } => hosts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilesystemPermission {
    Flag(bool),
    Scoped {
        read: Vec<String>,
        write: Vec<String>,
    },
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
            Self::Scoped { read, write } => !read.is_empty() || !write.is_empty(),
        }
    }

    pub fn has_scope(&self) -> bool {
        matches!(self, Self::Scoped { .. })
    }

    pub fn read_paths(&self) -> &[String] {
        match self {
            Self::Flag(_) => &[],
            Self::Scoped { read, .. } => read,
        }
    }

    pub fn write_paths(&self) -> &[String] {
        match self {
            Self::Flag(_) => &[],
            Self::Scoped { write, .. } => write,
        }
    }
}
