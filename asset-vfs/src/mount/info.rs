use getset::{CopyGetters, Getters};

use crate::mount::Mount;

/// A persisted mount enriched with declarations from [`crate::DriverService`].
///
/// This is query data; driver metadata is not part of the persisted definition.
#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters)]
pub struct MountInfo {
    #[getset(get = "pub")]
    mount: Mount,
    /// `None` when this mount's driver kind is not registered.
    #[getset(get_copy = "pub")]
    allows_submounts: Option<bool>,
}

impl MountInfo {
    pub(crate) fn new(mount: Mount, allows_submounts: Option<bool>) -> Self {
        Self {
            mount,
            allows_submounts,
        }
    }
}
