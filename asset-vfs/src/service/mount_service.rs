use std::sync::Arc;

use crate::DriverService;
use crate::error::VfsError;
use crate::mount::{Mount, MountError, MountInfo, MountRepository, MountSelector, ResolvedMount};
use crate::namespace::VirtualPath;

/// Public entry point for managing mounts.
///
/// Queries read the current persisted definitions, including disabled mounts.
/// Driver declarations come from the shared DriverService without binding backends
/// or checking backend availability. Writes validate enabled-path uniqueness
/// and submount policies inside a serialized repository transaction.
pub struct MountService {
    repository: Arc<dyn MountRepository>,
    drivers: Arc<DriverService>,
}

impl MountService {
    /// Creates a service using the mount repository and shared driver service.
    pub fn new(repository: Arc<dyn MountRepository>, drivers: Arc<DriverService>) -> Self {
        Self {
            repository,
            drivers,
        }
    }

    /// Saves a new definition or updates the definition with the same ID.
    ///
    /// Enabled definitions are bound to validate their backend roots before
    /// commit. All definitions require a registered kind and valid driver-path
    /// syntax; disabled definitions can refer to unavailable roots.
    /// Re-enabling retains the original ID. All enabled
    /// ancestors must allow submounts; disabled ancestors impose no restriction.
    pub async fn mount(&self, mount: Mount) -> Result<MountInfo, VfsError> {
        let mut transaction = self.repository.begin().await?;
        let mut mounts = transaction.list().await?;
        let existing = mounts.iter().position(|current| current.id() == mount.id());
        if let Some(index) = existing {
            let current = &mounts[index];
            if current.enabled()
                && current.virtual_path().is_root()
                && (!mount.enabled() || !mount.virtual_path().is_root())
            {
                return Err(MountError::RootRequired.into());
            }
            mounts[index] = mount.clone();
        } else {
            mounts.push(mount.clone());
        }
        self.drivers
            .validate_path(mount.driver(), mount.driver_path())?;
        self.validate_topology(&mounts)?;
        if mount.enabled() {
            self.drivers.bind(mount.driver(), mount.driver_path())?;
        }
        if existing.is_some() {
            transaction.update(&mount).await?;
        } else {
            transaction.insert(&mount).await?;
        }
        transaction.commit().await?;
        Ok(self.mount_info_from_definition(mount))
    }

    /// Disables a definition without deleting it or its descendant definitions.
    ///
    /// An exact path selects the enabled definition, or a single disabled one.
    /// Multiple disabled definitions require an ID. Unknown selectors return
    /// None. Already disabled definitions remain unchanged. The enabled root
    /// cannot be disabled.
    pub async fn unmount(
        &self,
        selector: impl Into<MountSelector>,
    ) -> Result<Option<MountInfo>, VfsError> {
        let selector = selector.into();
        let mut transaction = self.repository.begin().await?;
        let mounts = transaction.list().await?;
        let Some(mut mount) = selector.select(&mounts)?.cloned() else {
            return Ok(None);
        };
        if mount.enabled() {
            if mount.virtual_path().is_root() {
                return Err(MountError::RootRequired.into());
            }
            mount.disable();
            transaction.update(&mount).await?;
        }
        transaction.commit().await?;
        Ok(Some(self.mount_info_from_definition(mount)))
    }

    /// Lists all configured mounts in ascending virtual-path order.
    ///
    /// Includes disabled mounts and their driver declarations. Unregistered
    /// kinds have unknown declarations. Repository errors are returned unchanged.
    pub async fn list_mounts(&self) -> Result<Vec<MountInfo>, VfsError> {
        Ok(self
            .repository
            .list()
            .await?
            .into_iter()
            .map(|mount| self.mount_info_from_definition(mount))
            .collect())
    }

    /// Returns information about one mount.
    ///
    /// Accepts an exact virtual path or an ID. At a path, prefer its enabled
    /// definition, otherwise return the single disabled one or AmbiguousPath.
    /// Returns None when absent. Unregistered kinds have unknown declarations.
    /// Repository errors are returned unchanged. Drivers are not bound.
    pub async fn mount_info(
        &self,
        selector: impl Into<MountSelector>,
    ) -> Result<Option<MountInfo>, VfsError> {
        let mount = match selector.into() {
            MountSelector::Id(id) => self.repository.get(id).await?,
            selector @ MountSelector::Path(_) => {
                selector.select(&self.repository.list().await?)?.cloned()
            }
        };
        Ok(mount.map(|mount| self.mount_info_from_definition(mount)))
    }

    fn mount_info_from_definition(&self, mount: Mount) -> MountInfo {
        let allows_submounts = self
            .drivers
            .info(mount.driver())
            .map(|driver| driver.allows_submounts());
        MountInfo::new(mount, allows_submounts)
    }

    /// Selects the deepest enabled mount covering a request path.
    ///
    /// Ignores disabled definitions, compares whole path segments, and returns
    /// an owned mount snapshot with the relative path. No backend is bound.
    pub async fn resolve_mount(
        &self,
        path: &VirtualPath,
    ) -> Result<Option<ResolvedMount>, VfsError> {
        let selected = self
            .repository
            .list()
            .await?
            .into_iter()
            .filter(|mount| mount.enabled() && mount.covers(path))
            .max_by_key(|mount| mount.virtual_path().depth());
        Ok(selected.and_then(|mount| ResolvedMount::new(mount, path)))
    }

    fn validate_topology(&self, mounts: &[Mount]) -> Result<(), VfsError> {
        let enabled: Vec<_> = mounts.iter().filter(|mount| mount.enabled()).collect();
        for (index, mount) in enabled.iter().enumerate() {
            for other in &enabled[index + 1..] {
                if mount.virtual_path() == other.virtual_path() {
                    return Err(MountError::DuplicatePath(mount.virtual_path().clone()).into());
                }
            }
            if enabled
                .iter()
                .any(|child| mount.virtual_path().is_ancestor_of(child.virtual_path()))
                && !self.drivers.require(mount.driver())?.allows_submounts()
            {
                return Err(MountError::SubmountNotAllowed(mount.virtual_path().clone()).into());
            }
        }
        Ok(())
    }
}
