#![allow(dead_code)]
use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::driver_error::{LifecycleError, LifecycleFailure};
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::*;
use ah_vfs::application::port::*;
use ah_vfs::domain::{DriverInstance, DriverKey, InstanceId};
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
                LifecycleFailure::Unsupported,
            )),
        ),
    }
}
fn failure(kind: LifecycleFailure) -> DriverError {
    LifecycleError::with_source(kind, std::io::Error::other("backend-private-token")).into()
}

/// Host-owned records with isolated persistence fault injection.
#[derive(Default)]
pub struct Records {
    pub rows: Mutex<HashMap<DriverInstance, InstanceRecord>>,
    pub removals: AtomicUsize,
    pub failure: Option<LifecycleFailure>,
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
                    DriverInstance::new(driver.info().key().clone(), InstanceId::parse(id)?);
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
    pub fn contains(&self, reference: &DriverInstance) -> bool {
        self.rows
            .lock()
            .is_ok_and(|rows| rows.contains_key(reference))
    }
}
impl InstanceRecordRepository for Records {
    fn get(&self, reference: DriverInstance) -> DriverFuture<'_, Option<InstanceRecord>> {
        Box::pin(async move {
            Ok(self
                .rows
                .lock()
                .map_err(|_| failure(LifecycleFailure::Backend))?
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
                .map_err(|_| failure(LifecycleFailure::Backend))?;
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
            if self.failure.is_none() || self.failure == Some(LifecycleFailure::Backend) {
                self.rows
                    .lock()
                    .map_err(|_| failure(LifecycleFailure::Backend))?
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
                .map_err(|_| failure(LifecycleFailure::Backend))?;
            if rows
                .get(record.instance())
                .is_none_or(|old| old.revision() != revision)
            {
                return Err(failure(LifecycleFailure::Conflict));
            }
            rows.insert(record.instance().clone(), record);
            Ok(())
        })
    }
    fn remove(&self, reference: DriverInstance) -> DriverFuture<'_, RemoveInstanceOutcome> {
        Box::pin(async move {
            self.removals.fetch_add(1, Ordering::SeqCst);
            if self.suspend_remove.load(Ordering::SeqCst) {
                pending::<()>().await;
            }
            if self.uncertain_remove.load(Ordering::SeqCst) {
                return Err(failure(LifecycleFailure::Backend));
            }
            if self.failure.is_none() || self.failure == Some(LifecycleFailure::Backend) {
                let removed = self
                    .rows
                    .lock()
                    .map_err(|_| failure(LifecycleFailure::Backend))?
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
            Err(failure(self.failure.unwrap_or(LifecycleFailure::Backend)))
        })
    }
}

/// Small no-follow namespace for testing service routing and byte-stream calls.
pub struct TestLookup {
    pub operation: ah_vfs_storage::Operation,
    pub scope: LookupScope,
    pub size: u64,
    pub read: Option<Box<dyn Fn() -> Box<dyn StorageFileReader> + Send + Sync>>,
    pub write: Option<Box<dyn Fn() -> Box<dyn StorageFileWriter> + Send + Sync>>,
}

use ah_vfs_storage as storage;

impl TestLookup {
    pub fn new(operation: storage::Operation, size: u64) -> Self {
        Self {
            operation,
            scope: LookupScope::default(),
            size,
            read: None,
            write: None,
        }
    }

    fn bind(
        &self,
        path: storage::StoragePath,
        kind: storage::EntryType,
    ) -> Result<ObservedEntry, storage::StorageError> {
        let entry = storage::Entry::new(path, kind, None).map_err(|cause| {
            storage::StorageError::without_effects(
                self.operation,
                storage::Failure::Contract(storage::ContractViolation::InvalidEntry),
                Some(Box::new(cause)),
            )
        })?;
        Ok(self.scope.bind(entry, ()))
    }

    fn reject(&self, reason: storage::Rejection) -> storage::StorageError {
        storage::StorageError::without_effects(
            self.operation,
            storage::Failure::Rejected(reason),
            None,
        )
    }

    pub fn selection() -> storage::DirectorySelection {
        storage::DirectorySelection::from_lookup(
            storage::StoragePath::root(),
            storage::DirectoryIdentity::from_bytes([1; 16]),
        )
    }
}

impl OperationLookup for TestLookup {
    fn operation(&self) -> storage::Operation {
        self.operation
    }

    fn root(&mut self) -> StorageFuture<'_, ObservedEntry> {
        Box::pin(async { self.bind(storage::StoragePath::root(), storage::EntryType::Directory) })
    }

    fn observe<'a>(
        &'a mut self,
        parent: &'a ObservedEntry,
        name: &'a storage::EntryName,
    ) -> StorageFuture<'a, ObservedEntry> {
        Box::pin(async move {
            if self.operation == storage::Operation::Write {
                return Err(self.reject(storage::Rejection::NotFound));
            }
            let path = parent
                .entry()
                .path()
                .join(name)
                .map_err(|_| self.reject(storage::Rejection::PathTooLong))?;
            self.bind(path, storage::EntryType::file(Some(self.size)))
        })
    }

    fn read_link<'a>(&'a self, _: &'a ObservedEntry) -> StorageFuture<'a, storage::LinkTarget> {
        Box::pin(async { Err(self.reject(storage::Rejection::NotSymlink)) })
    }

    fn verify(&self) -> StorageFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn execute_read<'a>(
        &'a self,
        _: EntrySlot<'a>,
        _: storage::ReadRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            self.read
                .as_ref()
                .map(|open| open())
                .ok_or_else(|| self.reject(storage::Rejection::Unsupported))
        })
    }

    fn execute_write<'a>(
        &'a self,
        _: EntrySlot<'a>,
        _: storage::WriteRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            self.write
                .as_ref()
                .map(|open| open())
                .ok_or_else(|| self.reject(storage::Rejection::Unsupported))
        })
    }

    fn select_directory<'a>(
        &'a self,
        _: Option<EntrySlot<'a>>,
        _: &'a ObservedEntry,
        _: storage::ListRequest,
    ) -> StorageFuture<'a, storage::DirectorySelection> {
        Box::pin(async { Ok(Self::selection()) })
    }
}
