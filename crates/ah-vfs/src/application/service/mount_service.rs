//! Mount management use cases.

use crate::{
    application::{
        dto::{MountFilter, MountRequest, MountView},
        error::MountError,
        mount_runtime::MountRuntime,
    },
    domain::{
        DriverInstance,
        namespace::{Mount, MountId, MountTable, VirtualPath},
    },
};

use std::sync::Arc;

pub struct MountService {
    runtime: Arc<MountRuntime>,
}

impl MountService {
    pub fn new(runtime: Arc<MountRuntime>) -> Self {
        Self { runtime }
    }

    /// Initialize an empty namespace with an enabled root referencing an existing instance.
    ///
    /// Existing definitions are validated and retained, even if `instance` differs.
    /// Invalid non-empty data is reported, never replaced. This does not create
    /// or open a driver instance.
    pub async fn initialize_root(&self, instance: DriverInstance) -> Result<MountId, MountError> {
        let mounts = self.runtime.repository.list(MountFilter::default()).await?;
        if !mounts.is_empty() {
            return Ok(self.runtime.restore(mounts)?.root().id());
        }
        if self.runtime.drivers.get(instance.driver_key()).is_none() {
            return Err(MountError::DriverNotRegistered(
                instance.driver_key().clone(),
            ));
        }

        let root = Mount::builder()
            .id(MountId::new())
            .virtual_path(VirtualPath::root())
            .driver_instance(instance)
            .enabled(true)
            .build();
        let id = root.id();
        let _invalidation = self.runtime.changing_mounts();
        self.runtime.repository.insert(root).await?;
        Ok(id)
    }

    /// Validate namespace rules and obtain a validated instance handle before
    /// persisting a mount. Aliases reuse the runtime's retained storage handle.
    ///
    /// An empty namespace accepts only `/`. Concurrent domain changes are not
    /// coordinated across service instances or host processes.
    pub async fn mount(&self, request: MountRequest) -> Result<MountView, MountError> {
        let driver = self
            .runtime
            .drivers
            .get(request.driver_instance.driver_key())
            .ok_or_else(|| {
                MountError::DriverNotRegistered(request.driver_instance.driver_key().clone())
            })?;
        let mount = Mount::builder()
            .id(MountId::new())
            .virtual_path(request.virtual_path)
            .driver_instance(request.driver_instance)
            .enabled(true)
            .build();
        let mounts = self.runtime.repository.list(MountFilter::default()).await?;
        if mounts.is_empty() {
            MountTable::new(mount.clone())?;
        } else {
            let mut table = self.runtime.restore(mounts)?;
            table.insert(mount.clone(), |key| {
                self.runtime
                    .drivers
                    .get(key)
                    .map(|driver| driver.info().child_mount_policy())
            })?;
        }
        self.runtime
            .storage(mount.driver_instance(), driver.as_ref())
            .await?;
        let _invalidation = self.runtime.changing_mounts();
        self.runtime.repository.insert(mount.clone()).await?;
        Ok(mount.into())
    }

    /// Remove a mount without deleting its driver instance.
    pub async fn unmount(&self, id: MountId) -> Result<(), MountError> {
        let mut table = self
            .runtime
            .restore(self.runtime.repository.list(MountFilter::default()).await?)?;
        table.remove(id)?;
        let _invalidation = self.runtime.changing_mounts();
        self.runtime.repository.remove(id).await?;
        Ok(())
    }

    /// Enable an existing mount.
    pub async fn enable(&self, id: MountId) -> Result<MountView, MountError> {
        self.set_enabled(id, true).await
    }

    /// Disable a mount while retaining its definition.
    pub async fn disable(&self, id: MountId) -> Result<MountView, MountError> {
        self.set_enabled(id, false).await
    }

    /// Retrieve persisted settings without opening the backing instance.
    pub async fn get_mount(&self, id: MountId) -> Result<MountView, MountError> {
        let mount = self.runtime.repository.get(id).await?.ok_or(
            crate::application::error::MountRepositoryError::NotFound(id),
        )?;
        Ok(mount.into())
    }

    /// List mounts matching all supplied filter conditions.
    pub async fn list_mounts(&self, filter: MountFilter) -> Result<Vec<MountView>, MountError> {
        Ok(self
            .runtime
            .repository
            .list(filter)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn set_enabled(&self, id: MountId, enabled: bool) -> Result<MountView, MountError> {
        let mut table = self
            .runtime
            .restore(self.runtime.repository.list(MountFilter::default()).await?)?;
        if enabled {
            table.enable(id)?;
        } else {
            table.disable(id)?;
        }
        let _invalidation = self.runtime.changing_mounts();
        self.runtime.repository.set_enabled(id, enabled).await?;
        self.get_mount(id).await
    }
}
