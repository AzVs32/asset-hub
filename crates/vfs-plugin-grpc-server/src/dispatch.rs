//! Native server dispatch with stream-owned, bounded Storage resources.
use ah_vfs::application::driver_error::{DriverError, LifecycleError, LifecycleFailure};
use ah_vfs::application::port::*;
use ah_vfs::domain::InstanceId;
use ah_vfs_storage::ContractViolation;
use ah_vfs_storage::{BackendFailure, EntryName, Failure, Operation, Rejection, StorageError};
use futures_util::FutureExt;
use std::{
    collections::HashMap,
    panic::AssertUnwindSafe,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, watch};
use vfs_plugin_codec as c;
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

type Reader = Arc<Mutex<Box<dyn StorageFileReader>>>;
type Writer = Arc<Mutex<Box<dyn StorageFileWriter>>>;
struct Lookup {
    backend: Box<dyn OperationLookup>,
    entries: HashMap<u64, ObservedEntry>,
}

struct ResolvedSlot {
    parent: ObservedEntry,
    name: EntryName,
    entry: Option<ObservedEntry>,
}

impl ResolvedSlot {
    fn as_slot(&self) -> EntrySlot<'_> {
        EntrySlot {
            parent: &self.parent,
            name: &self.name,
            entry: self.entry.as_ref(),
        }
    }
}

impl Lookup {
    fn check_purpose(&self, operation: Operation) -> Result<(), StorageError> {
        if self.backend.operation() == operation {
            Ok(())
        } else {
            Err(StorageError::without_effects(
                operation,
                Failure::InvalidBinding,
                None,
            ))
        }
    }

    fn entry(&self, id: u64) -> Result<ObservedEntry, StorageError> {
        self.entries
            .get(&id)
            .cloned()
            .ok_or_else(|| stale_lookup(self.backend.operation()))
    }

    fn slot(&self, slot: p::bound::Slot) -> Result<ResolvedSlot, StorageError> {
        let resolved = ResolvedSlot {
            parent: self.entry(slot.parent)?,
            name: slot.name,
            entry: slot.entry.map(|id| self.entry(id)).transpose()?,
        };
        resolved
            .as_slot()
            .validate(self.backend.operation(), &slot.path)?;
        Ok(resolved)
    }

    fn target(
        &self,
        target: p::bound::Target,
    ) -> Result<(Option<ResolvedSlot>, ObservedEntry), StorageError> {
        let entry = self.entry(target.entry)?;
        let slot = target.slot.map(|slot| self.slot(slot)).transpose()?;
        if slot.is_none() && !entry.entry().path().is_root() {
            return Err(StorageError::without_effects(
                self.backend.operation(),
                Failure::InvalidBinding,
                None,
            ));
        }
        Ok((slot, entry))
    }

    fn insert(&mut self, id: u64, entry: ObservedEntry) -> Result<p::ObservedEntry, StorageError> {
        let wire = c::entry(self.backend.operation(), entry.entry().clone())?;
        self.entries.insert(id, entry);
        Ok(p::ObservedEntry {
            id,
            entry: Some(wire),
        })
    }
}

fn stale_lookup(operation: Operation) -> StorageError {
    StorageError::without_effects(operation, Failure::Backend(BackendFailure::Stale), None)
}

fn lookup_capacity(operation: Operation) -> StorageError {
    StorageError::without_effects(
        operation,
        Failure::Backend(BackendFailure::CapacityExceeded),
        None,
    )
}

fn lookup_purpose(purpose: &str) -> Result<Operation, DriverError> {
    p::decode_lookup_purpose(purpose)
        .ok_or_else(|| LifecycleError::new(LifecycleFailure::InvalidRequest).into())
}

