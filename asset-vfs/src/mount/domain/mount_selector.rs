use crate::error::VfsError;
use crate::mount::{Mount, MountError, MountId};
use crate::namespace::VirtualPath;

/// An exact mount point or a specific persisted definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountSelector {
    Path(VirtualPath),
    Id(MountId),
}

impl MountSelector {
    pub(crate) fn select<'a>(&self, mounts: &'a [Mount]) -> Result<Option<&'a Mount>, VfsError> {
        match self {
            Self::Id(id) => Ok(mounts.iter().find(|mount| mount.id() == *id)),
            Self::Path(path) => {
                let matches: Vec<_> = mounts
                    .iter()
                    .filter(|mount| mount.virtual_path() == path)
                    .collect();
                let active: Vec<_> = matches
                    .iter()
                    .copied()
                    .filter(|mount| mount.enabled())
                    .collect();
                match active.as_slice() {
                    [mount] => Ok(Some(*mount)),
                    [] => match matches.as_slice() {
                        [] => Ok(None),
                        [mount] => Ok(Some(*mount)),
                        _ => Err(MountError::AmbiguousPath(path.clone()).into()),
                    },
                    _ => Err(MountError::DuplicatePath(path.clone()).into()),
                }
            }
        }
    }
}

impl From<MountId> for MountSelector {
    fn from(id: MountId) -> Self {
        Self::Id(id)
    }
}

impl From<VirtualPath> for MountSelector {
    fn from(path: VirtualPath) -> Self {
        Self::Path(path)
    }
}

impl From<&VirtualPath> for MountSelector {
    fn from(path: &VirtualPath) -> Self {
        Self::Path(path.clone())
    }
}

impl std::str::FromStr for MountSelector {
    type Err = VfsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.starts_with('/') {
            Ok(Self::Path(VirtualPath::try_from(value)?))
        } else {
            Ok(Self::Id(value.parse()?))
        }
    }
}

impl std::fmt::Display for MountSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(path) => path.fmt(formatter),
            Self::Id(id) => id.fmt(formatter),
        }
    }
}
