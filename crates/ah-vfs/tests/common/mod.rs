#![allow(dead_code)]
use ah_vfs::{
    application::{
        dto::{InstanceInfo, InstancePage, InstanceQuery},
        port::DriverFuture,
    },
    domain::{InstanceId, StorageError, StorageErrorKind},
};
use std::collections::BTreeMap;

pub fn create_instance() -> DriverFuture<'static, InstanceInfo> {
    Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation).into()) })
}

pub fn remove_instance() -> DriverFuture<'static, ah_vfs::application::dto::RemoveInstanceOutcome> {
    Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation).into()) })
}

pub fn get_instance(id: InstanceId, known: &[&str]) -> DriverFuture<'static, Option<InstanceInfo>> {
    let instance = known.contains(&id.as_str()).then_some(InstanceInfo {
        id,
        details: BTreeMap::new(),
    });
    Box::pin(async move { Ok(instance) })
}

pub fn list_instances(query: InstanceQuery, known: &[&str]) -> DriverFuture<'static, InstancePage> {
    let records = known
        .iter()
        .map(|id| InstanceId::parse(id))
        .collect::<Result<Vec<_>, _>>();
    Box::pin(async move {
        let mut ids = records
            .map_err(|error| StorageError::with_source(StorageErrorKind::InvalidData, error))?;
        ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        ids.retain(|id| {
            query
                .after()
                .is_none_or(|after| id.as_str() > after.as_str())
        });
        let more = ids.len() > query.limit().get();
        ids.truncate(query.limit().get());
        let next_after = more.then(|| ids.last().cloned()).flatten();
        Ok(InstancePage {
            instances: ids
                .into_iter()
                .map(|id| InstanceInfo {
                    id,
                    details: BTreeMap::new(),
                })
                .collect(),
            next_after,
        })
    })
}

use ah_vfs::application::{
    driver_info::DriverInfo,
    port::{Driver, InstanceManagement, InstanceRegistry, OpenContext, Storage},
};
use std::sync::Arc;

pub trait Backend: Send + Sync + 'static {
    fn registry(self: Arc<Self>) -> Arc<dyn InstanceRegistry> {
        static_registry(&[])
    }
    fn info(&self) -> &DriverInfo;
    fn open(&self, context: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>>;
}
struct Managed<T>(Arc<T>, Arc<dyn InstanceRegistry>);
impl<T: Backend> Driver for Managed<T> {
    fn info(&self) -> &DriverInfo {
        self.0.info()
    }
    fn instances(&self) -> InstanceManagement {
        InstanceManagement::SelfManaged(self.1.clone())
    }
    fn open(&self, context: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        self.0.open(context)
    }
}
pub trait IntoDriver {
    fn into_driver(self) -> Arc<dyn Driver>;
}
impl<T: Backend> IntoDriver for Arc<T> {
    fn into_driver(self) -> Arc<dyn Driver> {
        {
            let registry = self.clone().registry();
            Arc::new(Managed(self, registry))
        }
    }
}
impl IntoDriver for Arc<dyn Driver> {
    fn into_driver(self) -> Arc<dyn Driver> {
        self
    }
}
/// Construct a registered test driver while sharing a separately inspectable backend.
pub fn managed(value: impl IntoDriver) -> Arc<dyn Driver> {
    value.into_driver()
}

/// Register an already-assembled test provider whose default initializer is ready.
pub fn register(
    catalog: &mut ah_vfs::application::driver_catalog::DriverCatalog,
    value: impl IntoDriver,
) -> Result<(), ah_vfs::application::error::DriverCatalogError> {
    let mut future = Box::pin(catalog.register(managed(value), Default::default()));
    match std::future::Future::poll(
        future.as_mut(),
        &mut std::task::Context::from_waker(std::task::Waker::noop()),
    ) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => Err(
            ah_vfs::application::error::DriverCatalogError::Initialization(
                ah_vfs::domain::StorageError::new(
                    ah_vfs::domain::StorageErrorKind::UnsupportedOperation,
                )
                .into(),
            ),
        ),
    }
}

struct StaticRegistry {
    known: Vec<String>,
}
/// Construct one immutable registry shared by a test driver's entire lifetime.
pub fn static_registry(known: &[&str]) -> Arc<dyn InstanceRegistry> {
    Arc::new(StaticRegistry {
        known: known.iter().map(|id| (*id).to_owned()).collect(),
    })
}
impl InstanceRegistry for StaticRegistry {
    fn create(
        &self,
        _: ah_vfs::application::dto::InstanceConfig,
    ) -> DriverFuture<'_, InstanceInfo> {
        create_instance()
    }
    fn remove(
        &self,
        _: InstanceId,
    ) -> DriverFuture<'_, ah_vfs::application::dto::RemoveInstanceOutcome> {
        remove_instance()
    }
    fn get(&self, id: InstanceId) -> DriverFuture<'_, Option<InstanceInfo>> {
        get_instance(
            id,
            &self.known.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    }
    fn list(&self, query: InstanceQuery) -> DriverFuture<'_, InstancePage> {
        list_instances(
            query,
            &self.known.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    }
}
