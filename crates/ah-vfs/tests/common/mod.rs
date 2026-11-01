#![allow(dead_code)]
use ah_vfs::{
    application::{driver_info::DriverInfo, dto::*, port::*},
    domain::*,
};
use std::{
    collections::{BTreeMap, HashMap},
    future::pending,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

pub trait Backend: Send + Sync + 'static {
    fn provisioner(self: Arc<Self>) -> Arc<dyn InstanceProvisioner> {
        Arc::new(Provisioner)
    }
    fn info(&self) -> &DriverInfo;
    fn open(&self, context: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>>;
}
struct Managed<T>(Arc<T>);
impl<T: Backend> Driver for Managed<T> {
    fn info(&self) -> &DriverInfo {
        self.0.info()
    }
    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        self.0.clone().provisioner()
    }
    fn open(&self, context: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        self.0.open(context)
    }
}
pub struct Provisioner;
impl InstanceProvisioner for Provisioner {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
        Box::pin(async move { Ok(PreparedConfig::new(config, BTreeMap::new())) })
    }
}
pub trait IntoDriver {
    fn into_driver(self) -> Arc<dyn Driver>;
}
impl<T: Backend> IntoDriver for Arc<T> {
    fn into_driver(self) -> Arc<dyn Driver> {
        Arc::new(Managed(self))
    }
}
impl IntoDriver for Arc<dyn Driver> {
    fn into_driver(self) -> Arc<dyn Driver> {
        self
    }
}
pub fn managed(value: impl IntoDriver) -> Arc<dyn Driver> {
    value.into_driver()
}
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
            ah_vfs::application::error::DriverCatalogError::Initialization(failure(
                StorageErrorKind::UnsupportedOperation,
            )),
        ),
    }
}
fn failure(kind: StorageErrorKind) -> DriverError {
    StorageError::with_source(kind, std::io::Error::other("backend-private-token")).into()
}

/// Host-owned records with isolated persistence fault injection.
#[derive(Default)]
pub struct Records {
    pub rows: Mutex<HashMap<DriverInstanceRef, InstanceRecord>>,
    pub removals: AtomicUsize,
    pub failure: Option<StorageErrorKind>,
    pub suspend_remove: AtomicBool,
    pub uncertain_remove: AtomicBool,
}
impl Records {
    pub fn seeded(
        catalog: &ah_vfs::application::driver_catalog::DriverCatalog,
        known: &[&str],
    ) -> Result<Arc<Self>, Box<dyn std::error::Error>> {
        let records = Self::default();
        for driver in catalog.list() {
            for id in known {
                let reference =
                    DriverInstanceRef::new(driver.info().key().clone(), InstanceId::parse(id)?);
                let record = InstanceRecord::new(
                    reference.clone(),
                    1,
                    1,
                    r#"{"parameters":{},"secret_paths":[],"details":{}}"#.into(),
                    0,
                    0,
                )?;
                records
                    .rows
                    .lock()
                    .map_err(|_| "lock")?
                    .insert(reference, record);
            }
        }
        Ok(Arc::new(records))
    }
    pub fn contains(&self, reference: &DriverInstanceRef) -> bool {
        self.rows
            .lock()
            .is_ok_and(|rows| rows.contains_key(reference))
    }
}
impl InstanceRecordRepository for Records {
    fn get(&self, reference: DriverInstanceRef) -> DriverFuture<'_, Option<InstanceRecord>> {
        Box::pin(async move {
            Ok(self
                .rows
                .lock()
                .map_err(|_| failure(StorageErrorKind::Backend))?
                .get(&reference)
                .cloned())
        })
    }
    fn list(
        &self,
        driver: DriverKey,
        query: InstanceQuery,
    ) -> DriverFuture<'_, InstanceRecordPage> {
        Box::pin(async move {
            let rows = self
                .rows
                .lock()
                .map_err(|_| failure(StorageErrorKind::Backend))?;
            let mut records: Vec<_> = rows
                .values()
                .filter(|r| {
                    r.instance().driver_key() == &driver
                        && query.after().is_none_or(|after| {
                            r.instance().instance_id().as_str() > after.as_str()
                        })
                })
                .cloned()
                .collect();
            records.sort_by(|a, b| {
                a.instance()
                    .instance_id()
                    .as_str()
                    .cmp(b.instance().instance_id().as_str())
            });
            let more = records.len() > query.limit().get();
            records.truncate(query.limit().get());
            let next_after = more
                .then(|| records.last().map(|r| r.instance().instance_id().clone()))
                .flatten();
            Ok(InstanceRecordPage {
                records,
                next_after,
            })
        })
    }
    fn insert(&self, record: InstanceRecord) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            if self.failure.is_none() || self.failure == Some(StorageErrorKind::OutcomeUnknown) {
                self.rows
                    .lock()
                    .map_err(|_| failure(StorageErrorKind::Backend))?
                    .insert(record.instance().clone(), record);
            }
            if let Some(kind) = self.failure {
                return Err(failure(kind));
            }
            Ok(())
        })
    }
    fn update(&self, record: InstanceRecord, revision: u64) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            let mut rows = self
                .rows
                .lock()
                .map_err(|_| failure(StorageErrorKind::Backend))?;
            if rows
                .get(record.instance())
                .is_none_or(|old| old.revision() != revision)
            {
                return Err(failure(StorageErrorKind::Conflict));
            }
            rows.insert(record.instance().clone(), record);
            Ok(())
        })
    }
    fn remove(&self, reference: DriverInstanceRef) -> DriverFuture<'_, RemoveInstanceOutcome> {
        Box::pin(async move {
            self.removals.fetch_add(1, Ordering::SeqCst);
            if self.suspend_remove.load(Ordering::SeqCst) {
                pending::<()>().await;
            }
            if self.uncertain_remove.load(Ordering::SeqCst) {
                return Err(failure(StorageErrorKind::OutcomeUnknown));
            }
            if self.failure.is_none() || self.failure == Some(StorageErrorKind::OutcomeUnknown) {
                let removed = self
                    .rows
                    .lock()
                    .map_err(|_| failure(StorageErrorKind::Backend))?
                    .remove(&reference)
                    .is_some();
                if self.failure.is_none() {
                    return Ok(if removed {
                        RemoveInstanceOutcome::Removed
                    } else {
                        RemoveInstanceOutcome::AlreadyAbsent
                    });
                }
            }
            Err(failure(self.failure.unwrap_or(StorageErrorKind::Backend)))
        })
    }
}
