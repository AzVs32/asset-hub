//! In-memory registration of available driver implementations.

use crate::application::error::DriverCatalogError;
use crate::application::port::Driver;
use crate::domain::{DriverKey, StorageError};
use std::collections::HashMap;
use std::{fmt, sync::Arc};

/// Driver registrations for the current host process, indexed by stable keys.
///
/// Drivers own their declarations and behavior. Live instances are managed
/// separately from registrations, and borrowed handles may be cloned by callers.
#[derive(Default)]
pub struct DriverCatalog {
    drivers: HashMap<DriverKey, Arc<dyn Driver>>,
}

impl DriverCatalog {
    /// Create an empty catalog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize once and register only on success. Duplicate keys are rejected
    /// before invoking initialization. Settings are never retained by the catalog.
    /// The host must not initialize the same driver object independently/repeatedly.
    pub async fn register(
        &mut self,
        driver: Arc<dyn Driver>,
        settings: crate::application::dto::DriverSettings,
    ) -> Result<(), DriverCatalogError> {
        let key = driver.info().key().clone();
        if self.drivers.contains_key(&key) {
            return Err(DriverCatalogError::AlreadyRegistered(key));
        }
        driver.info().capabilities().validate().map_err(|error| {
            DriverCatalogError::InvalidDeclaration(StorageError::from(error).into())
        })?;
        if let Some(schema) = driver.info().instance_config_schema() {
            super::schema::validate_instance_config_schema(schema)
                .map_err(DriverCatalogError::InvalidDeclaration)?;
        }
        driver
            .initialize(settings)
            .await
            .map_err(DriverCatalogError::Initialization)?;
        self.drivers.insert(key, driver);
        Ok(())
    }

    /// Remove and return a registration, or fail if the key is unknown.
    ///
    /// The shared handle is returned; existing driver and storage handles stay valid.
    /// Plugin unloading requires a separate lifecycle policy.
    pub fn unregister(&mut self, key: &DriverKey) -> Result<Arc<dyn Driver>, DriverCatalogError> {
        self.drivers
            .remove(key)
            .ok_or_else(|| DriverCatalogError::NotRegistered(key.clone()))
    }

    /// Borrow a registered driver, or return `None` for an unknown key.
    pub fn get(&self, key: &DriverKey) -> Option<&Arc<dyn Driver>> {
        self.drivers.get(key)
    }

    /// List registered drivers in ascending, case-sensitive key order.
    pub fn list(&self) -> Vec<&Arc<dyn Driver>> {
        let mut drivers: Vec<_> = self.drivers.iter().collect();
        drivers.sort_unstable_by(|left, right| left.0.as_str().cmp(right.0.as_str()));
        drivers.into_iter().map(|(_, driver)| driver).collect()
    }
}

impl fmt::Debug for DriverCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let declarations: Vec<_> = self.list().iter().map(|driver| driver.info()).collect();
        formatter
            .debug_struct("DriverCatalog")
            .field("drivers", &declarations)
            .finish()
    }
}
