//! Transport-independent host proxies with common session and capability rules.
//! Native server dispatch is provided separately by vfs-plugin-grpc-server.
pub use vfs_plugin_codec as convert;

use ah_vfs::{
    application::{driver_info::DriverInfo, dto::*, port::*},
    domain::*,
};
use std::{
    collections::HashSet,
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

pub type TransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<p::Reply, DriverError>> + Send + 'a>>;
pub trait Transport: Send + Sync {
    /// Services this execution environment supplies to guest code. Native RPC
    /// plugins own their process resources and do not import host services.
    fn host_capabilities(&self) -> &[&str] {
        &[]
    }
    fn call<'a>(&'a self, generation: &'a str, operation: Op, data: Vec<u8>)
    -> TransportFuture<'a>;
}

struct Runtime {
    transport: Arc<dyn Transport>,
    generation: String,
    leases: Mutex<HashSet<(p::ResourceKind, u64)>>,
    heartbeat: Mutex<Option<tokio::task::JoinHandle<()>>>,
    stale: AtomicBool,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(task) = self
            .heartbeat
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}
impl Runtime {
    fn new(transport: Arc<dyn Transport>, generation: String) -> Arc<Self> {
        let runtime = Arc::new(Self {
            transport,
            generation,
            leases: Mutex::new(HashSet::new()),
            heartbeat: Mutex::new(None),
            stale: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&runtime);
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.tick().await;
            loop {
                interval.tick().await;
                let Some(runtime) = weak.upgrade() else {
                    break;
                };
                if runtime.is_stale() {
                    break;
                }
                let resources = runtime
                    .leases
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .iter()
                    .map(|(kind, handle)| p::ResourceRef {
                        kind: *kind as i32,
                        handle: *handle,
                    })
                    .collect::<Vec<_>>();
                if !resources.is_empty() {
                    let _ = runtime
                        .call(Op::Renew(p::Renew { resources }), vec![])
                        .await;
                }
            }
        });
        *runtime
            .heartbeat
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(task);
        runtime
    }
    async fn call(&self, op: Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        if self.is_stale() && !matches!(op, Op::Release(_)) {
            return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
        }
        if data.len() > p::MAX_DATA_BYTES {
            return Err(StorageError::new(StorageErrorKind::TooLarge).into());
        }
        let mutates = op.mutates();
        let reply = self
            .transport
            .call(&self.generation, op.clone(), data)
            .await
            .inspect_err(|error| {
                if error.storage_error().is_some_and(|error| {
                    matches!(
                        error.kind(),
                        StorageErrorKind::StaleHandle
                            | StorageErrorKind::Unavailable
                            | StorageErrorKind::TimedOut
                            | StorageErrorKind::OutcomeUnknown
                    )
                }) {
                    self.stale.store(true, Ordering::Release);
                }
            })?;
        if p::validate_reply(&reply).is_err()
            || !reply
                .result
                .as_ref()
                .is_some_and(|value| op.accepts_reply(value))
        {
            return Err(bad_response(mutates).into());
        }
        Ok(reply)
    }
    fn is_stale(&self) -> bool {
        self.stale.load(Ordering::Acquire)
    }
    async fn value(&self, op: Op) -> Result<Value, DriverError> {
        let mutates = op.mutates();
        let r = self.call(op, vec![]).await?;
        if !r.data.is_empty() {
            return Err(bad_response(mutates).into());
        }
        Ok(r.result.ok_or_else(convert::storage_invalid)?)
    }
    async fn unit(&self, op: Op) -> Result<(), DriverError> {
        let mutates = op.mutates();
        match self.value(op).await? {
            Value::Unit(p::Empty {}) => Ok(()),
            _ => Err(bad_response(mutates).into()),
        }
    }
}
fn bad_response(mutates: bool) -> StorageError {
    StorageError::new(if mutates {
        StorageErrorKind::OutcomeUnknown
    } else {
        StorageErrorKind::InvalidData
    })
}

