use std::collections::HashSet;

use crate::mount::domain::Mount;
use crate::mount::error::ServiceError;
use crate::path::domain::VPath;

/// The result of resolving a path in the virtual mount namespace.
#[derive(Debug, PartialEq, Eq)]
pub enum MountResolution<'a> {
    /// The deepest enabled mount that covers the path.
    Mounted(&'a Mount),
    /// A directory synthesized from the ancestors of enabled mount points.
    VirtualDirectory {
        /// The covering mount whose entries may be merged into this directory.
        underlying_mount: Option<&'a Mount>,
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
            if !paths.insert(mount.v_path().as_str()) {
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
            return MountResolution::Mounted(mount);
        }

        if path.is_root() || has_nested_mount {
            MountResolution::VirtualDirectory {
                underlying_mount: covering_mount,
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

            if !children.contains(&child) {
                children.push(child);
            }
        }

        children.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        children
    }
}

#[cfg(test)]
mod tests;