struct Slot<T> {
    value: T,
    _capacity: OwnedSemaphorePermit,
}
#[derive(Default)]
struct Resources {
    next: u64,
    storage: Option<Slot<Arc<dyn Storage>>>,
    readers: HashMap<u64, Slot<Reader>>,
    writers: HashMap<u64, Slot<Writer>>,
    lookups: HashMap<u64, Slot<Arc<Mutex<Lookup>>>>,
}
impl Resources {
    fn kind(&self, handle: u64) -> Option<p::ResourceKind> {
        if handle == 1 && self.storage.is_some() {
            Some(p::ResourceKind::Storage)
        } else if self.readers.contains_key(&handle) {
            Some(p::ResourceKind::Reader)
        } else if self.writers.contains_key(&handle) {
            Some(p::ResourceKind::Writer)
        } else if self.lookups.contains_key(&handle) {
            Some(p::ResourceKind::Lookup)
        } else {
            None
        }
    }
    fn allocate(&mut self, operation: Operation) -> Result<u64, DriverError> {
        if usize::from(self.storage.is_some())
            + self.readers.len()
            + self.writers.len()
            + self.lookups.len()
            >= p::MAX_RESOURCES
        {
            return Err(StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::CapacityExceeded),
                None,
            )
            .into());
        }
        self.next_id(operation)
    }

    fn next_id(&mut self, operation: Operation) -> Result<u64, DriverError> {
        self.next = self.next.checked_add(1).ok_or_else(|| {
            StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::CapacityExceeded),
                None,
            )
        })?;
        Ok(self.next)
    }
}
fn missing(operation: Operation) -> DriverError {
    StorageError::without_effects(operation, Failure::Backend(BackendFailure::Closed), None).into()
}
fn invalid_execution(cause: p::bound::ExecutionError) -> DriverError {
    LifecycleError::with_source(LifecycleFailure::InvalidRequest, cause).into()
}

fn id(s: &str) -> Result<InstanceId, DriverError> {
    InstanceId::parse(s).map_err(|error| {
        LifecycleError::with_source(LifecycleFailure::InvalidRequest, error).into()
    })
}
fn unsupported() -> DriverError {
    LifecycleError::new(LifecycleFailure::Unsupported).into()
}
fn get<T: Clone>(
    operation: Operation,
    map: &HashMap<u64, Slot<T>>,
    handle: u64,
) -> Result<T, DriverError> {
    map.get(&handle)
        .map(|slot| slot.value.clone())
        .ok_or_else(|| missing(operation))
}
/// Constructs a fresh backend after a panic; never return the failed driver again.
pub type DriverFactory = dyn Fn() -> DriverFuture<'static, Arc<dyn Driver>> + Send + Sync;

/// A replaceable generation. In-flight calls retain their original generation.
pub struct Dispatcher {
    current: RwLock<Arc<Generation>>,
    recovery: Mutex<()>,
    factory: Option<Arc<DriverFactory>>,
    stopped: AtomicBool,
}
impl Dispatcher {
    pub fn new(driver: Arc<dyn Driver>) -> Result<Self, DriverError> {
        Ok(Self {
            current: RwLock::new(Arc::new(Generation::new(driver)?)),
            recovery: Mutex::new(()),
            factory: None,
            stopped: AtomicBool::new(false),
        })
    }
    /// Rebuild a poisoned backend when the host next describes the plugin.
    pub async fn with_factory(factory: Arc<DriverFactory>) -> Result<Self, DriverError> {
        let driver = factory().await?;
        let mut dispatcher = Self::new(driver)?;
        dispatcher.factory = Some(factory);
        Ok(dispatcher)
    }
    fn snapshot(&self) -> Arc<Generation> {
        self.current
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    fn publish(&self, generation: Arc<Generation>) {
        let previous = {
            let mut current = self
                .current
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            current.retire();
            if self.stopped.load(Ordering::Acquire) {
                generation.retire();
            }
            std::mem::replace(&mut *current, generation)
        };
        drop(previous);
    }
    /// Install a newly constructed backend without restarting the RPC server.
    /// Its declarations are revalidated by the host before new opens are allowed.
    pub async fn replace_driver(&self, driver: Arc<dyn Driver>) -> Result<(), DriverError> {
        let _recovery = self.recovery.lock().await;
        self.running()?;
        if Arc::ptr_eq(&driver, &self.snapshot().driver) {
            return Err(LifecycleError::new(LifecycleFailure::Conflict).into());
        }
        self.publish(Arc::new(Generation::new(driver)?));
        Ok(())
    }
    pub fn generation(&self) -> String {
        self.snapshot().generation().to_owned()
    }
    /// Close every Storage stream before shutting down the RPC server.
    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        self.snapshot().retire();
    }
    fn running(&self) -> Result<(), DriverError> {
        if self.stopped.load(Ordering::Acquire) {
            Err(LifecycleError::new(LifecycleFailure::Unavailable).into())
        } else {
            Ok(())
        }
    }
    pub async fn open_session(
        &self,
        generation: &str,
        open: p::Open,
    ) -> Result<Session, DriverError> {
        self.running()?;
        self.snapshot().open(generation, open).await
    }
    pub async fn call(
        &self,
        generation: &str,
        op: Op,
        data: Vec<u8>,
    ) -> Result<p::Reply, DriverError> {
        self.running()?;
        let mut current = self.snapshot();
        if matches!(op, Op::Describe(p::Empty {}))
            && current.poisoned.load(Ordering::Acquire)
            && let Some(factory) = &self.factory
        {
            let _recovery = self.recovery.lock().await;
            current = self.snapshot();
            if current.poisoned.load(Ordering::Acquire) {
                let replacement = AssertUnwindSafe(async {
                    let driver = factory().await?;
                    if Arc::ptr_eq(&driver, &current.driver) {
                        return Err(LifecycleError::new(LifecycleFailure::Conflict).into());
                    }
                    Generation::new(driver)
                })
                .catch_unwind()
                .await
                .map_err(|_| LifecycleError::new(LifecycleFailure::Unavailable))??;
                current = Arc::new(replacement);
                self.publish(current.clone());
            }
        }
        current.call(generation, op, data).await
    }
}

