use crate::domain::internal::MountIdError;
use crate::domain::{DriverKind, DriverPath, VirtualPath};
use crate::error::CoreError;
use getset::{CopyGetters, Getters};

#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters)]
pub struct Mount {
    #[getset(get_copy = "pub")]
    id: MountId,
    #[getset(get = "pub")]
    virtual_path: VirtualPath,
    #[getset(get = "pub")]
    driver: DriverKind,
    #[getset(get = "pub")]
    driver_path: DriverPath,
    #[getset(get_copy = "pub")]
    enabled: bool,
}

impl Mount {
    /// Creates a mount; its driver validates the driver path when bound.
    pub fn new(
        id: MountId,
        virtual_path: VirtualPath,
        driver: DriverKind,
        driver_path: DriverPath,
        enabled: bool,
    ) -> Self {
        Self {
            id,
            virtual_path,
            driver,
            driver_path,
            enabled,
        }
    }

    /// Enables this mount.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disables this mount.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Returns whether the path is located at or below this mount point.
    pub fn covers(&self, path: &VirtualPath) -> bool {
        self.virtual_path.is_ancestor_or_self_of(path)
    }
}

utils::gen_id_uuid_v7!(MountId, error = CoreError, invalid = MountIdError.into());

#[cfg(test)]
mod tests;
