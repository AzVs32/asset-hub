use super::{ChildMountPolicy, Mount, MountId, VirtualPath};
use crate::domain::DriverKey;
use ah_vfs_storage::{PathError, StoragePath};
use thiserror::Error;

/// A virtual namespace with one required root mount and unique mount IDs and paths.
///
/// Driver policies are supplied by the application when inserting mounts.
/// Disabled mounts retain their namespace boundaries and still constrain nesting.
#[derive(Debug)]
pub struct MountTable {
    root: Mount,
    mounts: Vec<Mount>,
}

#[derive(Debug, Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum MountTableError {
    #[error("the initial mount must be at the virtual root")]
    RootRequired,
    #[error("the root mount cannot be removed")]
    RootCannotBeRemoved,
    #[error("mount '{0}' is already registered")]
    DuplicateId(MountId),
    #[error("virtual path '{0}' already has a mount")]
    PathOccupied(VirtualPath),
    #[error("mount '{0}' is not registered")]
    NotFound(MountId),
    #[error("child mount policy is unavailable for driver '{0}'")]
    PolicyUnavailable(DriverKey),
    #[error("driver '{driver}' forbids child mounts under '{path}'")]
    ChildMountsForbidden {
        driver: DriverKey,
        path: VirtualPath,
    },
    #[error("mount '{0}' is disabled")]
    Disabled(MountId),
}

/// The selected mount and the requested path relative to its mount point.
#[derive(Debug)]
pub struct MountResolution<'mount, 'path> {
    mount: &'mount Mount,
    relative_path: &'path str,
}

impl<'mount, 'path> MountResolution<'mount, 'path> {
    pub fn mount(&self) -> &'mount Mount {
        self.mount
    }

    /// Borrow the suffix without a leading slash; the mount point itself yields `""`.
    pub fn relative_path(&self) -> &'path str {
        self.relative_path
    }

    /// Validate the routed suffix as an instance-local storage address.
    ///
    /// Namespace prefixes may contain characters such as `:` that storage addresses
    /// reject. Only the suffix is checked: mounting at `/cloud:archive` is valid,
    /// while a suffix such as `file:stream` returns [`PathError::InvalidComponent`].
    /// The mount point itself maps to [`StoragePath::root`]. No backend is opened.
    pub fn storage_path(&self) -> Result<StoragePath, PathError> {
        StoragePath::parse(self.relative_path)
    }
}

impl MountTable {
    /// Create a namespace whose initial mount is at `/`.
    pub fn new(root: Mount) -> Result<Self, MountTableError> {
        if !root.virtual_path().is_root() {
            return Err(MountTableError::RootRequired);
        }
        Ok(Self {
            root,
            mounts: Vec::new(),
        })
    }

    /// Restore unordered persisted definitions using the same checks as insertion.
    ///
    /// Exactly one root, unique IDs and paths, and valid driver nesting declarations
    /// are required. Missing parent-driver policies fail without dropping any records.
    pub fn restore(
        mut mounts: Vec<Mount>,
        policy: impl Fn(&DriverKey) -> Option<ChildMountPolicy>,
    ) -> Result<Self, MountTableError> {
        mounts.sort_by_key(|mount| mount.virtual_path().as_str().len());
        let mut mounts = mounts.into_iter();
        let root = mounts.next().ok_or(MountTableError::RootRequired)?;
        let mut table = Self::new(root)?;
        for mount in mounts {
            table.insert(mount, &policy)?;
        }
        Ok(table)
    }

    pub fn root(&self) -> &Mount {
        &self.root
    }

    /// Add a mount after checking identity, path conflicts and driver nesting rules.
    ///
    /// `policy` supplies driver declarations without coupling the domain to a catalog.
    /// The nearest parent must allow children. A mount inserted above existing mounts
    /// must also allow children. Missing policies fail before changing the table.
    pub fn insert(
        &mut self,
        mount: Mount,
        policy: impl Fn(&DriverKey) -> Option<ChildMountPolicy>,
    ) -> Result<(), MountTableError> {
        if self.get(mount.id()).is_some() {
            return Err(MountTableError::DuplicateId(mount.id()));
        }
        if self.get_by_path(mount.virtual_path()).is_some() {
            return Err(MountTableError::PathOccupied(mount.virtual_path().clone()));
        }

        let (parent, _) = self.closest(mount.virtual_path());
        Self::require_child_mounts(parent, &policy)?;

        if self
            .mounts
            .iter()
            .any(|existing| mount.virtual_path().contains(existing.virtual_path()))
        {
            Self::require_child_mounts(&mount, &policy)?;
        }

        self.mounts.push(mount);
        Ok(())
    }