struct Generation {
    driver: Arc<dyn Driver>,
    description: p::Description,
    initialized: Mutex<Option<p::Object>>,
    poisoned: AtomicBool,
    retired: watch::Sender<bool>,
    capacity: Arc<Semaphore>,
}
struct InitializationGuard<'a> {
    generation: &'a Generation,
    completed: bool,
}
impl Drop for InitializationGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.generation.retire();
        }
    }
}
impl Generation {
    fn new(driver: Arc<dyn Driver>) -> Result<Self, DriverError> {
        let generation = InstanceId::generate().as_str().to_owned();
        let description = c::describe(driver.as_ref(), generation);
        p::negotiate(&description, &[])
            .map_err(|source| LifecycleError::with_source(LifecycleFailure::Unsupported, source))?;
        c::driver_info(&description)?;
        let (retired, _) = watch::channel(false);
        Ok(Self {
            description,
            driver,
            initialized: Mutex::new(None),
            retired,
            capacity: Arc::new(Semaphore::new(p::MAX_RESOURCES)),
            poisoned: AtomicBool::new(false),
        })
    }
    fn retire(&self) {
        self.poisoned.store(true, Ordering::Release);
        self.retired.send_replace(true);
    }
    fn reserve(&self) -> Result<OwnedSemaphorePermit, DriverError> {
        self.capacity.clone().try_acquire_owned().map_err(|source| {
            LifecycleError::with_source(LifecycleFailure::CapacityExceeded, source).into()
        })
    }
    pub fn generation(&self) -> &str {
        &self.description.generation
    }
    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        self.driver.provisioner()
    }
    pub async fn call(
        &self,
        generation: &str,
        op: Op,
        data: Vec<u8>,
    ) -> Result<p::Reply, DriverError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(LifecycleError::new(LifecycleFailure::Stale).into());
        }
        let request_context = op.clone();
        // An unwinding backend must not reset the RPC stream and masquerade as
        // caller cancellation. Never expose its panic payload or replay a write.
        AssertUnwindSafe(self.call_inner(generation, op, data))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                // Retained handles must never resume against possibly corrupted
                // backend state. Retiring the generation closes its active streams.
                self.retire();
                Err(c::call_error(
                    &request_context,
                    Failure::Backend(BackendFailure::Unavailable),
                    true,
                    None,
                ))
            })
    }
    async fn call_inner(
        &self,
        generation: &str,
        op: Op,
        data: Vec<u8>,
    ) -> Result<p::Reply, DriverError> {
        if data.len() > p::MAX_DATA_BYTES {
            return Err(LifecycleError::new(LifecycleFailure::CapacityExceeded).into());
        }
        if matches!(op, Op::Describe(p::Empty {})) {
            return Ok(p::Reply::new(Value::Description(
                self.description.clone().into(),
            )));
        }
        if generation != self.generation() {
            return Err(LifecycleError::new(LifecycleFailure::Stale).into());
        }
        if !data.is_empty() {
            return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into());
        }
        if let Op::Initialize(p::Initialize {
            settings: Some(settings),
        }) = op
        {
            let mut initialized = self.initialized.lock().await;
            if let Some(previous) = &*initialized {
                return if previous == &settings {
                    Ok(p::Reply::unit())
                } else {
                    Err(LifecycleError::new(LifecycleFailure::Conflict).into())
                };
            }
            let mut guard = InitializationGuard {
                generation: self,
                completed: false,
            };
            let outcome = self
                .driver
                .initialize(c::request_object(Some(settings.clone()))?.into())
                .await;
            guard.completed = true;
            outcome?;
            *initialized = Some(settings);
            return Ok(p::Reply::unit());
        }
        if self.initialized.lock().await.is_none() {
            return Err(LifecycleError::new(LifecycleFailure::Unavailable).into());
        }
        let value = match op {
            Op::Prepare(p::Config {
                config: Some(config),
            }) => {
                let prepared = self
                    .provisioner()
                    .prepare(c::request_object(Some(config))?.into())
                    .await?;
                Value::Prepared(p::Prepared {
                    config: Some(prepared.config().clone().into_object().into()),
                    details: Some(prepared.details().clone().into()),
                })
            }
            Op::Migrate(p::Migrate {
                from_version,
                config: Some(config),
            }) => Value::Config(p::Config {
                config: Some(
                    self.provisioner()
                        .migrate_config(from_version, c::request_object(Some(config))?.into())
                        .await?
                        .into_object()
                        .into(),
                ),
            }),
            Op::TestConnection(p::Config {
                config: Some(config),
            }) => {
                self.provisioner()
                    .test_connection(&c::request_object(Some(config))?.into())
                    .await?;
                Value::Unit(p::Empty {})
            }
            _ => return Err(unsupported()),
        };
        Ok(p::Reply::new(value))
    }
    async fn open(
        self: &Arc<Self>,
        generation: &str,
        open: p::Open,
    ) -> Result<Session, DriverError> {
        if generation != self.generation() || self.poisoned.load(Ordering::Acquire) {
            return Err(LifecycleError::new(LifecycleFailure::Stale).into());
        }
        if self.initialized.lock().await.is_none() {
            return Err(LifecycleError::new(LifecycleFailure::Unavailable).into());
        }
        let p::Open { id: raw, config } = open;
        AssertUnwindSafe(async {
            let config =
                config.ok_or_else(|| LifecycleError::new(LifecycleFailure::InvalidRequest))?;
            let context = OpenContext::new(id(&raw)?, c::request_object(Some(config))?.into());
            let capacity = self.reserve()?;
            let storage = self.driver.open(context).await?;
            validate_storage_extensions(self.driver.info().capabilities(), storage.as_ref())?;
            let chunk_size = u32::try_from(storage.chunk_size().get())
                .map_err(c::protocol_violation_with_source)?;
            if chunk_size as usize > p::MAX_DATA_BYTES {
                return Err(c::protocol_violation());
            }
            let info = p::StorageInfo { chunk_size };
            Ok(Session {
                generation: self.clone(),
                info,
                resources: Mutex::new(Resources {
                    next: 1,
                    storage: Some(Slot {
                        value: storage,
                        _capacity: capacity,
                    }),
                    ..Default::default()
                }),
            })
        })
        .catch_unwind()
        .await
        .unwrap_or_else(|_| {
            self.retire();
            Err(LifecycleError::new(LifecycleFailure::Unavailable).into())
        })
    }
}

