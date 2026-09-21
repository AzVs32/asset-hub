use std::collections::HashSet;

use crate::mount::domain::Mount;
use crate::mount::error::ServiceError;
use crate::path::domain::{DPath, VPath};

/// A mount together with the path it should handle.
#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedMount<'a> {
    mount: &'a Mount,
    relative_path: DPath,
}

impl<'a> ResolvedMount<'a> {
    fn new(mount: &'a Mount, path: &VPath) -> Self {
        let relative_path = if mount.v_path() == path {
            ""
        } else if mount.v_path().is_root() {
            path.as_str()
                .strip_prefix('/')
                .expect("a virtual path is always absolute")
        } else {
            path.as_str()
                .strip_prefix(mount.v_path().as_str())
                .and_then(|path| path.strip_prefix('/'))
                .expect("a resolved mount always covers the path")
        };

        Self {
            mount,
            relative_path: DPath::new(relative_path),
        }
    }

    /// Returns the resolved mount.
    pub fn mount(&self) -> &'a Mount {
        self.mount
    }

    /// Returns the normalized path relative to the mount point.
    pub fn relative_path(&self) -> &DPath {
        &self.relative_path
    }
}

/// The result of resolving a path in the virtual mount namespace.
#[derive(Debug, PartialEq, Eq)]
pub enum MountResolution<'a> {
    /// The deepest enabled mount that covers the path and its relative path.
    Mounted(ResolvedMount<'a>),
    /// A directory synthesized from the ancestors of enabled mount points.
    VirtualDirectory {
        /// The covering mount whose entries may be merged into this directory.
        underlying_mount: Option<ResolvedMount<'a>>,
    },
    /// A path with no enabled mount or synthesized directory.
    NotFound,
}

/// Resolves paths against an immutable snapshot of mount configurations.
#[derive(Debug)]
pub struct MountService {
    mounts: Vec<Mount>,
}

impl MountService {
    /// Creates a mount service and rejects duplicate IDs or mount points.
    pub fn new(mounts: Vec<Mount>) -> Result<Self, ServiceError> {
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();

        for mount in &mounts {
            if !ids.insert(mount.id()) {
                return Err(ServiceError::DuplicateMountId(mount.id()));
            }
            if !paths.insert(mount.v_path()) {
                return Err(ServiceError::DuplicateMountPath(mount.v_path().clone()));
            }
        }

        Ok(Self { mounts })
    }

    /// Finds the deepest enabled mount or a synthesized virtual directory.
    pub fn resolve(&self, path: &VPath) -> MountResolution<'_> {
        let covering_mount = self
            .mounts
            .iter()
            .filter(|mount| mount.enabled() && mount.covers(path))
            .max_by_key(|mount| mount.v_path().depth());
        let has_nested_mount = self
            .mounts
            .iter()
            .any(|mount| mount.enabled() && path.is_ancestor_of(mount.v_path()));

        if let Some(mount) = covering_mount
            && (mount.v_path() == path || !has_nested_mount)
        {
            return MountResolution::Mounted(ResolvedMount::new(mount, path));
        }

        if path.is_root() || has_nested_mount {
            MountResolution::VirtualDirectory {
                underlying_mount: covering_mount.map(|mount| ResolvedMount::new(mount, path)),
            }
        } else {
            MountResolution::NotFound
        }
    }

    /// Lists the direct virtual children needed to expose enabled mount points.
    pub fn virtual_children(&self, parent: &VPath) -> Vec<VPath> {
        let mut children = Vec::new();

        for mount in self
            .mounts
            .iter()
            .filter(|mount| mount.enabled() && parent.is_ancestor_of(mount.v_path()))
        {
            let mut child = mount.v_path().clone();
            while child.parent().as_ref() != Some(parent) {
                child = child.parent().expect("an ancestor has a direct child");
            }

            children.push(child);
        }

        children.sort();
        children.dedup();
        children
    }
}

#[cfg(test)]
mod tests;
