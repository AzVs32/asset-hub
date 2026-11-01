//! Native server dispatch with generation-scoped, bounded, expiring resources.
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
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use vfs_plugin_codec as c;
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

type Reader = Arc<Mutex<Box<dyn StorageFileReader>>>;
type Writer = Arc<Mutex<Box<dyn StorageFileWriter>>>;
struct Slot<T> {
    value: T,
    touched: Instant,
}
#[derive(Default)]
struct Resources {
    next: u64,
    storage: HashMap<u64, Slot<Arc<dyn Storage>>>,
    readers: HashMap<u64, Slot<Reader>>,
    writers: HashMap<u64, Slot<Writer>>,
}
impl Resources {
    fn kind(&self, handle: u64) -> Option<p::ResourceKind> {
        if self.storage.contains_key(&handle) {
            Some(p::ResourceKind::Storage)
        } else if self.readers.contains_key(&handle) {
            Some(p::ResourceKind::Reader)
        } else if self.writers.contains_key(&handle) {
            Some(p::ResourceKind::Writer)
        } else {
            None
        }
    }
    fn expire(&mut self, ttl: Duration) {
        self.storage.retain(|_, s| s.touched.elapsed() < ttl);
        self.readers.retain(|_, s| s.touched.elapsed() < ttl);
        self.writers.retain(|_, s| s.touched.elapsed() < ttl);
    }
    fn allocate(&mut self) -> Result<u64, DriverError> {
        if self.storage.len() + self.readers.len() + self.writers.len() >= p::MAX_RESOURCES {
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
fn slot<T>(value: T) -> Slot<T> {
    Slot {
        value,
        touched: Instant::now(),
    }
}
fn get<T: Clone>(map: &mut HashMap<u64, Slot<T>>, handle: u64) -> Result<T, DriverError> {
    let s = map.get_mut(&handle).ok_or_else(missing)?;
    s.touched = Instant::now();
    Ok(s.value.clone())
}

/// Constructs a fresh backend after a panic; never return the failed driver again.
pub type DriverFactory = dyn Fn() -> DriverFuture<'static, Arc<dyn Driver>> + Send + Sync;

/// A replaceable generation. In-flight calls retain their original generation.
pub struct Dispatcher {
    current: RwLock<Arc<Generation>>,
    recovery: Mutex<()>,
    factory: Option<Arc<DriverFactory>>,
    ttl: Duration,
}
impl Dispatcher {
    pub fn new(driver: Arc<dyn Driver>) -> Result<Self, DriverError> {
        Self::with_lease(driver, Duration::from_secs(300))
    }
    pub fn with_lease(driver: Arc<dyn Driver>, ttl: Duration) -> Result<Self, DriverError> {
        Ok(Self {
            current: RwLock::new(Arc::new(Generation::with_lease(driver, ttl)?)),
            recovery: Mutex::new(()),
            factory: None,
            ttl,
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
            current.poisoned.store(true, Ordering::Release);
            std::mem::replace(&mut *current, generation)
        };
        drop(previous);
    }
    /// Install a newly constructed backend without restarting the RPC server.
    /// Its declarations are revalidated by the host before new opens are allowed.
    pub async fn replace_driver(&self, driver: Arc<dyn Driver>) -> Result<(), DriverError> {
        let _recovery = self.recovery.lock().await;
        if Arc::ptr_eq(&driver, &self.snapshot().driver) {
            return Err(StorageError::new(StorageErrorKind::Conflict).into());
        }
        self.publish(Arc::new(Generation::with_lease(driver, self.ttl)?));
        Ok(())
    }
    pub fn generation(&self) -> String {
        self.snapshot().generation().to_owned()
    }
    pub async fn expire(&self) {
        self.snapshot().expire().await;
    }
    pub async fn call(
        &self,
        generation: &str,
        op: Op,
        data: Vec<u8>,
    ) -> Result<p::Reply, DriverError> {
        let mut current = self.snapshot();
        if matches!(op, Op::Describe)
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
                    Generation::with_lease(driver, self.ttl)
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
    resources: Mutex<Resources>,
    ttl: Duration,
    poisoned: AtomicBool,
}
struct InitializationGuard<'a> {
    poisoned: &'a AtomicBool,
    completed: bool,
}
impl Drop for InitializationGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.poisoned.store(true, Ordering::Release);
        }
    }
}
impl Generation {
    fn with_lease(driver: Arc<dyn Driver>, ttl: Duration) -> Result<Self, DriverError> {
        if ttl.is_zero() {
            return Err(c::invalid());
        }
        let generation = new_instance_id()?.as_str().to_owned();
        let description = c::describe(driver.as_ref(), generation);
        p::negotiate(&description, &[]).map_err(|_| unsupported())?;
        c::driver_info(&description)?;
        Ok(Self {
            description,
            driver,
            initialized: Mutex::new(None),
            resources: Mutex::new(Resources::default()),
            ttl,
            poisoned: AtomicBool::new(false),
        })
    }
    pub async fn expire(&self) {
        self.resources.lock().await.expire(self.ttl);
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
    async fn storage(&self, handle: u64) -> Result<Arc<dyn Storage>, DriverError> {
        get(&mut self.resources.lock().await.storage, handle)
    }
    pub async fn call(
        &self,
        generation: &str,
        op: Op,
        data: Vec<u8>,
    ) -> Result<p::Reply, DriverError> {
        if self.poisoned.load(Ordering::Acquire) && !matches!(&op, Op::Release { .. }) {
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
                // backend state. Releases remain available for best-effort cleanup.
                self.poisoned.store(true, Ordering::Release);
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
        if matches!(op, Op::Describe) {
            return Ok(p::Reply::new(Value::Description {
                description: self.description.clone(),
            }));
        }
        if generation != self.generation() {
            return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
        }
        if !data.is_empty()
            && !matches!(
                op,
                Op::WriterChunk { .. }
                    | Op::Storage {
                        request: p::StorageOp::SetXattr { .. },
                        ..
                    }
            )
        {
            return Err(StorageError::new(StorageErrorKind::InvalidRequest).into());
        }
        self.expire().await;
        if let Op::Initialize { settings } = op {
            let mut initialized = self.initialized.lock().await;
            if let Some(previous) = &*initialized {
                return if previous == &settings {
                    Ok(p::Reply::unit())
                } else {
                    Err(StorageError::new(StorageErrorKind::Conflict).into())
                };
            }
            let mut guard = InitializationGuard {
                poisoned: &self.poisoned,
                completed: false,
            };
            let outcome = self.driver.initialize(settings.clone().into()).await;
            guard.completed = true;
            outcome?;
            *initialized = Some(settings);
            return Ok(p::Reply::unit());
        }
        if self.initialized.lock().await.is_none() {
            return Err(StorageError::new(StorageErrorKind::Unavailable).into());
        }
        let value = match op {
            Op::Prepare { config } => {
                let prepared = self.provisioner()?.prepare(config.into()).await?;
                Value::Prepared {
                    config: prepared.config().clone().into_object(),
                    details: prepared.details().clone(),
                }
            }
            Op::Migrate {
                from_version,
                config,
            } => Value::Config {
                config: self
                    .provisioner()?
                    .migrate_config(from_version, config.into())
                    .await?
                    .into_object(),
            },
            Op::TestConnection { config } => {
                self.provisioner()?.test_connection(&config.into()).await?;
                Value::Unit
            }
            Op::GetInstance { id: raw } => Value::MaybeInstance {
                instance: self.registry()?.get(id(&raw)?).await?.map(c::instance),
            },
            Op::ListInstances { after, limit } => {
                let mut q = InstanceQuery::new(c::limit(limit)?)?;
                if let Some(after) = after {
                    q = q.with_after(id(&after)?);
                }
                let page = self.registry()?.list(q).await?;
                Value::Instances {
                    instances: page.instances.into_iter().map(c::instance).collect(),
                    next_after: page.next_after.map(|id| id.as_str().into()),
                }
            }
            Op::CreateInstance { config } => Value::Instance {
                instance: c::instance(self.registry()?.create(config.into()).await?),
            },
            Op::RemoveInstance { id: raw } => Value::Removed {
                removed: self.registry()?.remove(id(&raw)?).await?
                    == RemoveInstanceOutcome::Removed,
            },
            Op::Open { id: raw, config } => {
                let host = self.description.ownership == p::Ownership::Host;
                if host != config.is_some() {
                    return Err(StorageError::new(StorageErrorKind::InvalidRequest).into());
                }
                let context = match config {
                    Some(config) => OpenContext::host_managed(id(&raw)?, config.into()),
                    None => OpenContext::self_managed(id(&raw)?),
                };
                let storage = self.driver.open(context).await?;
                validate_storage_capabilities(self.driver.info().capabilities(), storage.as_ref())?;
                let chunk_size =
                    u32::try_from(storage.chunk_size().get()).map_err(|_| c::invalid())?;
                if chunk_size as usize > p::MAX_DATA_BYTES {
                    return Err(c::invalid());
                }
                let info = p::StorageInfo {
                    capabilities: c::capabilities(storage.capabilities()),
                    chunk_size,
                };
                let mut resources = self.resources.lock().await;
                let handle = resources.allocate()?;
                resources.storage.insert(handle, slot(storage));
                Value::Storage { handle, info }
            }
            Op::Storage { handle, request } => {
                return self
                    .storage_op(self.storage(handle).await?, request, data)
                    .await;
            }
            Op::Read {
                handle,
                path: raw,
                offset,
                length,
            } => {
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
                resources
                    .readers
                    .insert(handle, slot(Arc::new(Mutex::new(reader))));
                Value::Reader { handle }
            }
            Op::Write {
                handle,
                path: raw,
                replace,
                atomic,
            } => {
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
                resources
                    .writers
                    .insert(handle, slot(Arc::new(Mutex::new(writer))));
                Value::Writer { handle }
            }
            Op::ReaderNext { handle } => {
                let reader = get(&mut self.resources.lock().await.readers, handle)?;
                let result = reader.lock().await.next_chunk().await;
                match result {
                    Ok(Some(data)) if !data.is_empty() && data.len() <= p::MAX_DATA_BYTES => {
                        return Ok(p::Reply {
                            value: Value::Chunk { eof: false },
                            data,
                        });
                    }
                    Ok(None) => {
                        self.resources.lock().await.readers.remove(&handle);
                        Value::Chunk { eof: true }
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
            Op::WriterChunk { handle } => {
                let writer = get(&mut self.resources.lock().await.writers, handle)?;
                writer.lock().await.write_chunk(&data).await?;
                Value::Unit
            }
            Op::Commit { handle } => {
                let writer = self
                    .resources
                    .lock()
                    .await
                    .writers
                    .remove(&handle)
                    .ok_or_else(missing)?
                    .value;
                let entry = writer.lock().await.commit().await?;
                Value::Entry {
                    entry: c::entry(entry)?,
                }
            }
            Op::Abort { handle } => {
                let writer = self
                    .resources
                    .lock()
                    .await
                    .writers
                    .remove(&handle)
                    .ok_or_else(missing)?
                    .value;
                writer.lock().await.abort().await?;
                Value::Unit
            }
            Op::Release { kind, handle } => {
                let mut resources = self.resources.lock().await;
                if resources.kind(handle).is_some_and(|actual| actual != kind) {
                    return Err(c::invalid());
                }
                match kind {
                    p::ResourceKind::Storage => {
                        resources.storage.remove(&handle);
                    }
                    p::ResourceKind::Reader => {
                        resources.readers.remove(&handle);
                    }
                    p::ResourceKind::Writer => {
                        resources.writers.remove(&handle);
                    }
                }
                Value::Unit
            }
            Op::Renew { resources } => {
                if resources.len() > p::MAX_RESOURCES {
                    return Err(c::invalid());
                }
                let mut table = self.resources.lock().await;
                for r in resources {
                    match r.kind {
                        p::ResourceKind::Storage => {
                            if let Some(slot) = table.storage.get_mut(&r.handle) {
                                slot.touched = Instant::now();
                            }
                        }
                        p::ResourceKind::Reader => {
                            if let Some(slot) = table.readers.get_mut(&r.handle) {
                                slot.touched = Instant::now();
                            }
                        }
                        p::ResourceKind::Writer => {
                            if let Some(slot) = table.writers.get_mut(&r.handle) {
                                slot.touched = Instant::now();
                            }
                        }
                    }
                }
                Value::Unit
            }
            Op::Describe | Op::Initialize { .. } => return Err(c::invalid()),
        };
        Ok(p::Reply::new(value))
    }
    async fn storage_op(
        &self,
        s: Arc<dyn Storage>,
        op: p::StorageOp,
        data: Vec<u8>,
    ) -> Result<p::Reply, DriverError> {
        use p::StorageOp::*;
        let value = match op {
            Stat { path: raw } => Value::Entry {
                entry: c::entry(
                    s.stat_entry(StorageStatEntryRequest { path: path(&raw)? })
                        .await?,
                )?,
            },
            List {
                path: raw,
                after,
                limit,
            } => {
                let q = StorageDirQuery {
                    path: path(&raw)?,
                    after,
                    limit: c::limit(limit)?,
                };
                q.validate()?;
                let page = s.list_entries(q).await?;
                Value::Page {
                    entries: page
                        .entries
                        .into_iter()
                        .map(c::entry)
                        .collect::<Result<_, _>>()?,
                    next_after: page.next_after,
                }
            }
            CreateDir {
                path: raw,
                recursive,
            } => Value::Entry {
                entry: c::entry(
                    s.create_dir(StorageCreateDirRequest {
                        path: path(&raw)?,
                        recursive,
                    })
                    .await?,
                )?,
            },
            Remove { path: raw } => {
                s.remove_entry(StorageRemoveEntryRequest { path: path(&raw)? })
                    .await?;
                Value::Unit
            }
            Move {
                source,
                destination,
                replace,
            } => Value::Entry {
                entry: c::entry(
                    s.move_entry(StorageMoveEntryRequest {
                        source: path(&source)?,
                        destination: path(&destination)?,
                        replace,
                    })
                    .await?,
                )?,
            },
            GetPermissions { path: raw } => Value::Permissions {
                mode: s
                    .permissions()
                    .ok_or_else(unsupported)?
                    .get_permissions(path(&raw)?)
                    .await?
                    .bits(),
            },
            SetPermissions { path: raw, mode } => {
                s.permissions()
                    .ok_or_else(unsupported)?
                    .set_permissions(path(&raw)?, EntryPermissions::new(mode)?)
                    .await?;
                Value::Unit
            }
            ListXattrs {
                path: raw,
                max_bytes,
            } => Value::Names {
                names: s
                    .xattrs()
                    .ok_or_else(unsupported)?
                    .list_xattrs(path(&raw)?, c::limit(max_bytes)?)
                    .await?
                    .into_iter()
                    .map(|n| n.as_str().into())
                    .collect(),
            },
            GetXattr {
                path: raw,
                name,
                max_bytes,
            } => {
                return Ok(p::Reply {
                    value: Value::Bytes,
                    data: s
                        .xattrs()
                        .ok_or_else(unsupported)?
                        .get_xattr(path(&raw)?, XattrName::parse(&name)?, c::limit(max_bytes)?)
                        .await?,
                });
            }
            SetXattr {
                path: raw,
                name,
                mode,
            } => {
                let mode = match mode.as_str() {
                    "upsert" => XattrWriteMode::Upsert,
                    "create_new" => XattrWriteMode::CreateNew,
                    "replace" => XattrWriteMode::Replace,
                    _ => return Err(StorageError::new(StorageErrorKind::InvalidRequest).into()),
                };
                s.xattrs()
                    .ok_or_else(unsupported)?
                    .set_xattr(path(&raw)?, XattrName::parse(&name)?, data, mode)
                    .await?;
                Value::Unit
            }
            RemoveXattr { path: raw, name } => {
                s.xattrs()
                    .ok_or_else(unsupported)?
                    .remove_xattr(path(&raw)?, XattrName::parse(&name)?)
                    .await?;
                Value::Unit
            }
        };
        Ok(p::Reply::new(value))
    }
}