/// One Storage and its child resources, owned exclusively by one duplex stream.
pub struct Session {
    generation: Arc<Generation>,
    info: p::StorageInfo,
    resources: Mutex<Resources>,
}
impl Session {
    pub fn opened(&self) -> p::Reply {
        p::Reply::new(Value::Storage(p::Storage {
            handle: 1,
            info: Some(self.info),
        }))
    }
    pub fn retired(&self) -> watch::Receiver<bool> {
        self.generation.retired.subscribe()
    }
    async fn storage(
        &self,
        operation: Operation,
        handle: u64,
    ) -> Result<Arc<dyn Storage>, DriverError> {
        if handle != 1 {
            return Err(missing(operation));
        }
        self.resources
            .lock()
            .await
            .storage
            .as_ref()
            .map(|slot| slot.value.clone())
            .ok_or_else(|| missing(operation))
    }
    pub async fn call(&self, op: Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        if self.generation.poisoned.load(Ordering::Acquire) {
            return Err(c::call_error(
                &op,
                Failure::Backend(BackendFailure::Stale),
                false,
                None,
            ));
        }
        let request_context = op.clone();
        AssertUnwindSafe(self.call_inner(op, data))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                self.generation.retire();
                Err(c::call_error(
                    &request_context,
                    Failure::Backend(BackendFailure::Unavailable),
                    true,
                    None,
                ))
            })
    }
    async fn lookup(
        &self,
        handle: u64,
        operation: Operation,
    ) -> Result<Arc<Mutex<Lookup>>, StorageError> {
        self.resources
            .lock()
            .await
            .lookups
            .get(&handle)
            .map(|slot| slot.value.clone())
            .ok_or_else(|| stale_lookup(operation))
    }

    async fn call_inner(&self, op: Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        let value = match op {
            Op::Storage(p::StorageCall {
                handle,
                operation: Some(request),
            }) => {
                return self
                    .storage_op(
                        self.storage(request.storage_operation(), handle).await?,
                        request,
                        data,
                    )
                    .await;
            }
            Op::ReaderNext(p::Handle { handle }) => {
                let reader = get(
                    Operation::Read,
                    &self.resources.lock().await.readers,
                    handle,
                )?;
                let result = reader.lock().await.next_chunk().await;
                match result {
                    Ok(Some(data)) if !data.is_empty() && data.len() <= p::MAX_DATA_BYTES => {
                        return Ok(p::Reply {
                            result: Some(Value::Chunk(p::Chunk { eof: false })),
                            data,
                        });
                    }
                    Ok(None) => {
                        self.resources.lock().await.readers.remove(&handle);
                        Value::Chunk(p::Chunk { eof: true })
                    }
                    Ok(_) => {
                        self.resources.lock().await.readers.remove(&handle);
                        return Err(c::storage_protocol_violation(Operation::Read).into());
                    }
                    Err(error) => {
                        self.resources.lock().await.readers.remove(&handle);
                        return Err(error.into());
                    }
                }
            }
            Op::WriterChunk(p::Handle { handle }) => {
                let writer = get(
                    Operation::Write,
                    &self.resources.lock().await.writers,
                    handle,
                )?;
                let result = writer.lock().await.write_chunk(&data).await;
                if let Err(error) = result {
                    self.resources.lock().await.writers.remove(&handle);
                    return Err(error.into());
                }
                Value::Unit(p::Empty {})
            }
            Op::Commit(p::Handle { handle }) => {
                let writer = self
                    .resources
                    .lock()
                    .await
                    .writers
                    .remove(&handle)
                    .ok_or_else(|| missing(Operation::Write))?;
                let entry = writer.value.lock().await.commit().await?;
                Value::Entry(
                    c::entry(Operation::Write, entry)
                        .map_err(|error| c::response_error(Operation::Write, true, error))?,
                )
            }
            Op::Abort(p::Handle { handle }) => {
                let writer = self
                    .resources
                    .lock()
                    .await
                    .writers
                    .remove(&handle)
                    .ok_or_else(|| missing(Operation::Write))?;
                writer.value.lock().await.abort().await?;
                Value::Unit(p::Empty {})
            }
            Op::Release(p::ResourceRef { kind, handle }) => {
                let mut resources = self.resources.lock().await;
                if resources
                    .kind(handle)
                    .is_some_and(|actual| actual as i32 != kind)
                {
                    return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into());
                }
                match p::ResourceKind::try_from(kind).map_err(|source| {
                    LifecycleError::with_source(LifecycleFailure::InvalidRequest, source)
                })? {
                    p::ResourceKind::Storage => {
                        if handle == 1 {
                            resources.storage = None;
                        }
                    }
                    p::ResourceKind::Reader => {
                        resources.readers.remove(&handle);
                    }
                    p::ResourceKind::Writer => {
                        resources.writers.remove(&handle);
                    }
                    p::ResourceKind::Lookup => {
                        resources.lookups.remove(&handle);
                    }
                    _ => return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into()),
                }
                Value::Unit(p::Empty {})
            }
            Op::Execute(wire) => return self.execute(wire).await,
            Op::BeginLookup(p::LookupHandle { handle, purpose }) => {
                let operation = lookup_purpose(&purpose)?;
                if !self
                    .generation
                    .driver
                    .info()
                    .capabilities()
                    .supports(operation)
                {
                    return Err(unsupported_storage(operation).into());
                }
                let storage = self.storage(operation, handle).await?;
                let capacity = self.generation.reserve().map_err(|source| {
                    StorageError::without_effects(
                        operation,
                        Failure::Backend(BackendFailure::CapacityExceeded),
                        Some(Box::new(source)),
                    )
                })?;
                let mut backend = storage.begin_lookup(operation).await?;
                if backend.operation() != operation {
                    return Err(StorageError::without_effects(
                        operation,
                        Failure::Contract(ContractViolation::InvalidResponse),
                        None,
                    )
                    .into());
                }
                let root = backend.root().await?;
                let mut lookup = Lookup {
                    backend,
                    entries: HashMap::new(),
                };
                let mut resources = self.resources.lock().await;
                let handle = resources.allocate(operation)?;
                let id = resources.next_id(operation)?;
                let root = lookup.insert(id, root)?;
                resources.lookups.insert(
                    handle,
                    Slot {
                        value: Arc::new(Mutex::new(lookup)),
                        _capacity: capacity,
                    },
                );
                Value::OpenedLookup(p::OpenedLookup {
                    handle,
                    root: Some(root),
                })
            }
            Op::ObserveChild(p::ObserveChild {
                lookup,
                parent,
                name,
                purpose,
            }) => {
                let operation = lookup_purpose(&purpose)?;
                let lookup = self.lookup(lookup, operation).await?;
                let mut lookup = lookup.lock().await;
                lookup.check_purpose(operation)?;
                let parent = lookup.entry(parent)?;
                let name = EntryName::parse(&name).map_err(|source| {
                    LifecycleError::with_source(LifecycleFailure::InvalidRequest, source)
                })?;
                if lookup.entries.len() >= p::MAX_LOOKUP_ENTRIES {
                    return Err(lookup_capacity(operation).into());
                }
                let entry = lookup.backend.observe(&parent, &name).await?;
                let id = self.resources.lock().await.next_id(operation)?;
                Value::ObservedEntry(lookup.insert(id, entry)?)
            }
            Op::ReadObservedLink(p::ReadObservedLink {
                lookup,
                entry,
                final_slot,
                purpose,
            }) => {
                let operation = lookup_purpose(&purpose)?;
                let lookup = self.lookup(lookup, operation).await?;
                let lookup = lookup.lock().await;
                lookup.check_purpose(operation)?;
                let entry = lookup.entry(entry)?;
                let target = if let Some(slot) = final_slot {
                    let (name, path) = p::decode_entry_slot(&slot).map_err(|source| {
                        LifecycleError::with_source(LifecycleFailure::InvalidRequest, source)
                    })?;
                    let parent = lookup.entry(slot.parent)?;
                    lookup
                        .backend
                        .execute_read_link(
                            &parent,
                            &name,
                            &entry,
                            ah_vfs_storage::ReadLinkRequest::new(path),
                        )
                        .await?
                } else {
                    lookup.backend.read_link(&entry).await?
                };
                return p::encode_link_target(target).map_err(|reason| {
                    StorageError::without_effects(operation, Failure::Rejected(reason), None).into()
                });
            }
            Op::VerifyLookup(p::LookupHandle { handle, purpose }) => {
                let operation = lookup_purpose(&purpose)?;
                let lookup = self.lookup(handle, operation).await?;
                let lookup = lookup.lock().await;
                lookup.check_purpose(operation)?;
                lookup.backend.verify().await?;
                Value::Unit(p::Empty {})
            }
            _ => return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into()),
        };
        Ok(p::Reply::new(value))
    }
    async fn execute(&self, wire: p::BoundCall) -> Result<p::Reply, DriverError> {
        use p::bound::Execution;
        let (id, command) = p::decode_bound_call(wire).map_err(invalid_execution)?;
        let op = command.operation();
        let lookup = self.lookup(id, op).await?;
        let mut lookup = lookup.lock().await;
        lookup.check_purpose(op)?;
        let capabilities = self.generation.driver.info().capabilities();
        let admission = |cause| StorageError::without_effects(op, Failure::Rejected(cause), None);
        let value = match command {
            Execution::Stat(target, request) => {
                let (slot, entry) = lookup.target(target)?;
                let entry = lookup
                    .backend
                    .execute_stat(slot.as_ref().map(ResolvedSlot::as_slot), &entry, request)
                    .await?;
                Value::Entry(c::entry(op, entry)?)
            }
            Execution::Read(slot, request) => {
                capabilities.require_read(&request).map_err(admission)?;
                let slot = lookup.slot(slot)?;
                let capacity = self.generation.reserve().map_err(|_| lookup_capacity(op))?;
                let handle = self.resources.lock().await.allocate(op)?;
                let reader = lookup.backend.execute_read(slot.as_slot(), request).await?;
                self.resources.lock().await.readers.insert(
                    handle,
                    Slot {
                        value: Arc::new(Mutex::new(reader)),
                        _capacity: capacity,
                    },
                );
                Value::Reader(p::Handle { handle })
            }
            Execution::Write(slot, request) => {
                capabilities.require_write(&request).map_err(admission)?;
                let slot = lookup.slot(slot)?;
                let capacity = self.generation.reserve().map_err(|_| lookup_capacity(op))?;
                let handle = self.resources.lock().await.allocate(op)?;
                let writer = lookup
                    .backend
                    .execute_write(slot.as_slot(), request)
                    .await?;
                self.resources.lock().await.writers.insert(
                    handle,
                    Slot {
                        value: Arc::new(Mutex::new(writer)),
                        _capacity: capacity,
                    },
                );
                Value::Writer(p::Handle { handle })
            }
            Execution::CreateDirectory(slot, request) => {
                let slot = lookup.slot(slot)?;
                if lookup.entries.len() >= p::MAX_LOOKUP_ENTRIES {
                    return Err(lookup_capacity(op).into());
                }
                let id = self.resources.lock().await.next_id(op)?;
                let entry = lookup
                    .backend
                    .execute_create_directory(slot.as_slot(), request)
                    .await?;
                Value::ObservedEntry(lookup.insert(id, entry).map_err(|error| {
                    StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
                })?)
            }
            Execution::Remove(slot, request) => {
                let slot = lookup.slot(slot)?;
                lookup
                    .backend
                    .execute_remove(slot.as_slot(), request)
                    .await?;
                Value::Unit(p::Empty {})
            }
            Execution::Rename(source, destination, request) => {
                capabilities.require_rename(&request).map_err(admission)?;
                let source = lookup.slot(source)?;
                let destination = lookup.slot(destination)?;
                let entry = lookup
                    .backend
                    .execute_rename(source.as_slot(), destination.as_slot(), request)
                    .await?;
                Value::Entry(
                    c::entry(op, entry).map_err(|error| c::response_error(op, true, error))?,
                )
            }
            Execution::SelectDirectory(target, request) => {
                let (slot, entry) = lookup.target(target)?;
                let selection = lookup
                    .backend
                    .select_directory(slot.as_ref().map(ResolvedSlot::as_slot), &entry, request)
                    .await?;
                Value::SelectedDirectory(p::encode_selection(&selection))
            }
        };
        Ok(p::Reply::new(value))
    }

    async fn storage_op(
        &self,
        s: Arc<dyn Storage>,
        op: p::StorageOp,
        data: Vec<u8>,
    ) -> Result<p::Reply, DriverError> {
        let operation = op.storage_operation();
        let capabilities = self.generation.driver.info().capabilities();
        if !capabilities.supports(operation) {
            return Err(unsupported_storage(operation).into());
        }
        let value = match op {
            p::StorageOp::ResumeDirectory(checkpoint) => {
                let cursor = p::decode_checkpoint(checkpoint)
                    .map_err(|_| LifecycleError::new(LifecycleFailure::InvalidRequest))?;
                let selection = s.resume_directory(&cursor).await?;
                if &selection != cursor.selection() {
                    return Err(c::storage_protocol_violation(Operation::List).into());
                }
                Value::SelectedDirectory(p::encode_selection(&selection))
            }
            p::StorageOp::List(wire) => {
                let (request, selection) = p::decode_list(wire)
                    .map_err(|_| LifecycleError::new(LifecycleFailure::InvalidRequest))?;
                let bound = request.bind_selection(selection).map_err(|failure| {
                    StorageError::without_effects(Operation::List, Failure::Backend(failure), None)
                })?;
                let page = s.list_page(&bound).await?;
                if page.selection() != bound.selection()
                    || page.entries().len() > bound.size().get()
                {
                    return Err(c::storage_protocol_violation(Operation::List).into());
                }
                return p::encode_page(page).map_err(|cause| {
                    c::storage_protocol_violation_with_source(Operation::List, cause).into()
                });
            }
            attribute @ (p::StorageOp::GetPermissions(_)
            | p::StorageOp::SetPermissions(_)
            | p::StorageOp::ListXattrs(_)
            | p::StorageOp::GetXattr(_)
            | p::StorageOp::SetXattr(_)
            | p::StorageOp::RemoveXattr(_)) => {
                let (request, target) = p::decode_attribute(attribute, data).map_err(|cause| {
                    LifecycleError::with_source(LifecycleFailure::InvalidRequest, cause)
                })?;
                let lookup = self.lookup(target.lookup, operation).await?;
                let lookup = lookup.lock().await;
                lookup.check_purpose(operation)?;
                let entry = lookup.entry(target.entry)?;
                let slot = target
                    .slot
                    .map(|slot| {
                        let parent = lookup.entry(slot.parent)?;
                        let (name, _) = p::decode_entry_slot(&slot).map_err(|cause| {
                            LifecycleError::with_source(LifecycleFailure::InvalidRequest, cause)
                        })?;
                        Ok::<_, DriverError>((parent, name))
                    })
                    .transpose()?;
                let expected = request.action().expectation();
                let result = lookup
                    .backend
                    .execute_attribute(
                        slot.as_ref().map(|(parent, name)| (parent, name)),
                        &entry,
                        request,
                    )
                    .await?;
                return p::encode_attribute_result(expected, result).map_err(|reason| {
                    StorageError::after_dispatch(operation, Failure::Contract(reason), None).into()
                });
            }
        };
        Ok(p::Reply::new(value))
    }
}

fn unsupported_storage(operation: Operation) -> StorageError {
    StorageError::without_effects(operation, Failure::Rejected(Rejection::Unsupported), None)
}

#[cfg(test)]
mod request_validation_tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn request_ids_keep_the_value_validation_cause() -> Result<(), Box<dyn Error>> {
        let error = id("bad id").err().ok_or("invalid ID accepted")?;
        let storage = error.lifecycle_error().ok_or("storage category")?;
        assert_eq!(storage.failure(), LifecycleFailure::InvalidRequest);
        assert!(
            storage
                .source()
                .and_then(|e| e.downcast_ref::<ah_vfs::domain::InstanceIdError>())
                .is_some()
        );
        Ok(())
    }
}