/// A fully described plugin. Register it normally to initialize plugin settings.
/// Transport failures retire the affected generation. Later instance/open requests
/// recover before dispatch; failed operations and retained streams are never replayed.
pub struct PluginDriver {
    info: DriverInfo,
    remote: Arc<RemoteInstances>,
    ownership: p::Ownership,
    negotiated: p::NegotiatedProtocol,
}
impl PluginDriver {
    pub async fn connect(transport: Arc<dyn Transport>) -> Result<Self, DriverError> {
        let description = describe(transport.as_ref()).await?;
        let negotiated = p::negotiate(&description, transport.host_capabilities())
            .map_err(|_| StorageError::new(StorageErrorKind::UnsupportedOperation))?;
        let info = convert::driver_info(&description)?;
        let runtime = Runtime::new(transport.clone(), description.generation.clone());
        Ok(Self {
            info,
            remote: Arc::new(RemoteInstances {
                transport,
                description: description.clone(),
                connection: Arc::new(tokio::sync::Mutex::new(Connection {
                    runtime,
                    settings: None,
                })),
            }),
            ownership: description.ownership(),
            negotiated,
        })
    }
    pub fn negotiated_protocol(&self) -> &p::NegotiatedProtocol {
        &self.negotiated
    }

    /// Refresh the generation and initialize a replacement with the original settings.
    /// Declarations must remain identical; changed declarations require registration
    /// of a new driver. Existing resources are never rebound or replayed.
    pub async fn reconnect(&self) -> Result<(), DriverError> {
        self.remote.reconnect(true).await.map(|_| ())
    }
}
async fn describe(transport: &dyn Transport) -> Result<p::Description, DriverError> {
    let reply = transport
        .call("", Op::Describe(p::Empty {}), vec![])
        .await?;
    p::validate_reply(&reply).map_err(|_| convert::invalid())?;
    match reply.result.ok_or_else(convert::storage_invalid)? {
        Value::Description(description) if reply.data.is_empty() => Ok(*description),
        _ => Err(convert::invalid()),
    }
}
impl Driver for PluginDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn instances(&self) -> InstanceManagement {
        match self.ownership {
            p::Ownership::Host => InstanceManagement::HostManaged(self.remote.clone()),
            _ => InstanceManagement::SelfManaged(self.remote.clone()),
        }
    }
    fn initialize(&self, settings: DriverSettings) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            let mut connection = self.remote.connection.lock().await;
            if connection.settings.is_some() {
                return Err(StorageError::new(StorageErrorKind::Conflict).into());
            }
            let settings: p::Object = settings.into_object().into();
            connection
                .runtime
                .unit(Op::Initialize(p::Initialize {
                    settings: Some(settings.clone()),
                }))
                .await?;
            connection.settings = Some(settings);
            Ok(())
        })
    }
    fn open(&self, context: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        Box::pin(async move {
            if (self.ownership == p::Ownership::Host) != context.config().is_some() {
                return Err(convert::invalid());
            }
            let runtime = self.remote.runtime().await?;
            let value = runtime
                .value(Op::Open(p::Open {
                    id: context.instance_id().as_str().into(),
                    config: context
                        .config()
                        .cloned()
                        .map(|config| config.into_object().into()),
                }))
                .await?;
            let Value::Storage(p::Storage {
                handle,
                info: Some(info),
            }) = value
            else {
                return Err(convert::invalid());
            };
            let lease = Lease::new(runtime, p::ResourceKind::Storage, handle)?;
            let capabilities = convert::decode_capabilities(&info.capabilities)?;
            if info.chunk_size == 0 || info.chunk_size as usize > p::MAX_DATA_BYTES {
                return Err(convert::invalid());
            }
            let storage = RemoteStorage {
                lease,
                capabilities,
                chunk_size: convert::limit(info.chunk_size).map_err(DriverError::from)?,
            };
            validate_storage_capabilities(self.info.capabilities(), &storage)?;
            Ok(Arc::new(storage) as Arc<dyn Storage>)
        })
    }
}

