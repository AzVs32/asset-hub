use std::error::Error;

use crate::mount::domain::Mount;

/// Binds a mount configuration to a driver-specific backend.
pub trait MountBinder {
    /// The backend handle produced by this driver.
    type Backend;

    /// The error returned when the mount cannot be bound.
    type Error: Error + Send + Sync + 'static;

    /// Validates the driver's configuration and creates a bound backend.
    fn bind(&self, mount: &Mount) -> Result<Self::Backend, Self::Error>;
}
