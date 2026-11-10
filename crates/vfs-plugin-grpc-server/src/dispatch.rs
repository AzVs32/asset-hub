//! Native server dispatch with stream-owned, bounded Storage resources.
use ah_vfs::application::driver_error::{DriverError, LifecycleError, LifecycleFailure};
use ah_vfs::application::port::*;
use ah_vfs::domain::InstanceId;
use ah_vfs_storage::ContractViolation;
use ah_vfs_storage::{BackendFailure, Failure, Operation, Rejection, StorageError};
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
}
impl Resources {
    fn kind(&self, handle: u64) -> Option<p::ResourceKind> {
        if handle == 1 && self.storage.is_some() {
            Some(p::ResourceKind::Storage)
        } else if self.readers.contains_key(&handle) {
            Some(p::ResourceKind::Reader)
        } else if self.writers.contains_key(&handle) {
            Some(p::ResourceKind::Writer)
        } else {
            None
        }
    }
    fn allocate(&mut self, operation: Operation) -> Result<u64, DriverError> {
        if usize::from(self.storage.is_some()) + self.readers.len() + self.writers.len()
            >= p::MAX_RESOURCES
        {
            return Err(StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::CapacityExceeded),
                None,
            )
            .into());
        }
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
fn invalid_lookup(cause: p::LookupValueError) -> DriverError {
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
                )
                .into())
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
                )
                .into())
            })
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
            Op::Read(read) => {
                let handle = read.handle;
                let (request, working_directory, intermediate_symlinks) =
                    p::decode_read(read).map_err(invalid_lookup)?;
                self.generation
                    .driver
                    .info()
                    .capabilities()
                    .require_read(&request)
                    .map_err(|reason| {
                        StorageError::without_effects(
                            Operation::Read,
                            Failure::Rejected(reason),
                            None,
                        )
                    })?;
                let storage = self.storage(Operation::Read, handle).await?;
                let capacity = self.generation.reserve().map_err(|source| {
                    StorageError::without_effects(
                        Operation::Read,
                        Failure::Backend(BackendFailure::CapacityExceeded),
                        Some(Box::new(source)),
                    )
                })?;
                let reader = storage
                    .read_file(
                        request,
                        LookupContext {
                            working_directory,
                            intermediate_symlinks,
                        },
                    )
                    .await?;
                let mut resources = self.resources.lock().await;
                let handle = resources.allocate(Operation::Read)?;
                resources.readers.insert(
                    handle,
                    Slot {
                        value: Arc::new(Mutex::new(reader)),
                        _capacity: capacity,
                    },
                );
                Value::Reader(p::Handle { handle })
            }
            Op::Write(write) => {
                let handle = write.handle;
                let (request, working_directory, intermediate_symlinks) =
                    p::decode_write(write).map_err(invalid_lookup)?;
                self.generation
                    .driver
                    .info()
                    .capabilities()
                    .require_write(&request)
                    .map_err(|reason| {
                        StorageError::without_effects(
                            Operation::Write,
                            Failure::Rejected(reason),
                            None,
                        )
                    })?;
                let storage = self.storage(Operation::Write, handle).await?;
                let capacity = self.generation.reserve().map_err(|source| {
                    StorageError::without_effects(
                        Operation::Write,
                        Failure::Backend(BackendFailure::CapacityExceeded),
                        Some(Box::new(source)),
                    )
                })?;
                let writer = storage
                    .write_file(
                        request,
                        LookupContext {
                            working_directory,
                            intermediate_symlinks,
                        },
                    )
                    .await?;
                let mut resources = self.resources.lock().await;
                let handle = resources.allocate(Operation::Write)?;
                resources.writers.insert(
                    handle,
                    Slot {
                        value: Arc::new(Mutex::new(writer)),
                        _capacity: capacity,
                    },
                );
                Value::Writer(p::Handle { handle })
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
                    _ => return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into()),
                }
                Value::Unit(p::Empty {})
            }
            _ => return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into()),
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
            p::StorageOp::Stat(stat) => {
                let (request, working_directory, intermediate_symlinks) =
                    p::decode_stat(stat).map_err(invalid_lookup)?;
                let context = LookupContext {
                    working_directory,
                    intermediate_symlinks,
                };
                Value::Entry(c::entry(operation, s.stat_entry(request, context).await?)?)
            }
            p::StorageOp::ReadLink(request) => {
                let (request, working_directory, intermediate_symlinks) =
                    p::decode_read_link(request).map_err(invalid_lookup)?;
                let context = LookupContext {
                    working_directory,
                    intermediate_symlinks,
                };
                let target = s.read_link(request, context).await?;
                return p::encode_link_target(target).map_err(|reason| {
                    StorageError::without_effects(operation, Failure::Rejected(reason), None).into()
                });
            }
            p::StorageOp::SelectDirectory(wire) => {
                let (request, working_directory, intermediate_symlinks) =
                    p::decode_select_directory(wire)
                        .map_err(|_| LifecycleError::new(LifecycleFailure::InvalidRequest))?;
                let selection = s
                    .select_directory(
                        &request,
                        LookupContext {
                            working_directory,
                            intermediate_symlinks,
                        },
                    )
                    .await?;
                request
                    .bind_selection(selection.clone())
                    .map_err(|failure| {
                        StorageError::without_effects(
                            Operation::List,
                            Failure::Backend(failure),
                            None,
                        )
                    })?;
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
            p::StorageOp::CreateDirectory(wire) => {
                let (request, working_directory, intermediate_symlinks) =
                    p::decode_create_directory(wire).map_err(invalid_lookup)?;
                let entry = s
                    .create_dir(
                        request,
                        LookupContext {
                            working_directory,
                            intermediate_symlinks,
                        },
                    )
                    .await?;
                Value::Entry(
                    c::entry(operation, entry)
                        .map_err(|error| c::response_error(operation, true, error))?,
                )
            }
            p::StorageOp::Remove(wire) => {
                let (request, working_directory, intermediate_symlinks) =
                    p::decode_remove(wire).map_err(invalid_lookup)?;
                s.remove_entry(
                    request,
                    LookupContext {
                        working_directory,
                        intermediate_symlinks,
                    },
                )
                .await?;
                Value::Unit(p::Empty {})
            }
            p::StorageOp::Rename(wire) => {
                let (request, working_directory, intermediate_symlinks) =
                    p::decode_rename(wire).map_err(invalid_lookup)?;
                if capabilities.require_rename(&request).is_err() {
                    return Err(unsupported_storage(operation).into());
                }
                let entry = s
                    .move_entry(
                        request,
                        LookupContext {
                            working_directory,
                            intermediate_symlinks,
                        },
                    )
                    .await?;
                Value::Entry(
                    c::entry(operation, entry)
                        .map_err(|error| c::response_error(operation, true, error))?,
                )
            }
            attribute @ (p::StorageOp::GetPermissions(_)
            | p::StorageOp::SetPermissions(_)
            | p::StorageOp::ListXattrs(_)
            | p::StorageOp::GetXattr(_)
            | p::StorageOp::SetXattr(_)
            | p::StorageOp::RemoveXattr(_)) => {
                let (request, working_directory, intermediate_symlinks) =
                    p::decode_attribute(attribute, data).map_err(|cause| {
                        LifecycleError::with_source(LifecycleFailure::InvalidRequest, cause)
                    })?;
                let attributes = s.attributes().ok_or_else(|| {
                    StorageError::without_effects(
                        operation,
                        Failure::Contract(ContractViolation::InvalidAttributes),
                        None,
                    )
                })?;
                let expected = request.action().expectation();
                let result = attributes
                    .execute(
                        request,
                        LookupContext {
                            working_directory,
                            intermediate_symlinks,
                        },
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

fn unsupported_storage(operation: Operation) -> StorageError {
    StorageError::without_effects(operation, Failure::Rejected(Rejection::Unsupported), None)
}