struct RemoteInstances {
    transport: Arc<dyn Transport>,
    description: p::Description,
    connection: Arc<tokio::sync::Mutex<Connection>>,
}
struct Connection {
    runtime: Arc<Runtime>,
    settings: Option<p::Object>,
}
impl RemoteInstances {
    async fn runtime(&self) -> Result<Arc<Runtime>, DriverError> {
        let connection = self.connection.lock().await;
        if !connection.runtime.is_stale() {
            return Ok(connection.runtime.clone());
        }
        drop(connection);
        self.reconnect(false).await
    }
    async fn value(&self, op: Op) -> Result<Value, DriverError> {
        self.runtime().await?.value(op).await
    }
    async fn reconnect(&self, force: bool) -> Result<Arc<Runtime>, DriverError> {
        let transport = self.transport.clone();
        let mut expected = self.description.clone();
        let connection = self.connection.clone();
        // Recovery owns initialization even if its initiating request is cancelled.
        // Concurrent callers share the published runtime and never replay their op.
        tokio::spawn(async move {
            let mut connection = connection.lock().await;
            if !force && !connection.runtime.is_stale() {
                return Ok(connection.runtime.clone());
            }
            let mut description = describe(transport.as_ref()).await?;
            let generation = description.generation.clone();
            let changed = generation != connection.runtime.generation;
            if changed {
                connection.runtime.stale.store(true, Ordering::Release);
            }
            p::negotiate(&description, transport.host_capabilities())
                .map_err(|_| StorageError::new(StorageErrorKind::UnsupportedOperation))?;
            convert::driver_info(&description)?;
            expected.generation.clear();
            description.generation.clear();
            if expected != description {
                return Err(convert::invalid());
            }
            if !changed && !connection.runtime.is_stale() {
                return Ok(connection.runtime.clone());
            }
            let runtime = Runtime::new(transport, generation);
            if changed && let Some(settings) = &connection.settings {
                runtime
                    .unit(Op::Initialize(p::Initialize {
                        settings: Some(settings.clone()),
                    }))
                    .await?;
            }
            connection.runtime = runtime.clone();
            Ok(runtime)
        })
        .await
        .map_err(|_| StorageError::new(StorageErrorKind::Unavailable))?
    }
}
impl InstanceProvisioner for RemoteInstances {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
        Box::pin(async move {
            match self
                .value(Op::Prepare(p::Config {
                    config: Some(config.into_object().into()),
                }))
                .await?
            {
                Value::Prepared(p::Prepared {
                    config: Some(config),
                    details: Some(details),
                }) => Ok(PreparedConfig::new(
                    convert::object(Some(config))?.into(),
                    convert::object(Some(details))?,
                )),
                _ => Err(convert::invalid()),
            }
        })
    }
    fn migrate_config(
        &self,
        from_version: u32,
        config: InstanceConfig,
    ) -> DriverFuture<'_, InstanceConfig> {
        Box::pin(async move {
            match self
                .value(Op::Migrate(p::Migrate {
                    from_version,
                    config: Some(config.into_object().into()),
                }))
                .await?
            {
                Value::Config(p::Config {
                    config: Some(config),
                }) => Ok(convert::object(Some(config))?.into()),
                _ => Err(convert::invalid()),
            }
        })
    }
    fn test_connection<'a>(&'a self, config: &'a InstanceConfig) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            self.runtime()
                .await?
                .unit(Op::TestConnection(p::Config {
                    config: Some(config.clone().into_object().into()),
                }))
                .await
        })
    }
}
impl InstanceRegistry for RemoteInstances {
    fn get(&self, id: InstanceId) -> DriverFuture<'_, Option<InstanceInfo>> {
        Box::pin(async move {
            match self
                .value(Op::GetInstance(p::InstanceId {
                    id: id.as_str().into(),
                }))
                .await?
            {
                Value::MaybeInstance(p::MaybeInstance { instance }) => {
                    let result = instance.map(convert::decode_instance).transpose()?;
                    if result.as_ref().is_some_and(|v| v.id != id) {
                        return Err(convert::invalid());
                    }
                    Ok(result)
                }
                _ => Err(convert::invalid()),
            }
        })
    }
    fn list(&self, query: InstanceQuery) -> DriverFuture<'_, InstancePage> {
        Box::pin(async move {
            match self
                .value(Op::ListInstances(p::ListInstances {
                    after: query.after().map(|id| id.as_str().into()),
                    limit: query.limit().get() as u32,
                }))
                .await?
            {
                Value::Instances(p::Instances {
                    instances,
                    next_after,
                }) => {
                    let instances = instances
                        .into_iter()
                        .map(convert::decode_instance)
                        .collect::<Result<Vec<_>, _>>()?;
                    if instances.len() > query.limit().get()
                        || instances
                            .windows(2)
                            .any(|v| v[0].id.as_str() >= v[1].id.as_str())
                        || instances.first().is_some_and(|v| {
                            query.after().is_some_and(|a| v.id.as_str() <= a.as_str())
                        })
                        || next_after
                            .as_deref()
                            .is_some_and(|a| instances.last().is_none_or(|v| v.id.as_str() != a))
                    {
                        return Err(convert::invalid());
                    }
                    Ok(InstancePage {
                        instances,
                        next_after: next_after
                            .map(|id| InstanceId::parse(&id).map_err(|_| convert::invalid()))
                            .transpose()?,
                    })
                }
                _ => Err(convert::invalid()),
            }
        })
    }
    fn create(&self, config: InstanceConfig) -> DriverFuture<'_, InstanceInfo> {
        Box::pin(async move {
            match self
                .value(Op::CreateInstance(p::Config {
                    config: Some(config.into_object().into()),
                }))
                .await?
            {
                Value::Instance(instance) => {
                    convert::decode_instance(instance).map_err(|_| bad_response(true).into())
                }
                _ => Err(bad_response(true).into()),
            }
        })
    }
    fn remove(&self, id: InstanceId) -> DriverFuture<'_, RemoveInstanceOutcome> {
        Box::pin(async move {
            match self
                .value(Op::RemoveInstance(p::InstanceId {
                    id: id.as_str().into(),
                }))
                .await?
            {
                Value::Removed(p::Removed { removed }) => Ok(if removed {
                    RemoveInstanceOutcome::Removed
                } else {
                    RemoveInstanceOutcome::AlreadyAbsent
                }),
                _ => Err(bad_response(true).into()),
            }
        })
    }
}

