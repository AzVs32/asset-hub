mod d_path;
mod driver_kind;
mod v_path;

use getset::{CopyGetters, Getters};

pub use d_path::DPath;
pub use driver_kind::DriverKind;
pub use v_path::VPath;

#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters)]
pub struct Mount {
    #[getset(get_copy = "pub")]
    id: MountId,
    #[getset(get = "pub")]
    v_path: VPath,
    #[getset(get_copy = "pub")]
    driver: DriverKind,
    #[getset(get_copy = "pub")]
    d_path: DPath,
    #[getset(get_copy = "pub")]
    enabled: bool,
}

impl Mount {
    /// Creates a mount from validated domain values.
    pub fn new(
        id: MountId,
        v_path: VPath,
        driver: DriverKind,
        d_path: DPath,
        enabled: bool,
    ) -> Self {
        Self {
            id,
            v_path,
            driver,
            d_path,
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
    pub fn covers(&self, path: &VPath) -> bool {
        self.v_path.is_ancestor_or_self_of(path)
    }
}

utils::gen_id_uuid_v7!(MountId);

#[cfg(test)]
mod tests;
