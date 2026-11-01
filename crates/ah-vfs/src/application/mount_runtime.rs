//! Process-local namespace snapshots and validated storage handles shared by services.

use crate::{
    application::{
        driver_catalog::DriverCatalog,
        error::EntryError,
        port::{Driver, MountRepository, Storage, validate_storage_capabilities},
    },
    domain::{DriverError, DriverInstanceRef, Mount, MountTable, MountTableError},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};
use tokio::sync::OnceCell;

type StorageCell = Arc<OnceCell<Arc<dyn Storage>>>;

#[derive(Default)]
struct Cache {
    table: Arc<OnceCell<Arc<MountTable>>>,
    storage: HashMap<DriverInstanceRef, StorageCell>,
}

/// Shared state for one host's mount namespace. Construct once and pass the same
/// Arc to EntryService, MountService and InstanceService. The first access loads
/// and validates a mount snapshot; each instance is opened and validated once.
/// Concurrent cold accesses share initialization without serializing other IDs.
/// Failed or cancelled initialization can be retried and is never cached as success.
/// Handles reporting a stale runtime generation are evicted before the next open.
///
/// Service mutations invalidate the affected cache. Already-started operations
/// and streams may retain their previous snapshot/handle. This cache does not
/// coordinate namespace mutations with I/O or with another process. After external
/// persistence/configuration changes, call invalidate() or invalidate_instance().
/// Uncertain or cancelled writes still require reconciliation; invalidate again
/// if their final effects are discovered later.
/// Retained storage handles are released on invalidation or runtime drop; this
/// does not revoke any handles or streams already owned by callers.
pub struct MountRuntime {
    pub(crate) repository: Arc<dyn MountRepository>,
    pub(crate) drivers: Arc<DriverCatalog>,
    cache: Mutex<Cache>,
    records: Arc<dyn super::port::InstanceRecordRepository>,
    cipher: Option<Arc<dyn super::port::SecretCipher>>,
}

impl MountRuntime {
    /// Host record persistence is required; encryption is optional for secret-free schemas.
    pub fn new(
        repository: Arc<dyn MountRepository>,
        drivers: Arc<DriverCatalog>,
        records: Arc<dyn super::port::InstanceRecordRepository>,
        cipher: Option<Arc<dyn super::port::SecretCipher>>,
    ) -> Self {
        Self {
            repository,
            drivers,
            records,
            cipher,
            cache: Mutex::new(Cache::default()),
        }
    }
    pub(crate) fn host_registry(
        &self,
        driver: &dyn Driver,
    ) -> super::host_instance_registry::HostInstanceRegistry {
        super::host_instance_registry::HostInstanceRegistry::new(
            driver.info(),
            driver.provisioner(),
            self.records.clone(),
            self.cipher.clone(),
        )
    }
    fn cache(&self) -> MutexGuard<'_, Cache> {
        // Cache edits contain no user code. Recover this disposable state if a
        // thread was interrupted while holding its short, synchronous lock.
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Forget the namespace and all retained storage handles. New requests reload
    /// persistence and reopen drivers. Existing operations retain their handles.
    pub fn invalidate(&self) {
        let previous = std::mem::take(&mut *self.cache());
        // Driver-owned destructors must run outside the cache lock.
        drop(previous);
    }

    /// Forget a storage handle after external configuration changes or removal.
    /// This neither removes the registration nor changes mount definitions.
    pub fn invalidate_instance(&self, instance: &DriverInstanceRef) {
        let previous = self.cache().storage.remove(instance);
        drop(previous);
    }

    fn invalidate_mounts(&self) {
        self.cache().table = Arc::new(OnceCell::new());
    }

    pub(crate) fn restore(&self, mounts: Vec<Mount>) -> Result<MountTable, MountTableError> {
        MountTable::restore(mounts, |key| {
            self.drivers
                .get(key)
                .map(|driver| driver.info().child_mount_policy())
        })
    }

    pub(crate) async fn table(&self) -> Result<Arc<MountTable>, EntryError> {
        let cell = self.cache().table.clone();
        cell.get_or_try_init(|| async {
            let mounts = self.repository.list(Default::default()).await?;
            Ok(Arc::new(self.restore(mounts)?))
        })
        .await
        .cloned()
    }

    pub(crate) async fn storage(
        &self,
        instance: &DriverInstanceRef,
        driver: &dyn Driver,
    ) -> Result<Arc<dyn Storage>, DriverError> {
        let (cell, retired) = {
            let mut cache = self.cache();
            let retired = if cache
                .storage
                .get(instance)
                .and_then(|cell| cell.get())
                .is_some_and(|storage| storage.is_stale())
            {
                cache.storage.remove(instance)
            } else {
                None
            };
            (
                cache.storage.entry(instance.clone()).or_default().clone(),
                retired,
            )
        };
        drop(retired);
        cell.get_or_try_init(|| async {
            let context = self
                .host_registry(driver)
                .open_context(instance.instance_id().clone())
                .await?;
            let storage = driver.open(context).await?;
            validate_storage_capabilities(driver.info().capabilities(), storage.as_ref())?;
            Ok(storage)
        })
        .await
        .cloned()
    }

    pub(crate) fn changing_mounts(&self) -> Invalidation<'_> {
        let guard = Invalidation {
            runtime: self,
            instance: None,
        };
        guard.invalidate();
        guard
    }

    pub(crate) fn changing_instance(&self, instance: DriverInstanceRef) -> Invalidation<'_> {
        let guard = Invalidation {
            runtime: self,
            instance: Some(instance),
        };
        guard.invalidate();
        guard
    }
}

/// Invalidate before and after a write, including errors, uncertain outcomes and
/// cancellation. Detached OnceCells cannot republish old results into the cache.
pub(crate) struct Invalidation<'a> {
    runtime: &'a MountRuntime,
    instance: Option<DriverInstanceRef>,
}

impl Invalidation<'_> {
    fn invalidate(&self) {
        if let Some(instance) = &self.instance {
            self.runtime.invalidate_instance(instance);
        } else {
            self.runtime.invalidate_mounts();
        }
    }
}

impl Drop for Invalidation<'_> {
    fn drop(&mut self) {
        self.invalidate();
    }
}

#[cfg(test)]
mod tests;
