//! Requests, filters and views for mount management.

use crate::domain::{DriverInstance, DriverKey, Mount, MountId, VirtualPath};

/// Attach an existing driver instance at a virtual path, initially enabled.
pub struct MountRequest {
    pub virtual_path: VirtualPath,
    pub driver_instance: DriverInstance,
}

/// Persisted mount settings; this view does not imply backend health.
#[derive(Debug)]
pub struct MountView {
    pub id: MountId,
    pub virtual_path: VirtualPath,
    pub driver_instance: DriverInstance,
    pub enabled: bool,
}

impl From<Mount> for MountView {
    fn from(mount: Mount) -> Self {
        Self {
            id: mount.id(),
            virtual_path: mount.virtual_path().clone(),
            driver_instance: mount.driver_instance().clone(),
            enabled: mount.enabled(),
        }
    }
}

/// Optional mount criteria, combined with AND. The default matches all mounts.
#[derive(Debug, Default)]
#[must_use]
pub struct MountFilter {
    driver_key: Option<DriverKey>,
    enabled: Option<bool>,
}

impl MountFilter {
    /// Restrict results to the exact, case-sensitive driver identity.
    pub fn driver_key(mut self, key: DriverKey) -> Self {
        self.driver_key = Some(key);
        self
    }

    /// Restrict results to the given configured enabled state.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    pub fn get_driver_key(&self) -> Option<&DriverKey> {
        self.driver_key.as_ref()
    }

    pub fn get_enabled(&self) -> Option<bool> {
        self.enabled
    }
}
