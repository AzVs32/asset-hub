use super::{DriverKey, InstanceId};
use getset::Getters;

/// Identifies a host-owned instance record scoped to a particular driver.
///
/// This persistent reference contains no configuration or live storage handle.
/// An instance identifier is scoped to its driver; both fields define identity.
#[derive(Clone, Debug, Eq, Getters, Hash, PartialEq)]
pub struct DriverInstanceRef {
    #[getset(get = "pub")]
    driver_key: DriverKey,
    #[getset(get = "pub")]
    instance_id: InstanceId,
}

impl DriverInstanceRef {
    pub fn new(driver_key: DriverKey, instance_id: InstanceId) -> Self {
        Self {
            driver_key,
            instance_id,
        }
    }
}