    /// Remove one non-root mount; nested mounts keep their IDs and virtual paths.
    pub fn remove(&mut self, id: MountId) -> Result<Mount, MountTableError> {
        if self.root.id() == id {
            return Err(MountTableError::RootCannotBeRemoved);
        }
        let index = self
            .mounts
            .iter()
            .position(|mount| mount.id() == id)
            .ok_or(MountTableError::NotFound(id))?;
        Ok(self.mounts.remove(index))
    }

    pub fn get(&self, id: MountId) -> Option<&Mount> {
        self.iter().find(|mount| mount.id() == id)
    }

    /// Find a mount at exactly this path, including disabled mounts.
    pub fn get_by_path(&self, path: &VirtualPath) -> Option<&Mount> {
        self.iter().find(|mount| mount.virtual_path() == path)
    }

    /// List all mount definitions, including disabled ones, in virtual path order.
    pub fn list(&self) -> Vec<&Mount> {
        let mut mounts: Vec<_> = self.iter().collect();
        mounts.sort_unstable_by(|left, right| {
            left.virtual_path()
                .as_str()
                .cmp(right.virtual_path().as_str())
        });
        mounts
    }

    pub fn enable(&mut self, id: MountId) -> Result<(), MountTableError> {
        self.set_enabled(id, true)
    }

    /// Keep the mount boundary while making requests routed to it fail.
    ///
    /// More-specific mounts retain their own enabled state and remain independently
    /// addressable. Disabling the root preserves the required root registration.
    pub fn disable(&mut self, id: MountId) -> Result<(), MountTableError> {
        self.set_enabled(id, false)
    }

    /// Select the deepest whole-segment match and borrow its relative path.
    ///
    /// A disabled match returns an error instead of falling back to a parent.
    pub fn resolve<'mount, 'path>(
        &'mount self,
        path: &'path VirtualPath,
    ) -> Result<MountResolution<'mount, 'path>, MountTableError> {
        let (mount, relative_path) = self.closest(path);
        if !mount.enabled() {
            return Err(MountTableError::Disabled(mount.id()));
        }
        Ok(MountResolution {
            mount,
            relative_path,
        })
    }

    fn iter(&self) -> impl Iterator<Item = &Mount> {
        std::iter::once(&self.root).chain(self.mounts.iter())
    }

    fn closest<'mount, 'path>(
        &'mount self,
        path: &'path VirtualPath,
    ) -> (&'mount Mount, &'path str) {
        let mut closest = &self.root;
        // Every VirtualPath is absolute and this table always owns a root mount.
        let mut relative_path = &path.as_str()[1..];
        for mount in &self.mounts {
            if mount.virtual_path().as_str().len() > closest.virtual_path().as_str().len()
                && let Some(suffix) = path.strip_prefix(mount.virtual_path())
            {
                closest = mount;
                relative_path = suffix;
            }
        }
        (closest, relative_path)
    }

    fn require_child_mounts(
        mount: &Mount,
        policy: &impl Fn(&DriverKey) -> Option<ChildMountPolicy>,
    ) -> Result<(), MountTableError> {
        let driver = mount.driver_instance().driver_key();
        match policy(driver) {
            Some(ChildMountPolicy::Allow) => Ok(()),
            Some(ChildMountPolicy::Deny) => Err(MountTableError::ChildMountsForbidden {
                driver: driver.clone(),
                path: mount.virtual_path().clone(),
            }),
            None => Err(MountTableError::PolicyUnavailable(driver.clone())),
        }
    }

    fn set_enabled(&mut self, id: MountId, enabled: bool) -> Result<(), MountTableError> {
        let mount = if self.root.id() == id {
            &mut self.root
        } else {
            self.mounts
                .iter_mut()
                .find(|mount| mount.id() == id)
                .ok_or(MountTableError::NotFound(id))?
        };
        mount.set_enabled(enabled);
        Ok(())
    }
}
