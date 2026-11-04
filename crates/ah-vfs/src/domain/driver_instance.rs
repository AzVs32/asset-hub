use super::{DriverKey, InstanceId};
use getset::Getters;

/// The persistent identity of a host-owned instance scoped to a particular driver.
///
/// This value contains no configuration or live storage handle.
/// An instance identifier is scoped to its driver; both fields define identity.
#[derive(Clone, Debug, Eq, Getters, Hash, PartialEq)]
pub struct DriverInstance {
    #[getset(get = "pub")]
    driver_key: DriverKey,
    #[getset(get = "pub")]
    instance_id: InstanceId,
}

impl DriverInstance {
    pub fn new(driver_key: DriverKey, instance_id: InstanceId) -> Self {
        Self {
            driver_key,
            instance_id,
        }
    }
}