struct Lease {
    runtime: Arc<Runtime>,
    kind: p::ResourceKind,
    handle: u64,
}
impl Lease {
    fn new(runtime: Arc<Runtime>, kind: p::ResourceKind, handle: u64) -> Result<Self, DriverError> {
        if runtime.is_stale() {
            return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
        }
        if handle == 0 {
            return Err(convert::invalid());
        }
        if !runtime
            .leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert((kind, handle))
        {
            return Err(convert::invalid());
        }
        Ok(Self {
            runtime,
            kind,
            handle,
        })
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.runtime
            .leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(self.kind, self.handle));
        if let Ok(executor) = tokio::runtime::Handle::try_current() {
            let runtime = self.runtime.clone();
            let operation = Op::Release(p::ResourceRef {
                kind: self.kind as i32,
                handle: self.handle,
            });
            executor.spawn(async move {
                let _ = runtime.call(operation, vec![]).await;
            });
        }
    }
}

struct RemoteStorage {
    lease: Lease,
    capabilities: StorageCapabilities,
    chunk_size: NonZeroUsize,
}
impl RemoteStorage {
    async fn call(&self, request: p::StorageOp, data: Vec<u8>) -> Result<p::Reply, StorageError> {
        self.lease
            .runtime
            .call(
                Op::Storage(p::StorageCall {
                    handle: self.lease.handle,
                    operation: Some(request),
                }),
                data,
            )
            .await
            .map_err(convert::storage_error)
    }
    async fn entry(
        &self,
        request: p::StorageOp,
        path: StoragePath,
    ) -> Result<StorageEntry, StorageError> {
        let mutates = !matches!(request, p::StorageOp::Stat(_));
        let r = self.call(request, vec![]).await?;
        match r.result.ok_or_else(convert::storage_invalid)? {
            Value::Entry(entry) if r.data.is_empty() => {
                let entry = convert::decode_entry(entry).map_err(|_| bad_response(mutates))?;
                if entry.path != path {
                    return Err(bad_response(mutates));
                }
                Ok(entry)
            }
            _ => Err(bad_response(mutates)),
        }
    }
    async fn unit(&self, request: p::StorageOp, data: Vec<u8>) -> Result<(), StorageError> {
        let r = self.call(request, data).await?;
        match r.result.ok_or_else(convert::storage_invalid)? {
            Value::Unit(p::Empty {}) if r.data.is_empty() => Ok(()),
            _ => Err(bad_response(true)),
        }
    }
}
impl Storage for RemoteStorage {
    fn is_stale(&self) -> bool {
        self.lease.runtime.is_stale()
    }
    fn capabilities(&self) -> StorageCapabilities {
        self.capabilities
    }
    fn chunk_size(&self) -> NonZeroUsize {
        self.chunk_size
    }
    fn permissions(&self) -> Option<&dyn StoragePermissions> {
        self.capabilities.permissions.can_read().then_some(self)
    }
    fn xattrs(&self) -> Option<&dyn StorageXattrs> {
        self.capabilities.xattrs.can_read().then_some(self)
    }
    fn stat_entry(&self, r: StorageStatEntryRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            self.entry(
                p::StorageOp::Stat(p::Path {
                    path: r.path.as_str().into(),
                }),
                r.path,
            )
            .await
        })
    }
    fn create_dir(&self, r: StorageCreateDirRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            self.entry(
                p::StorageOp::CreateDir(p::CreateDir {
                    path: r.path.as_str().into(),
                    recursive: r.recursive,
                }),
                r.path,
            )
            .await
        })
    }
    fn move_entry(&self, r: StorageMoveEntryRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            self.entry(
                p::StorageOp::Move(p::Move {
                    source: r.source.as_str().into(),
                    destination: r.destination.as_str().into(),
                    replace: r.replace,
                }),
                r.destination,
            )
            .await
        })
    }
    fn remove_entry(&self, r: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            self.unit(
                p::StorageOp::Remove(p::Path {
                    path: r.path.as_str().into(),
                }),
                vec![],
            )
            .await
        })
    }
    fn list_entries(&self, r: StorageDirQuery) -> StorageFuture<'_, StorageDirPage> {
        Box::pin(async move {
            r.validate()?;
            let reply = self
                .call(
                    p::StorageOp::List(p::List {
                        path: r.path.as_str().into(),
                        after: r.after.clone(),
                        limit: r.limit.get() as u32,
                    }),
                    vec![],
                )
                .await?;
            let Value::Page(p::Page {
                entries,
                next_after,
            }) = reply.result.ok_or_else(convert::storage_invalid)?
            else {
                return Err(convert::storage_invalid());
            };
            if !reply.data.is_empty() {
                return Err(convert::storage_invalid());
            }
            let entries = entries
                .into_iter()
                .map(convert::decode_entry)
                .collect::<Result<Vec<_>, _>>()?;
            if entries.len() > r.limit.get()
                || entries
                    .iter()
                    .any(|e| e.path.parent().as_ref() != Some(&r.path))
                || entries
                    .windows(2)
                    .any(|v| v[0].path.as_str() >= v[1].path.as_str())
                || entries.first().is_some_and(|e| {
                    r.after
                        .as_ref()
                        .is_some_and(|a| e.path.name().is_none_or(|n| n <= a.as_str()))
                })
                || next_after
                    .as_deref()
                    .is_some_and(|n| entries.last().and_then(|e| e.path.name()) != Some(n))
            {
                return Err(convert::storage_invalid());
            }
            Ok(StorageDirPage {
                entries,
                next_after,
            })
        })
    }
    fn read_file(
        &self,
        r: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            let value = self
                .lease
                .runtime
                .value(Op::Read(p::Read {
                    handle: self.lease.handle,
                    path: r.path.as_str().into(),
                    offset: r.offset,
                    length: r.length,
                }))
                .await
                .map_err(convert::storage_error)?;
            match value {
                Value::Reader(p::Handle { handle }) => Ok(Box::new(RemoteReader {
                    lease: Some(
                        Lease::new(self.lease.runtime.clone(), p::ResourceKind::Reader, handle)
                            .map_err(convert::storage_error)?,
                    ),
                    eof: false,
                    remaining: r.length,
                })
                    as Box<dyn StorageFileReader>),
                _ => Err(convert::storage_invalid()),
            }
        })
    }
    fn write_file(
        &self,
        r: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            let value = self
                .lease
                .runtime
                .value(Op::Write(p::Write {
                    handle: self.lease.handle,
                    path: r.path.as_str().into(),
                    replace: r.mode == StorageWriteFileMode::Replace,
                    atomic: r.require_atomic_publish,
                }))
                .await
                .map_err(convert::storage_error)?;
            match value {
                Value::Writer(p::Handle { handle }) => Ok(Box::new(RemoteWriter {
                    lease: Some(
                        Lease::new(self.lease.runtime.clone(), p::ResourceKind::Writer, handle)
                            .map_err(convert::storage_error)?,
                    ),
                    path: r.path,
                })
                    as Box<dyn StorageFileWriter>),
                _ => Err(convert::storage_invalid()),
            }
        })
    }
}
fn closed() -> StorageError {
    StorageError::new(StorageErrorKind::SessionClosed)
}
struct RemoteReader {
    lease: Option<Lease>,
    eof: bool,
    remaining: Option<u64>,
}
impl StorageFileReader for RemoteReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            if self.eof {
                return Ok(None);
            }
            let lease = self.lease.take().ok_or_else(closed)?;
            let reply = lease
                .runtime
                .call(
                    Op::ReaderNext(p::Handle {
                        handle: lease.handle,
                    }),
                    vec![],
                )
                .await
                .map_err(convert::storage_error)?;
            match reply.result.ok_or_else(convert::storage_invalid)? {
                Value::Chunk(p::Chunk { eof: true }) if reply.data.is_empty() => {
                    self.eof = true;
                    Ok(None)
                }
                Value::Chunk(p::Chunk { eof: false }) if !reply.data.is_empty() => {
                    if let Some(remaining) = self.remaining.as_mut() {
                        *remaining = remaining
                            .checked_sub(reply.data.len() as u64)
                            .ok_or_else(convert::storage_invalid)?;
                    }
                    self.lease = Some(lease);
                    Ok(Some(reply.data))
                }
                _ => Err(convert::storage_invalid()),
            }
        })
    }
}
struct RemoteWriter {
    lease: Option<Lease>,
    path: StoragePath,
}
impl StorageFileWriter for RemoteWriter {
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let lease = self.lease.take().ok_or_else(closed)?;
            for chunk in data.chunks(p::MAX_DATA_BYTES) {
                let reply = lease
                    .runtime
                    .call(
                        Op::WriterChunk(p::Handle {
                            handle: lease.handle,
                        }),
                        chunk.to_vec(),
                    )
                    .await
                    .map_err(convert::storage_error)?;
                if !matches!(
                    reply.result.ok_or_else(convert::storage_invalid)?,
                    Value::Unit(p::Empty {})
                ) || !reply.data.is_empty()
                {
                    return Err(bad_response(true));
                }
            }
            self.lease = Some(lease);
            Ok(())
        })
    }
    fn commit(&mut self) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            let lease = self.lease.take().ok_or_else(closed)?;
            let reply = lease
                .runtime
                .value(Op::Commit(p::Handle {
                    handle: lease.handle,
                }))
                .await
                .map_err(convert::storage_error)?;
            match reply {
                Value::Entry(entry) => {
                    let entry = convert::decode_entry(entry).map_err(|_| bad_response(true))?;
                    if entry.path != self.path {
                        return Err(bad_response(true));
                    }
                    Ok(entry)
                }
                _ => Err(bad_response(true)),
            }
        })
    }
    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let lease = self.lease.take().ok_or_else(closed)?;
            lease
                .runtime
                .unit(Op::Abort(p::Handle {
                    handle: lease.handle,
                }))
                .await
                .map_err(convert::storage_error)
        })
    }
}

