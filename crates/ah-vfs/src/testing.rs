//! Reusable behavioral assertions for instance registries and storage implementations.

mod storage_contract;

pub use storage_contract::{
    assert_create_dir_contract, assert_pagination_contract, assert_range_read_contract,
    assert_same_path_rename_contract,
};

use crate::application::driver_error::{DriverError, LifecycleError, LifecycleFailure};
use crate::application::dto::{InstanceConfig, InstanceQuery, RemoveInstanceOutcome};
use crate::application::port::DriverFuture;
use crate::domain::InstanceId;
use std::{error::Error, num::NonZeroUsize, sync::Arc};

/// Isolated test fixture. Hooks affect its own records/database only.
pub trait RegistryFixture: Send + Sync {
    fn registry(&self) -> Arc<crate::application::host_instance_registry::HostInstanceRegistry>;
    fn config(&self) -> InstanceConfig;
    /// Make backend storage unavailable while retaining the record database.
    fn disable_storage(&self) -> DriverFuture<'_, ()>;
    /// Inject invalid persisted configuration for this ID, without changing its ID.
    fn corrupt(&self, id: InstanceId) -> DriverFuture<'_, ()>;
    /// Disable record access so queries must report an error rather than absence.
    fn close_database(&self) -> DriverFuture<'_, ()>;
}

/// Assert cursor, lookahead, deletion, corruption and failure semantics against a
/// fresh registry. This intentionally mutates only the supplied test fixture.
pub async fn assert_registry_contract(fixture: &dyn RegistryFixture) -> Result<(), Box<dyn Error>> {
    let registry = fixture.registry();
    let mut records = Vec::new();
    for _ in 0..3 {
        records.push(registry.create(fixture.config()).await?);
    }
    records.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    assert!(records.windows(2).all(|pair| pair[0].id != pair[1].id));
    fixture.disable_storage().await?;
    for record in &records {
        assert_eq!(registry.get(record.id.clone()).await?, Some(record.clone()));
    }
    let query = InstanceQuery::new(NonZeroUsize::MIN)?;
    let first = registry.list(query.clone()).await?;
    assert_eq!(first.instances, records[..1]);
    assert_eq!(first.next_after.as_ref(), Some(&records[0].id));
    assert_eq!(
        registry.remove(records[0].id.clone()).await?,
        RemoveInstanceOutcome::Removed
    );
    assert_eq!(
        registry.remove(records[0].id.clone()).await?,
        RemoveInstanceOutcome::AlreadyAbsent
    );
    assert!(registry.get(records[0].id.clone()).await?.is_none());
    let after_deleted = registry
        .list(query.clone().with_after(records[0].id.clone()))
        .await?;
    assert_eq!(after_deleted.instances, records[1..2]);
    assert_eq!(after_deleted.next_after.as_ref(), Some(&records[1].id));
    let missing_cursor = InstanceId::parse(&format!("{}!", records[0].id))?;
    let after_missing = registry
        .list(query.clone().with_after(missing_cursor))
        .await?;
    assert_eq!(after_missing.instances, after_deleted.instances);
    let final_page = registry
        .list(query.clone().with_after(records[1].id.clone()))
        .await?;
    assert_eq!(final_page.instances, records[2..]);
    assert!(final_page.next_after.is_none());
    let empty = registry
        .list(query.with_after(records[2].id.clone()))
        .await?;
    assert!(empty.instances.is_empty() && empty.next_after.is_none());
    fixture.corrupt(records[2].id.clone()).await?;
    assert!(registry.list(InstanceQuery::default()).await.is_err());
    assert!(registry.get(records[2].id.clone()).await.is_err());
    assert_eq!(
        registry.remove(records[2].id.clone()).await?,
        RemoveInstanceOutcome::Removed
    );
    fixture.close_database().await?;
    assert!(matches!(
        registry.get(records[1].id.clone()).await,
        Err(DriverError::Lifecycle(error)) if error.failure() == LifecycleFailure::Unavailable
    ));
    assert!(registry.list(InstanceQuery::default()).await.is_err());
    assert!(registry.remove(records[1].id.clone()).await.is_err());
    Ok(())
}

/// Synchronous fixture registration for providers with a ready, empty-settings
/// initializer. Async initialization tests must await DriverCatalog::register.
pub fn register_driver(
    catalog: &mut crate::application::driver_catalog::DriverCatalog,
    driver: Arc<dyn crate::application::port::Driver>,
) -> Result<(), crate::application::error::DriverCatalogError> {
    let mut future = Box::pin(catalog.register(driver, Default::default()));
    match std::future::Future::poll(
        future.as_mut(),
        &mut std::task::Context::from_waker(std::task::Waker::noop()),
    ) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => Err(
            crate::application::error::DriverCatalogError::Initialization(
                LifecycleError::new(LifecycleFailure::Unsupported).into(),
            ),
        ),
    }
}
