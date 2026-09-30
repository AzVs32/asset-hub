use getset::{CopyGetters, Getters};

use crate::driver::DriverKind;

/// Driver declarations returned by [`crate::DriverService`].
///
/// This is an owned metadata snapshot, independent of any bound backend.
#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters)]
pub struct DriverInfo {
    #[getset(get = "pub")]
    kind: DriverKind,
    #[getset(get_copy = "pub")]
    allows_submounts: bool,
}

impl DriverInfo {
    pub(crate) fn new(kind: DriverKind, allows_submounts: bool) -> Self {
        Self {
            kind,
            allows_submounts,
        }
    }
}