impl StoragePermissions for RemoteStorage {
    fn get_permissions(&self, path: StoragePath) -> StorageFuture<'_, EntryPermissions> {
        Box::pin(async move {
            let r = self
                .call(
                    p::StorageOp::GetPermissions(p::Path {
                        path: path.as_str().into(),
                    }),
                    vec![],
                )
                .await?;
            match r.result.ok_or_else(convert::storage_invalid)? {
                Value::Permissions(p::Permissions { mode }) if r.data.is_empty() => {
                    EntryPermissions::new(mode).map_err(|_| convert::storage_invalid())
                }
                _ => Err(convert::storage_invalid()),
            }
        })
    }
    fn set_permissions(
        &self,
        path: StoragePath,
        permissions: EntryPermissions,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            self.unit(
                p::StorageOp::SetPermissions(p::SetPermissions {
                    path: path.as_str().into(),
                    mode: permissions.bits(),
                }),
                vec![],
            )
            .await
        })
    }
}
impl StorageXattrs for RemoteStorage {
    fn list_xattrs(
        &self,
        path: StoragePath,
        max_bytes: NonZeroUsize,
    ) -> StorageFuture<'_, Vec<XattrName>> {
        Box::pin(async move {
            let max_bytes = u32::try_from(max_bytes.get())
                .map_err(|_| StorageError::new(StorageErrorKind::TooLarge))?;
            let r = self
                .call(
                    p::StorageOp::ListXattrs(p::ListXattrs {
                        path: path.as_str().into(),
                        max_bytes,
                    }),
                    vec![],
                )
                .await?;
            match r.result.ok_or_else(convert::storage_invalid)? {
                Value::Names(p::Names { names }) if r.data.is_empty() => {
                    if names.windows(2).any(|v| v[0] >= v[1])
                        || names.iter().map(|n| n.len() + 1).sum::<usize>() > max_bytes as usize
                    {
                        return Err(convert::storage_invalid());
                    }
                    names
                        .into_iter()
                        .map(|n| XattrName::parse(&n).map_err(|_| convert::storage_invalid()))
                        .collect()
                }
                _ => Err(convert::storage_invalid()),
            }
        })
    }
    fn get_xattr(
        &self,
        path: StoragePath,
        name: XattrName,
        max_bytes: NonZeroUsize,
    ) -> StorageFuture<'_, Vec<u8>> {
        Box::pin(async move {
            let max_bytes = u32::try_from(max_bytes.get())
                .map_err(|_| StorageError::new(StorageErrorKind::TooLarge))?;
            let r = self
                .call(
                    p::StorageOp::GetXattr(p::GetXattr {
                        path: path.as_str().into(),
                        name: name.as_str().into(),
                        max_bytes,
                    }),
                    vec![],
                )
                .await?;
            if matches!(
                r.result.ok_or_else(convert::storage_invalid)?,
                Value::Bytes(p::Empty {})
            ) && r.data.len() <= max_bytes as usize
            {
                Ok(r.data)
            } else {
                Err(convert::storage_invalid())
            }
        })
    }
    fn set_xattr(
        &self,
        path: StoragePath,
        name: XattrName,
        value: Vec<u8>,
        mode: XattrWriteMode,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            self.unit(
                p::StorageOp::SetXattr(p::SetXattr {
                    path: path.as_str().into(),
                    name: name.as_str().into(),
                    mode: match mode {
                        XattrWriteMode::Upsert => p::XattrWriteMode::Upsert,
                        XattrWriteMode::CreateNew => p::XattrWriteMode::CreateNew,
                        XattrWriteMode::Replace => p::XattrWriteMode::Replace,
                    }
                    .into(),
                }),
                value,
            )
            .await
        })
    }
    fn remove_xattr(&self, path: StoragePath, name: XattrName) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            self.unit(
                p::StorageOp::RemoveXattr(p::RemoveXattr {
                    path: path.as_str().into(),
                    name: name.as_str().into(),
                }),
                vec![],
            )
            .await
        })
    }
}
