//! Native server dispatch with stream-owned, bounded Storage resources.
use ah_vfs::{
    application::{dto::*, port::*},
    domain::*,
};
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
    fn allocate(&mut self) -> Result<u64, DriverError> {
        if usize::from(self.storage.is_some()) + self.readers.len() + self.writers.len()
            >= p::MAX_RESOURCES
        {
            return Err(StorageError::new(StorageErrorKind::CapacityExceeded).into());
        }
        self.next = self.next.checked_add(1).ok_or_else(c::invalid)?;
        Ok(self.next)
    }
}
fn missing() -> DriverError {
    StorageError::new(StorageErrorKind::SessionClosed).into()
}
fn path(s: &str) -> Result<StoragePath, DriverError> {
    StoragePath::parse(s).map_err(|e| StorageError::from(e).into())
}
fn id(s: &str) -> Result<InstanceId, DriverError> {
    InstanceId::parse(s).map_err(|_| StorageError::new(StorageErrorKind::InvalidRequest).into())
}
fn unsupported() -> DriverError {
    StorageError::new(StorageErrorKind::UnsupportedOperation).into()
}
fn get<T: Clone>(map: &HashMap<u64, Slot<T>>, handle: u64) -> Result<T, DriverError> {
    map.get(&handle)
        .map(|slot| slot.value.clone())
        .ok_or_else(missing)
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
            return Err(StorageError::new(StorageErrorKind::Conflict).into());
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
            Err(StorageError::new(StorageErrorKind::Unavailable).into())
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
                        return Err(StorageError::new(StorageErrorKind::Conflict).into());
                    }
                    Generation::new(driver)
                })
                .catch_unwind()
                .await
                .map_err(|_| StorageError::new(StorageErrorKind::Unavailable))??;
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
        let generation = new_instance_id()?.as_str().to_owned();
        let description = c::describe(driver.as_ref(), generation);
        p::negotiate(&description, &[]).map_err(|_| unsupported())?;
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
        self.capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| StorageError::new(StorageErrorKind::CapacityExceeded).into())
    }
    pub fn generation(&self) -> &str {
        &self.description.generation
    }
    fn provisioner(&self) -> Result<Arc<dyn InstanceProvisioner>, DriverError> {
        match self.driver.instances() {
            InstanceManagement::HostManaged(p) => Ok(p),
            _ => Err(unsupported()),
        }
    }
    fn registry(&self) -> Result<Arc<dyn InstanceRegistry>, DriverError> {
        self.driver.instances().registry()
    }
    pub async fn call(
        &self,
        generation: &str,
        op: Op,
        data: Vec<u8>,
    ) -> Result<p::Reply, DriverError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
        }
        let mutates = op.mutates();
        // An unwinding backend must not reset the RPC stream and masquerade as
        // caller cancellation. Never expose its panic payload or replay a write.
        AssertUnwindSafe(self.call_inner(generation, op, data))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                // Retained handles must never resume against possibly corrupted
                // backend state. Retiring the generation closes its active streams.
                self.retire();
                Err(StorageError::new(if mutates {
                    StorageErrorKind::OutcomeUnknown
                } else {
                    StorageErrorKind::Unavailable
                })
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
            return Err(StorageError::new(StorageErrorKind::TooLarge).into());
        }
        if matches!(op, Op::Describe(p::Empty {})) {
            return Ok(p::Reply::new(Value::Description(
                self.description.clone().into(),
            )));
        }
        if generation != self.generation() {
            return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
        }
        if !data.is_empty() {
            return Err(StorageError::new(StorageErrorKind::InvalidRequest).into());
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
                    Err(StorageError::new(StorageErrorKind::Conflict).into())
                };
            }
            let mut guard = InitializationGuard {
                generation: self,
                completed: false,
            };
            let outcome = self
                .driver
                .initialize(c::object(Some(settings.clone()))?.into())
                .await;
            guard.completed = true;
            outcome?;
            *initialized = Some(settings);
            return Ok(p::Reply::unit());
        }
        if self.initialized.lock().await.is_none() {
            return Err(StorageError::new(StorageErrorKind::Unavailable).into());
        }
        let value = match op {
            Op::Prepare(p::Config {
                config: Some(config),
            }) => {
                let prepared = self
                    .provisioner()?
                    .prepare(c::object(Some(config))?.into())
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
                    self.provisioner()?
                        .migrate_config(from_version, c::object(Some(config))?.into())
                        .await?
                        .into_object()
                        .into(),
                ),
            }),
            Op::TestConnection(p::Config {
                config: Some(config),
            }) => {
                self.provisioner()?
                    .test_connection(&c::object(Some(config))?.into())
                    .await?;
                Value::Unit(p::Empty {})
            }
            Op::GetInstance(p::InstanceId { id: raw }) => Value::MaybeInstance(p::MaybeInstance {
                instance: self.registry()?.get(id(&raw)?).await?.map(c::instance),
            }),
            Op::ListInstances(p::ListInstances { after, limit }) => {
                let mut q = InstanceQuery::new(c::limit(limit)?)?;
                if let Some(after) = after {
                    q = q.with_after(id(&after)?);
                }
                let page = self.registry()?.list(q).await?;
                Value::Instances(p::Instances {
                    instances: page.instances.into_iter().map(c::instance).collect(),
                    next_after: page.next_after.map(|id| id.as_str().into()),
                })
            }
            Op::CreateInstance(p::Config {
                config: Some(config),
            }) => Value::Instance(c::instance(
                self.registry()?
                    .create(c::object(Some(config))?.into())
                    .await?,
            )),
            Op::RemoveInstance(p::InstanceId { id: raw }) => Value::Removed(p::Removed {
                removed: self.registry()?.remove(id(&raw)?).await?
                    == RemoveInstanceOutcome::Removed,
            }),
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
            return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
        }
        if self.initialized.lock().await.is_none() {
            return Err(StorageError::new(StorageErrorKind::Unavailable).into());
        }
        let p::Open { id: raw, config } = open;
        AssertUnwindSafe(async {
            let host = self.description.ownership == p::Ownership::Host as i32;
            if host != config.is_some() {
                return Err(StorageError::new(StorageErrorKind::InvalidRequest).into());
            }
            let context = match config {
                Some(config) => {
                    OpenContext::host_managed(id(&raw)?, c::object(Some(config))?.into())
                }
                None => OpenContext::self_managed(id(&raw)?),
            };
            let capacity = self.reserve()?;
            let storage = self.driver.open(context).await?;
            validate_storage_capabilities(self.driver.info().capabilities(), storage.as_ref())?;
            let chunk_size = u32::try_from(storage.chunk_size().get()).map_err(|_| c::invalid())?;
            if chunk_size as usize > p::MAX_DATA_BYTES {
                return Err(c::invalid());
            }
            let info = p::StorageInfo {
                capabilities: c::capabilities(storage.capabilities()),
                chunk_size,
            };
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
            Err(StorageError::new(StorageErrorKind::Unavailable).into())
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
            info: Some(self.info.clone()),
        }))
    }
    pub fn retired(&self) -> watch::Receiver<bool> {
        self.generation.retired.subscribe()
    }
    async fn storage(&self, handle: u64) -> Result<Arc<dyn Storage>, DriverError> {
        if handle != 1 {
            return Err(missing());
        }
        self.resources
            .lock()
            .await
            .storage
            .as_ref()
            .map(|slot| slot.value.clone())
            .ok_or_else(missing)
    }
    pub async fn call(&self, op: Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        if self.generation.poisoned.load(Ordering::Acquire) {
            return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
        }
        let mutates = op.mutates();
        AssertUnwindSafe(self.call_inner(op, data))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                self.generation.retire();
                Err(StorageError::new(if mutates {
                    StorageErrorKind::OutcomeUnknown
                } else {
                    StorageErrorKind::Unavailable
                })
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
                    .storage_op(self.storage(handle).await?, request, data)
                    .await;
            }
            Op::Read(p::Read {
                handle,
                path: raw,
                offset,
                length,
            }) => {
                let capacity = self.generation.reserve()?;
                let reader = self
                    .storage(handle)
                    .await?
                    .read_file(StorageReadFileRequest {
                        path: path(&raw)?,
                        offset,
                        length,
                    })
                    .await?;
                let mut resources = self.resources.lock().await;
                let handle = resources.allocate()?;
                resources.readers.insert(
                    handle,
                    Slot {
                        value: Arc::new(Mutex::new(reader)),
                        _capacity: capacity,
                    },
                );
                Value::Reader(p::Handle { handle })
            }
            Op::Write(p::Write {
                handle,
                path: raw,
                replace,
                atomic,
            }) => {
                let capacity = self.generation.reserve()?;
                let writer = self
                    .storage(handle)
                    .await?
                    .write_file(StorageWriteFileRequest {
                        path: path(&raw)?,
                        mode: if replace {
                            StorageWriteFileMode::Replace
                        } else {
                            StorageWriteFileMode::CreateNew
                        },
                        require_atomic_publish: atomic,
                    })
                    .await?;
                let mut resources = self.resources.lock().await;
                let handle = resources.allocate()?;
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
                let reader = get(&self.resources.lock().await.readers, handle)?;
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
                        return Err(c::invalid());
                    }
                    Err(error) => {
                        self.resources.lock().await.readers.remove(&handle);
                        return Err(error.into());
                    }
                }
            }
            Op::WriterChunk(p::Handle { handle }) => {
                let writer = get(&self.resources.lock().await.writers, handle)?;
                writer.lock().await.write_chunk(&data).await?;
                Value::Unit(p::Empty {})
            }
            Op::Commit(p::Handle { handle }) => {
                let writer = self
                    .resources
                    .lock()
                    .await
                    .writers
                    .remove(&handle)
                    .ok_or_else(missing)?;
                let entry = writer.value.lock().await.commit().await?;
                Value::Entry(c::entry(entry)?)
            }
            Op::Abort(p::Handle { handle }) => {
                let writer = self
                    .resources
                    .lock()
                    .await
                    .writers
                    .remove(&handle)
                    .ok_or_else(missing)?;
                writer.value.lock().await.abort().await?;
                Value::Unit(p::Empty {})
            }
            Op::Release(p::ResourceRef { kind, handle }) => {
                let mut resources = self.resources.lock().await;
                if resources
                    .kind(handle)
                    .is_some_and(|actual| actual as i32 != kind)
                {
                    return Err(c::invalid());
                }
                match p::ResourceKind::try_from(kind).map_err(|_| c::invalid())? {
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
                    _ => return Err(c::invalid()),
                }
                Value::Unit(p::Empty {})
            }
            _ => return Err(StorageError::new(StorageErrorKind::InvalidRequest).into()),
        };
        Ok(p::Reply::new(value))
    }
    async fn storage_op(
        &self,
        s: Arc<dyn Storage>,
        op: p::StorageOp,
        data: Vec<u8>,
    ) -> Result<p::Reply, DriverError> {
        let value = match op {
            p::StorageOp::Stat(p::Path { path: raw }) => Value::Entry(c::entry(
                s.stat_entry(StorageStatEntryRequest { path: path(&raw)? })
                    .await?,
            )?),
            p::StorageOp::List(p::List {
                path: raw,
                after,
                limit,
            }) => {
                let q = StorageDirQuery {
                    path: path(&raw)?,
                    after,
                    limit: c::limit(limit)?,
                };
                q.validate()?;
                let page = s.list_entries(q).await?;
                Value::Page(p::Page {
                    entries: page
                        .entries
                        .into_iter()
                        .map(c::entry)
                        .collect::<Result<_, _>>()?,
                    next_after: page.next_after,
                })
            }
            p::StorageOp::CreateDir(p::CreateDir {
                path: raw,
                recursive,
            }) => Value::Entry(c::entry(
                s.create_dir(StorageCreateDirRequest {
                    path: path(&raw)?,
                    recursive,
                })
                .await?,
            )?),
            p::StorageOp::Remove(p::Path { path: raw }) => {
                s.remove_entry(StorageRemoveEntryRequest { path: path(&raw)? })
                    .await?;
                Value::Unit(p::Empty {})
            }
            p::StorageOp::Move(p::Move {
                source,
                destination,
                replace,
            }) => Value::Entry(c::entry(
                s.move_entry(StorageMoveEntryRequest {
                    source: path(&source)?,
                    destination: path(&destination)?,
                    replace,
                })
                .await?,
            )?),
            p::StorageOp::GetPermissions(p::Path { path: raw }) => {
                Value::Permissions(p::Permissions {
                    mode: s
                        .permissions()
                        .ok_or_else(unsupported)?
                        .get_permissions(path(&raw)?)
                        .await?
                        .bits(),
                })
            }
            p::StorageOp::SetPermissions(p::SetPermissions { path: raw, mode }) => {
                s.permissions()
                    .ok_or_else(unsupported)?
                    .set_permissions(path(&raw)?, EntryPermissions::new(mode)?)
                    .await?;
                Value::Unit(p::Empty {})
            }
            p::StorageOp::ListXattrs(p::ListXattrs {
                path: raw,
                max_bytes,
            }) => Value::Names(p::Names {
                names: s
                    .xattrs()
                    .ok_or_else(unsupported)?
                    .list_xattrs(path(&raw)?, c::limit(max_bytes)?)
                    .await?
                    .into_iter()
                    .map(|n| n.as_str().into())
                    .collect(),
            }),
            p::StorageOp::GetXattr(p::GetXattr {
                path: raw,
                name,
                max_bytes,
            }) => {
                return Ok(p::Reply {
                    result: Some(Value::Bytes(p::Empty {})),
                    data: s
                        .xattrs()
                        .ok_or_else(unsupported)?
                        .get_xattr(path(&raw)?, XattrName::parse(&name)?, c::limit(max_bytes)?)
                        .await?,
                });
            }
            p::StorageOp::SetXattr(p::SetXattr {
                path: raw,
                name,
                mode,
            }) => {
                let mode = match p::XattrWriteMode::try_from(mode) {
                    Ok(p::XattrWriteMode::Upsert) => XattrWriteMode::Upsert,
                    Ok(p::XattrWriteMode::CreateNew) => XattrWriteMode::CreateNew,
                    Ok(p::XattrWriteMode::Replace) => XattrWriteMode::Replace,
                    _ => return Err(StorageError::new(StorageErrorKind::InvalidRequest).into()),
                };
                s.xattrs()
                    .ok_or_else(unsupported)?
                    .set_xattr(path(&raw)?, XattrName::parse(&name)?, data, mode)
                    .await?;
                Value::Unit(p::Empty {})
            }
            p::StorageOp::RemoveXattr(p::RemoveXattr { path: raw, name }) => {
                s.xattrs()
                    .ok_or_else(unsupported)?
                    .remove_xattr(path(&raw)?, XattrName::parse(&name)?)
                    .await?;
                Value::Unit(p::Empty {})
            }
        };
        Ok(p::Reply::new(value))
    }
}
