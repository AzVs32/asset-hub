//! Driver declarations, stateless preparation and storage opening.

use crate::{
    application::{driver_info::DriverInfo, port::Storage},
    domain::DriverError,
};

use std::{future::Future, pin::Pin, sync::Arc};

pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, DriverError>> + Send + 'a>>;

/// One driver's declarations, configuration validation and storage opening.
///
/// Creation parameters remain driver-specific, behind a shared object contract.
/// The host owns instance records; drivers never persist their lifecycle.
/// Storage handles and their streams must keep backend resources alive.
/// Plugin adapters follow the loading and resource contract in this port module.
pub trait Driver: Send + Sync {
    /// Borrow this driver's declarations. All fields must remain stable for the
    /// driver's lifetime, including while it is registered or opened handles exist.
    /// Remote adapters fetch and validate these before construction; this getter
    /// never performs I/O and initialization must not replace the declarations.
    fn info(&self) -> &DriverInfo;

    /// Initialize plugin-wide resources once before registration. Settings are
    /// independent of host and instance configuration; drivers may connect to any
    /// database/transport they own. Errors must not disclose credentials. Failure
    /// leaves the driver unregistered; cancellation may require driver-specific
    /// cleanup before retry. The default accepts empty settings only for drivers
    /// without a settings schema (such as already-assembled built-ins).
    fn initialize(
        &self,
        settings: crate::application::dto::DriverSettings,
    ) -> DriverFuture<'_, ()> {
        let supported = self.info().settings_schema().is_none() && settings.is_empty();
        Box::pin(async move {
            if supported {
                Ok(())
            } else {
                Err(crate::domain::storage::StorageError::new(
                    crate::domain::storage::StorageErrorKind::UnsupportedOperation,
                )
                .into())
            }
        })
    }

    /// Return stateless configuration validation and connectivity logic.
    fn provisioner(&self) -> Arc<dyn super::InstanceProvisioner>;

    /// Open the referenced instance. This does not create an instance or manage
    /// its configuration. The host supplies validated, decrypted parameters.
    /// This is a cold path that may perform backend probes;
    /// the host's MountRuntime retains validated handles for repeated I/O.
    /// All instances share info().capabilities(); declared independent attribute
    /// interfaces must be present. Instance/path limitations are operation errors.
    /// Backend
    /// failures use DriverError::Storage and retain the same domain categories
    /// used during I/O. Opening never selects file formats or viewers.
    fn open(&self, context: super::OpenContext) -> DriverFuture<'_, Arc<dyn Storage>>;
}
