//! Driver-independent instance views and mount-reference protection.

use crate::{
    application::{
        dto::{
            InstanceConfig, InstanceConfigSchema, InstanceInfo, InstancePage, InstanceQuery,
            MountFilter, RemoveInstanceOutcome,
        },
        error::InstanceError,
        mount_runtime::MountRuntime,
        port::Driver,
    },
    domain::{DriverError, DriverInstance, DriverKey, StorageError, StorageErrorKind},
};
use std::sync::Arc;

/// Host-facing instance management using only registered driver ports.
/// Mount references, including disabled ones, prevent removal. Changes to the
/// namespace and removal must be serialized by the host; these independent
/// persistence boundaries do not form a cross-process transaction.
pub struct InstanceService {
    runtime: Arc<MountRuntime>,
}

impl InstanceService {
    pub fn new(runtime: Arc<MountRuntime>) -> Self {
        Self { runtime }
    }

    fn driver(&self, key: &DriverKey) -> Result<&Arc<dyn Driver>, InstanceError> {
        self.runtime
            .drivers
            .get(key)
            .ok_or_else(|| InstanceError::DriverNotRegistered(key.clone()))
    }

    /// Discover one driver's creation fields, without knowing its concrete type.
    pub fn instance_config_schema(
        &self,
        driver_key: &DriverKey,
    ) -> Result<Option<InstanceConfigSchema>, InstanceError> {
        Ok(self
            .driver(driver_key)?
            .info()
            .instance_config_schema()
            .cloned())
    }

    /// Select the driver by key, then forward parameters for authoritative validation.
    /// No instance ID exists before creation. Combine the selected key and returned
    /// ID into DriverInstance for subsequent operations. Never open storage here.
    pub async fn create_instance(
        &self,
        driver_key: &DriverKey,
        config: InstanceConfig,
    ) -> Result<InstanceInfo, InstanceError> {
        let driver = self.driver(driver_key)?;
        if driver.info().instance_config_schema().is_none() {
            return Err(DriverError::Storage(StorageError::new(
                StorageErrorKind::UnsupportedOperation,
            ))
            .into());
        }
        Ok(self
            .runtime
            .host_registry(driver.as_ref())
            .create(config)
            .await?)
    }

    /// Read the driver's public view without opening storage or checking health.
    pub async fn get_instance(
        &self,
        instance: &DriverInstance,
    ) -> Result<Option<InstanceInfo>, InstanceError> {
        Ok(self
            .runtime
            .host_registry(self.driver(instance.driver_key())?.as_ref())
            .get(instance.instance_id().clone())
            .await?)
    }

    /// Query one driver's ID-ordered live page of registered instances.
    pub async fn list_instances(
        &self,
        driver_key: &DriverKey,
        query: InstanceQuery,
    ) -> Result<InstancePage, InstanceError> {
        Ok(self
            .runtime
            .host_registry(self.driver(driver_key)?.as_ref())
            .list(query)
            .await?)
    }

    /// Replace instance configuration. Immutable fields cannot
    /// change; mount references permit mutable updates. Existing cache invalidation also covers errors/cancellation.
    pub async fn update_instance(
        &self,
        instance: &DriverInstance,
        config: InstanceConfig,
    ) -> Result<InstanceInfo, InstanceError> {
        let driver = self.driver(instance.driver_key())?;
        let registry = self.runtime.host_registry(driver.as_ref());
        let _invalidation = self.runtime.changing_instance(instance.clone());
        Ok(registry
            .update(instance.instance_id().clone(), config)
            .await?)
    }

    /// Remove an unreferenced registration, retaining backend and session semantics
    /// defined by Driver. Never unmount, open storage, or infer absence from errors.
    /// Referenced IDs are rejected even if their driver record is already absent.
    /// Evict runtime retention before/after removal, including errors and cancellation.
    pub async fn remove_instance(
        &self,
        instance: &DriverInstance,
    ) -> Result<RemoveInstanceOutcome, InstanceError> {
        let driver = self.driver(instance.driver_key())?;
        let mut mounts = self
            .runtime
            .repository
            .list(MountFilter::default().driver_key(instance.driver_key().clone()))
            .await?;
        mounts.retain(|mount| mount.driver_instance() == instance);
        mounts.sort_by(|a, b| a.virtual_path().as_str().cmp(b.virtual_path().as_str()));
        if !mounts.is_empty() {
            return Err(InstanceError::InstanceInUse {
                instance: instance.clone(),
                mounts: mounts.into_iter().map(|mount| mount.id()).collect(),
            });
        }
        let _invalidation = self.runtime.changing_instance(instance.clone());
        Ok(self
            .runtime
            .host_registry(driver.as_ref())
            .remove(instance.instance_id().clone())
            .await?)
    }
}
