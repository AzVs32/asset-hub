//! Transport-independent host proxies with common session and capability rules.
//! Native server dispatch is provided separately by vfs-plugin-grpc-server.
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
pub use vfs_plugin_codec as convert;

use ah_vfs::application::driver_error::{DriverError, LifecycleError, LifecycleFailure};
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::*;
use ah_vfs::application::port::*;
use ah_vfs_storage::{
    AttributeRequest, AttributeResult, AttributeSupport, BoundListRequest, DirectoryPage,
    DirectorySelection, Entry, EntryName, MutationBinding, StoragePath,
};
use ah_vfs_storage::{
    CreateDirectoryRequest, LinkTarget, ListRequest, ReadLinkRequest, ReadRequest, RemoveKind,
    RemoveRequest, RenameMode, RenameRequest, StatRequest, WriteMode, WriteRequest,
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
};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

pub type TransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<p::Reply, DriverError>> + Send + 'a>>;
pub type ConnectFuture =
    Pin<Box<dyn Future<Output = Result<Arc<dyn Transport>, DriverError>> + Send>>;
/// Creates a fresh control instance while retaining compiled code or RPC channels.
pub trait TransportFactory: Send + Sync {
    fn connect(self: Arc<Self>) -> ConnectFuture;
}
/// A Storage owns an independent transport and resource generation.
pub struct OpenedStorage {
    pub transport: Arc<dyn SessionTransport>,
    pub generation: String,
    pub storage: p::Storage,
}
pub type OpenFuture<'a> =
    Pin<Box<dyn Future<Output = Result<OpenedStorage, DriverError>> + Send + 'a>>;
/// Calls associated with a single transport incarnation. A storage session
/// accepts only resource requests and cannot open another instance.
pub trait SessionTransport: Send + Sync {
    /// A later reconnect publishes a different token and cannot revive handles.
    fn disconnected(&self) -> Option<Arc<AtomicBool>> {
        None
    }
    fn call<'a>(&'a self, generation: &'a str, operation: Op, data: Vec<u8>)
    -> TransportFuture<'a>;
}

/// Control requests and the only entry point for opening an instance session.
pub trait Transport: SessionTransport {
    fn open_session(self: Arc<Self>, generation: &str, open: p::Open) -> OpenFuture<'_>;

    /// Native RPC plugins own their process resources and have no host imports.
    fn host_capabilities(&self) -> &[&str] {
        &[]
    }
}

fn connection_failure(error: &DriverError) -> bool {
    error.lifecycle_error().is_some_and(|error| {
        matches!(
            error.failure(),
            LifecycleFailure::Stale | LifecycleFailure::Unavailable | LifecycleFailure::TimedOut
        )
    }) || error.storage_error().is_some_and(|error| {
        !convert::is_stale_directory(error)
            && matches!(
                error.failure(),
                Failure::Backend(
                    BackendFailure::Stale | BackendFailure::Unavailable | BackendFailure::TimedOut
                )
            )
    })
}

struct Runtime<T: SessionTransport + ?Sized = dyn SessionTransport> {
    transport: Arc<T>,
    generation: String,
    resources: Mutex<HashSet<(p::ResourceKind, u64)>>,
    disconnected: Option<Arc<AtomicBool>>,
    executor: Option<tokio::runtime::Handle>,
    stale: AtomicBool,
}
impl<T: SessionTransport + ?Sized> Runtime<T> {
    fn new(transport: Arc<T>, generation: String) -> Arc<Self> {
        Arc::new(Self {
            disconnected: transport.disconnected(),
            executor: tokio::runtime::Handle::try_current().ok(),
            transport,
            generation,
            resources: Mutex::new(HashSet::new()),
            stale: AtomicBool::new(false),
        })
    }
    async fn call(&self, op: Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        if self.is_stale() && !matches!(op, Op::Release(_)) {
            return Err(convert::call_error(
                &op,
                Failure::Backend(BackendFailure::Stale),
                false,
                None,
            ));
        }
        if data.len() > p::MAX_DATA_BYTES {
            return Err(convert::call_error(
                &op,
                Failure::Rejected(Rejection::LimitExceeded),
                false,
                None,
            ));
        }
        let reply = self
            .transport
            .call(&self.generation, op.clone(), data)
            .await
            .inspect_err(|error| {
                if connection_failure(error) {
                    self.stale.store(true, Ordering::Release);
                }
            })?;
        p::validate_reply(&reply).map_err(|source| {
            convert::call_error(
                &op,
                Failure::Contract(ContractViolation::InvalidResponse),
                true,
                Some(Box::new(source)),
            )
        })?;
        if !reply
            .result
            .as_ref()
            .is_some_and(|value| op.accepts_reply(value))
        {
            return Err(convert::call_error(
                &op,
                Failure::Contract(ContractViolation::InvalidResponse),
                true,
                None,
            ));
        }
        Ok(reply)
    }
    fn is_stale(&self) -> bool {
        self.stale.load(Ordering::Acquire)
            || self
                .disconnected
                .as_ref()
                .is_some_and(|status| status.load(Ordering::Acquire))
    }
    async fn value(&self, op: Op) -> Result<Value, DriverError> {
        let r = self.call(op.clone(), vec![]).await?;
        if !r.data.is_empty() {
            return Err(convert::call_error(
                &op,
                Failure::Contract(ContractViolation::InvalidResponse),
                true,
                None,
            ));
        }
        r.result.ok_or_else(|| {
            convert::call_error(
                &op,
                Failure::Contract(ContractViolation::InvalidResponse),
                true,
                None,
            )
        })
    }
    async fn unit(&self, op: Op) -> Result<(), DriverError> {
        match self.value(op.clone()).await? {
            Value::Unit(p::Empty {}) => Ok(()),
            _ => Err(convert::call_error(
                &op,
                Failure::Contract(ContractViolation::InvalidResponse),
                true,
                None,
            )),
        }
    }
}
fn bad_response(operation: Operation, may_publish: bool) -> StorageError {
    convert::response_error(
        operation,
        may_publish,
        convert::storage_protocol_violation(operation),
    )
}

/// A fully described plugin. Register it normally to initialize plugin settings.
/// Transport failures retire the affected generation. Later configuration/open requests
/// recover before dispatch; failed operations and retained streams are never replayed.
pub struct PluginDriver {
    info: Arc<DriverInfo>,
    remote: Arc<RemoteInstances>,
    negotiated: p::NegotiatedProtocol,
}
impl PluginDriver {
    pub async fn connect(factory: Arc<dyn TransportFactory>) -> Result<Self, DriverError> {
        let transport = factory.clone().connect().await?;
        let description = describe(transport.as_ref()).await?;
        let negotiated = p::negotiate(&description, transport.host_capabilities())
            .map_err(|error| LifecycleError::with_source(LifecycleFailure::Unsupported, error))?;
        let info = Arc::new(convert::driver_info(&description)?);
        let runtime = Runtime::new(transport.clone(), description.generation.clone());
        Ok(Self {
            info,
            remote: Arc::new(RemoteInstances {
                factory,
                description: description.clone(),
                connection: Arc::new(tokio::sync::Mutex::new(Connection {
                    runtime,
                    settings: None,
                })),
            }),
            negotiated,
        })
    }
    pub fn negotiated_protocol(&self) -> &p::NegotiatedProtocol {
        &self.negotiated
    }

    /// Create a fresh control transport and initialize it with the original settings.
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
    p::validate_reply(&reply).map_err(convert::protocol_violation_with_source)?;
    match reply.result.ok_or_else(convert::protocol_violation)? {
        Value::Description(description) if reply.data.is_empty() => Ok(*description),
        _ => Err(convert::protocol_violation()),
    }
}
impl Driver for PluginDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        self.remote.clone()
    }
    fn initialize(&self, settings: DriverSettings) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            let mut connection = self.remote.connection.lock().await;
            if connection.settings.is_some() {
                return Err(LifecycleError::new(LifecycleFailure::Conflict).into());
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
            let mut attempt = 0;
            loop {
                let runtime = self.remote.runtime().await?;
                let request = p::Open {
                    id: context.instance_id().as_str().into(),
                    config: Some(context.config().clone().into_object().into()),
                };
                let opened = match runtime
                    .transport
                    .clone()
                    .open_session(&runtime.generation, request)
                    .await
                {
                    Ok(opened) => opened,
                    Err(error) => {
                        if runtime.is_stale()
                            || error
                                .lifecycle_error()
                                .is_some_and(|error| error.failure() == LifecycleFailure::Stale)
                        {
                            runtime.stale.store(true, Ordering::Release);
                        }
                        if attempt == 0 && connection_failure(&error) {
                            attempt += 1;
                            continue;
                        }
                        return Err(error);
                    }
                };
                let runtime = Runtime::new(opened.transport, opened.generation);
                let p::Storage {
                    handle,
                    info: Some(info),
                } = opened.storage
                else {
                    return Err(convert::protocol_violation());
                };
                if handle != 1 {
                    return Err(convert::protocol_violation());
                }
                let resource = Resource::new(runtime, p::ResourceKind::Storage, handle)?;
                let chunk_size = NonZeroUsize::new(info.chunk_size as usize)
                    .filter(|size| size.get() <= p::MAX_DATA_BYTES)
                    .ok_or_else(convert::protocol_violation)?;
                let storage = RemoteStorage {
                    resource,
                    driver_info: self.info.clone(),
                    chunk_size,
                };
                return Ok(Arc::new(storage) as Arc<dyn Storage>);
            }
        })
    }
}

struct RemoteInstances {
    factory: Arc<dyn TransportFactory>,
    description: p::Description,
    connection: Arc<tokio::sync::Mutex<Connection>>,
}
struct Connection {
    runtime: Arc<Runtime<dyn Transport>>,
    settings: Option<p::Object>,
}
impl RemoteInstances {
    async fn runtime(&self) -> Result<Arc<Runtime<dyn Transport>>, DriverError> {
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
    async fn reconnect(&self, force: bool) -> Result<Arc<Runtime<dyn Transport>>, DriverError> {
        let factory = self.factory.clone();
        let mut expected = self.description.clone();
        let connection = self.connection.clone();
        // Recovery owns initialization even if its initiating request is cancelled.
        // Concurrent callers share the published runtime and never replay their op.
        tokio::spawn(async move {
            let mut connection = connection.lock().await;
            if !force && !connection.runtime.is_stale() {
                return Ok(connection.runtime.clone());
            }
            let transport = factory.connect().await?;
            let mut description = describe(transport.as_ref()).await?;
            let generation = description.generation.clone();
            let changed = generation != connection.runtime.generation;
            if changed {
                connection.runtime.stale.store(true, Ordering::Release);
            }
            p::negotiate(&description, transport.host_capabilities()).map_err(|error| {
                LifecycleError::with_source(LifecycleFailure::Unsupported, error)
            })?;
            convert::driver_info(&description)?;
            expected.generation.clear();
            description.generation.clear();
            if expected != description {
                return Err(convert::protocol_violation());
            }
            let runtime = Runtime::new(transport, generation);
            if let Some(settings) = &connection.settings {
                runtime
                    .unit(Op::Initialize(p::Initialize {
                        settings: Some(settings.clone()),
                    }))
                    .await?;
            }
            connection.runtime.stale.store(true, Ordering::Release);
            connection.runtime = runtime.clone();
            Ok(runtime)
        })
        .await
        .map_err(|source| LifecycleError::with_source(LifecycleFailure::Unavailable, source))?
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
                    convert::response_object(Some(config))?.into(),
                    convert::response_object(Some(details))?,
                )),
                _ => Err(convert::protocol_violation()),
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
                }) => Ok(convert::response_object(Some(config))?.into()),
                _ => Err(convert::protocol_violation()),
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

struct Resource {
    runtime: Arc<Runtime>,
    kind: p::ResourceKind,
    handle: u64,
}
impl Resource {
    fn new(runtime: Arc<Runtime>, kind: p::ResourceKind, handle: u64) -> Result<Self, DriverError> {
        if runtime.is_stale() {
            return Err(LifecycleError::new(LifecycleFailure::Stale).into());
        }
        if handle == 0 {
            return Err(convert::protocol_violation());
        }
        if !runtime
            .resources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert((kind, handle))
        {
            return Err(convert::protocol_violation());
        }
        Ok(Self {
            runtime,
            kind,
            handle,
        })
    }
}
impl Drop for Resource {
    fn drop(&mut self) {
        self.runtime
            .resources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(self.kind, self.handle));
        if let Some(executor) = &self.runtime.executor {
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
    resource: Resource,
    driver_info: Arc<DriverInfo>,
    chunk_size: NonZeroUsize,
}
impl RemoteStorage {
    async fn call(&self, request: p::StorageOp, data: Vec<u8>) -> Result<p::Reply, StorageError> {
        let operation = request.storage_operation();
        self.resource
            .runtime
            .call(
                Op::Storage(p::StorageCall {
                    handle: self.resource.handle,
                    operation: Some(request),
                }),
                data,
            )
            .await
            .map_err(|error| convert::storage_error(operation, operation.may_mutate(), error))
    }
}
// When traversal cannot redirect the operand, verify the selected address
// rather than accepting an arbitrary response for this request.
impl Storage for RemoteStorage {
    fn is_stale(&self) -> bool {
        self.resource.runtime.is_stale()
    }
    fn chunk_size(&self) -> NonZeroUsize {
        self.chunk_size
    }
    fn attributes(&self) -> Option<&dyn StorageAttributes> {
        let (permissions, xattrs) = self.support();
        (permissions.can_read() || xattrs.can_read()).then_some(self)
    }

    fn mutation_binding(&self) -> MutationBinding {
        self.driver_info.capabilities().mutation_binding()
    }

    fn storage_capabilities(&self) -> Option<ah_vfs_storage::Capabilities> {
        Some(self.driver_info.capabilities())
    }

    fn begin_lookup(&self, op: Operation) -> StorageFuture<'_, Box<dyn OperationLookup>> {
        Box::pin(async move {
            let Some(purpose) = p::encode_lookup_purpose(op)
                .filter(|_| self.driver_info.capabilities().supports(op))
            else {
                return Err(StorageError::without_effects(
                    op,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            };
            let reply = self
                .resource
                .runtime
                .call(
                    Op::BeginLookup(p::LookupHandle {
                        handle: self.resource.handle,
                        purpose: purpose.into(),
                    }),
                    vec![],
                )
                .await
                .map_err(|error| convert::storage_error(op, false, error))?;
            let Some(Value::OpenedLookup(p::OpenedLookup { handle, root })) = reply.result else {
                return Err(bad_response(op, false));
            };
            let resource = Resource::new(
                self.resource.runtime.clone(),
                p::ResourceKind::Lookup,
                handle,
            )
            .map_err(|error| {
                if error
                    .lifecycle_error()
                    .is_some_and(|error| error.failure() == LifecycleFailure::Stale)
                {
                    StorageError::without_effects(
                        op,
                        Failure::Backend(BackendFailure::Stale),
                        Some(Box::new(error)),
                    )
                } else {
                    convert::storage_error(op, false, error)
                }
            })?;
            let scope = LookupScope::default();
            let root =
                decode_observation(op, &scope, root.ok_or_else(|| bad_response(op, false))?)?;
            if root.entry().entry_type() != ah_vfs_storage::EntryType::Directory {
                return Err(bad_response(op, false));
            }
            Ok(Box::new(RemoteLookup {
                operation: op,
                purpose,
                storage: self.resource.handle,
                resource,
                scope,
                root,
            }) as Box<dyn OperationLookup>)
        })
    }

    fn resume_directory<'a>(
        &'a self,
        cursor: &'a ah_vfs_storage::DirectoryCursor,
    ) -> StorageFuture<'a, DirectorySelection> {
        Box::pin(async move {
            let reply = self
                .call(
                    p::StorageOp::ResumeDirectory(p::encode_checkpoint(cursor)),
                    vec![],
                )
                .await?;
            let Some(Value::SelectedDirectory(selection)) = reply.result else {
                return Err(bad_response(Operation::List, false));
            };
            let selection =
                p::decode_selection(selection).map_err(|_| bad_response(Operation::List, false))?;
            if &selection != cursor.selection() {
                return Err(bad_response(Operation::List, false));
            }
            Ok(selection)
        })
    }

    fn list_page<'a>(&'a self, bound: &'a BoundListRequest) -> StorageFuture<'a, DirectoryPage> {
        Box::pin(async move {
            if !self.driver_info.capabilities().list() {
                return Err(StorageError::without_effects(
                    Operation::List,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            }
            let reply = self
                .call(p::StorageOp::List(p::encode_list(bound)), vec![])
                .await?;
            let Some(Value::Page(page)) = reply.result else {
                return Err(bad_response(Operation::List, false));
            };
            p::decode_page(bound, page, &reply.data).map_err(|reason| {
                StorageError::without_effects(Operation::List, Failure::Contract(reason), None)
            })
        })
    }
}
fn closed(operation: Operation) -> StorageError {
    StorageError::without_effects(operation, Failure::Backend(BackendFailure::Closed), None)
}
struct RemoteLookup {
    operation: Operation,
    purpose: &'static str,
    storage: u64,
    resource: Resource,
    scope: LookupScope,
    root: ObservedEntry,
}

fn decode_observation(
    op: Operation,
    scope: &LookupScope,
    observed: p::ObservedEntry,
) -> Result<ObservedEntry, StorageError> {
    let entry = observed.entry.ok_or_else(|| bad_response(op, false))?;
    Ok(scope.bind(convert::decode_entry(op, entry)?, observed.id))
}

impl RemoteLookup {
    fn id(&self, observation: &ObservedEntry) -> Result<u64, StorageError> {
        self.scope.get::<u64>(observation).copied().ok_or_else(|| {
            StorageError::without_effects(
                self.operation,
                Failure::Backend(BackendFailure::Stale),
                None,
            )
        })
    }

    async fn call(&self, op: Op) -> Result<p::Reply, StorageError> {
        self.resource
            .runtime
            .call(op, vec![])
            .await
            .map_err(|error| convert::storage_error(self.operation, false, error))
    }
    fn invalid(&self) -> StorageError {
        StorageError::without_effects(self.operation, Failure::InvalidBinding, None)
    }

    fn wire_slot(
        &self,
        slot: EntrySlot<'_>,
        path: &StoragePath,
    ) -> Result<p::BoundSlot, StorageError> {
        slot.validate(self.operation, path)?;
        Ok(p::BoundSlot {
            slot: Some(p::EntrySlot {
                parent: self.id(slot.parent)?,
                name: slot.name.as_str().into(),
                path: path.as_str().into(),
            }),
            entry: slot.entry.map(|entry| self.id(entry)).transpose()?,
        })
    }

    fn wire_target(
        &self,
        slot: Option<EntrySlot<'_>>,
        entry: &ObservedEntry,
        path: &StoragePath,
    ) -> Result<p::BoundTarget, StorageError> {
        let id = self.id(entry)?;
        if entry.entry().path() != path || slot.is_none() != path.is_root() {
            return Err(self.invalid());
        }
        let slot = slot.map(|slot| self.wire_slot(slot, path)).transpose()?;
        if slot.as_ref().is_some_and(|slot| slot.entry != Some(id)) {
            return Err(self.invalid());
        }
        Ok(p::BoundTarget {
            slot: slot.and_then(|slot| slot.slot),
            entry: id,
        })
    }

    async fn execute(&self, command: p::BoundOp) -> Result<Value, StorageError> {
        if self.operation != command.storage_operation() {
            return Err(self.invalid());
        }
        let op = Op::Execute(p::BoundCall {
            lookup: self.resource.handle,
            operation: Some(command),
        });
        let may_publish = op.may_publish();
        self.resource
            .runtime
            .value(op)
            .await
            .map_err(|error| convert::storage_error(self.operation, may_publish, error))
    }

    fn stream_resource(
        &self,
        kind: p::ResourceKind,
        handle: u64,
    ) -> Result<Resource, StorageError> {
        Resource::new(self.resource.runtime.clone(), kind, handle)
            .map_err(|error| convert::storage_error(self.operation, false, error))
    }

    async fn read(
        &self,
        link: &ObservedEntry,
        final_slot: Option<p::EntrySlot>,
    ) -> Result<LinkTarget, StorageError> {
        let reply = self
            .call(Op::ReadObservedLink(p::ReadObservedLink {
                lookup: self.resource.handle,
                entry: self.id(link)?,
                final_slot,
                purpose: self.purpose.into(),
            }))
            .await?;
        match reply.result {
            Some(Value::Bytes(_)) => Ok(LinkTarget::new(reply.data)),
            _ => Err(bad_response(self.operation, false)),
        }
    }
}

impl OperationLookup for RemoteLookup {
    fn operation(&self) -> Operation {
        self.operation
    }

    fn root(&mut self) -> StorageFuture<'_, ObservedEntry> {
        Box::pin(async move { Ok(self.root.clone()) })
    }

    fn observe<'a>(
        &'a mut self,
        parent: &'a ObservedEntry,
        name: &'a EntryName,
    ) -> StorageFuture<'a, ObservedEntry> {
        Box::pin(async move {
            let parent = self.id(parent)?;
            let reply = self
                .call(Op::ObserveChild(p::ObserveChild {
                    lookup: self.resource.handle,
                    parent,
                    name: name.as_str().into(),
                    purpose: self.purpose.into(),
                }))
                .await?;
            match reply.result {
                Some(Value::ObservedEntry(observed)) => {
                    decode_observation(self.operation, &self.scope, observed)
                }
                _ => Err(bad_response(self.operation, false)),
            }
        })
    }

    fn read_link<'a>(&'a self, link: &'a ObservedEntry) -> StorageFuture<'a, LinkTarget> {
        Box::pin(self.read(link, None))
    }

    fn execute_read_link<'a>(
        &'a self,
        parent: &'a ObservedEntry,
        name: &'a EntryName,
        link: &'a ObservedEntry,
        request: ReadLinkRequest,
    ) -> StorageFuture<'a, LinkTarget> {
        Box::pin(async move {
            if self.operation != Operation::ReadLink {
                return Err(StorageError::without_effects(
                    Operation::ReadLink,
                    Failure::InvalidBinding,
                    None,
                ));
            }
            let slot = p::EntrySlot {
                parent: self.id(parent)?,
                name: name.as_str().into(),
                path: request.path().as_str().into(),
            };
            self.read(link, Some(slot)).await
        })
    }

    fn execute_stat<'a>(
        &'a self,
        slot: Option<EntrySlot<'a>>,
        entry: &'a ObservedEntry,
        request: StatRequest,
    ) -> StorageFuture<'a, Entry> {
        Box::pin(async move {
            let op = Operation::Stat;
            let target = self.wire_target(slot, entry, request.path())?;
            let Value::Entry(wire) = self.execute(p::BoundOp::Stat(target)).await? else {
                return Err(bad_response(op, false));
            };
            let result = convert::decode_entry(op, wire)?;
            if result.path() != request.path() || result.entry_type() != entry.entry().entry_type()
            {
                return Err(bad_response(op, false));
            }
            Ok(result)
        })
    }

    fn execute_read<'a>(
        &'a self,
        slot: EntrySlot<'a>,
        request: ReadRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            let target = self.wire_slot(slot, request.path())?;
            let wire = p::BoundRead {
                target: Some(target),
                offset: request.range().offset(),
                length: request.range().length(),
            };
            let Value::Reader(p::Handle { handle }) = self.execute(p::BoundOp::Read(wire)).await?
            else {
                return Err(bad_response(Operation::Read, false));
            };
            Ok(Box::new(RemoteReader {
                resource: Some(self.stream_resource(p::ResourceKind::Reader, handle)?),
                eof: false,
                remaining: request.range().length(),
            }) as Box<dyn StorageFileReader>)
        })
    }

    fn execute_write<'a>(
        &'a self,
        slot: EntrySlot<'a>,
        request: WriteRequest,
    ) -> StorageFuture<'a, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            let target = self.wire_slot(slot, request.path())?;
            let mode = match request.mode() {
                WriteMode::CreateNew => p::WriteMode::CreateNew,
                WriteMode::Replace => p::WriteMode::Replace,
            };
            let publication = match request.publication() {
                ah_vfs_storage::Publication::Staged => p::Publication::Staged,
                ah_vfs_storage::Publication::Atomic => p::Publication::Atomic,
            };
            let wire = p::BoundWrite {
                target: Some(target),
                mode: mode.into(),
                publication: publication.into(),
            };
            let Value::Writer(p::Handle { handle }) = self.execute(p::BoundOp::Write(wire)).await?
            else {
                return Err(bad_response(Operation::Write, false));
            };
            Ok(Box::new(RemoteWriter {
                resource: Some(self.stream_resource(p::ResourceKind::Writer, handle)?),
                path: request.path().clone(),
            }) as Box<dyn StorageFileWriter>)
        })
    }

    fn execute_create_directory<'a>(
        &'a mut self,
        slot: EntrySlot<'a>,
        request: CreateDirectoryRequest,
    ) -> StorageFuture<'a, ObservedEntry> {
        Box::pin(async move {
            let op = Operation::CreateDirectory;
            let target = self.wire_slot(slot, request.path())?;
            let Value::ObservedEntry(wire) =
                self.execute(p::BoundOp::CreateDirectory(target)).await?
            else {
                return Err(bad_response(op, true));
            };
            decode_observation(op, &self.scope, wire).map_err(|error| {
                StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
            })
        })
    }

    fn execute_remove<'a>(
        &'a self,
        slot: EntrySlot<'a>,
        request: RemoveRequest,
    ) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let target = self.wire_slot(slot, request.path())?;
            let kind = match request.kind() {
                RemoveKind::NonDirectory => p::remove::Kind::NonDirectory,
                RemoveKind::EmptyDirectory => p::remove::Kind::EmptyDirectory,
            };
            let wire = p::BoundRemove {
                target: Some(target),
                kind: kind.into(),
            };
            match self.execute(p::BoundOp::Remove(wire)).await? {
                Value::Unit(_) => Ok(()),
                _ => Err(bad_response(Operation::Remove, true)),
            }
        })
    }

    fn execute_rename<'a>(
        &'a self,
        source: EntrySlot<'a>,
        destination: EntrySlot<'a>,
        request: RenameRequest,
    ) -> StorageFuture<'a, Entry> {
        Box::pin(async move {
            let op = Operation::Rename;
            let mode = match request.mode() {
                RenameMode::NoReplace => p::rename::Mode::NoReplace,
                RenameMode::Replace => p::rename::Mode::Replace,
            };
            let expected = source
                .entry
                .ok_or_else(|| self.invalid())?
                .entry()
                .entry_type();
            let wire = p::BoundRename {
                source: Some(self.wire_slot(source, request.source())?),
                destination: Some(self.wire_slot(destination, request.destination())?),
                mode: mode.into(),
            };
            let Value::Entry(entry) = self.execute(p::BoundOp::Rename(wire)).await? else {
                return Err(bad_response(op, true));
            };
            let entry = convert::decode_entry(op, entry).map_err(|error| {
                StorageError::after_dispatch(op, error.failure(), Some(Box::new(error)))
            })?;
            if entry.path() != request.destination() || entry.entry_type() != expected {
                return Err(bad_response(op, true));
            }
            Ok(entry)
        })
    }

    fn select_directory<'a>(
        &'a self,
        slot: Option<EntrySlot<'a>>,
        entry: &'a ObservedEntry,
        request: ListRequest,
    ) -> StorageFuture<'a, DirectorySelection> {
        Box::pin(async move {
            let op = Operation::List;
            if request.after().is_some() {
                return Err(self.invalid());
            }
            let target = self.wire_target(slot, entry, request.directory())?;
            let wire = p::BoundList {
                target: Some(target),
                size: request.size().get() as u32,
            };
            let Value::SelectedDirectory(selection) =
                self.execute(p::BoundOp::SelectDirectory(wire)).await?
            else {
                return Err(bad_response(op, false));
            };
            let selection = p::decode_selection(selection).map_err(|_| bad_response(op, false))?;
            if selection.address() != request.directory() {
                return Err(bad_response(op, false));
            }
            Ok(selection)
        })
    }

    fn execute_attribute<'a>(
        &'a self,
        slot: Option<(&'a ObservedEntry, &'a EntryName)>,
        entry: &'a ObservedEntry,
        request: AttributeRequest,
    ) -> StorageFuture<'a, AttributeResult> {
        Box::pin(async move {
            let operation = request.operation();
            if operation != self.operation {
                return Err(StorageError::without_effects(
                    operation,
                    Failure::InvalidBinding,
                    None,
                ));
            }
            let slot = if let Some((parent, name)) = slot {
                Some(p::EntrySlot {
                    parent: self.id(parent)?,
                    name: name.as_str().into(),
                    path: request.path().as_str().into(),
                })
            } else {
                None
            };
            let target = p::BoundAttribute {
                lookup: self.resource.handle,
                entry: self.id(entry)?,
                slot,
            };
            let expected = request.action().expectation();
            let (wire, data) = p::encode_attribute(request, target).map_err(|error| {
                let reason = match error {
                    p::AttributeValueError::Limit => Failure::Rejected(Rejection::LimitExceeded),
                    p::AttributeValueError::Target => Failure::InvalidBinding,
                    _ => Failure::Rejected(Rejection::Unsupported),
                };
                StorageError::without_effects(operation, reason, Some(Box::new(error)))
            })?;
            let reply = self
                .resource
                .runtime
                .call(
                    Op::Storage(p::StorageCall {
                        handle: self.storage,
                        operation: Some(wire),
                    }),
                    data,
                )
                .await
                .map_err(|error| {
                    let error = convert::storage_error(operation, operation.may_mutate(), error);
                    if error.failure() == Failure::Contract(ContractViolation::InvalidResponse) {
                        let failure = Failure::Contract(ContractViolation::InvalidAttributes);
                        let wrap = if error.outcome() == ah_vfs_storage::MutationOutcome::Unknown {
                            StorageError::after_dispatch
                        } else {
                            StorageError::without_effects
                        };
                        wrap(operation, failure, Some(Box::new(error)))
                    } else {
                        error
                    }
                })?;
            p::decode_attribute_result(expected, reply).map_err(|cause| {
                StorageError::after_dispatch(
                    operation,
                    Failure::Contract(ContractViolation::InvalidAttributes),
                    Some(Box::new(cause)),
                )
            })
        })
    }

    fn verify(&self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let reply = self
                .call(Op::VerifyLookup(p::LookupHandle {
                    handle: self.resource.handle,
                    purpose: self.purpose.into(),
                }))
                .await?;
            match reply.result {
                Some(Value::Unit(_)) => Ok(()),
                _ => Err(bad_response(self.operation, false)),
            }
        })
    }
}

struct RemoteReader {
    resource: Option<Resource>,
    eof: bool,
    remaining: Option<u64>,
}
impl StorageFileReader for RemoteReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            if self.eof {
                return Ok(None);
            }
            let resource = self
                .resource
                .take()
                .ok_or_else(|| closed(Operation::Read))?;
            let reply = resource
                .runtime
                .call(
                    Op::ReaderNext(p::Handle {
                        handle: resource.handle,
                    }),
                    vec![],
                )
                .await
                .map_err(|error| convert::storage_error(Operation::Read, false, error))?;
            match reply
                .result
                .ok_or_else(|| bad_response(Operation::Read, false))?
            {
                Value::Chunk(p::Chunk { eof: true }) if reply.data.is_empty() => {
                    self.eof = true;
                    Ok(None)
                }
                Value::Chunk(p::Chunk { eof: false }) if !reply.data.is_empty() => {
                    if let Some(remaining) = self.remaining.as_mut() {
                        *remaining = remaining
                            .checked_sub(reply.data.len() as u64)
                            .ok_or_else(|| bad_response(Operation::Read, false))?;
                    }
                    self.resource = Some(resource);
                    Ok(Some(reply.data))
                }
                _ => Err(bad_response(Operation::Read, false)),
            }
        })
    }
}
struct RemoteWriter {
    resource: Option<Resource>,
    path: StoragePath,
}
impl StorageFileWriter for RemoteWriter {
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let resource = self
                .resource
                .take()
                .ok_or_else(|| closed(Operation::Write))?;
            for chunk in data.chunks(p::MAX_DATA_BYTES) {
                let reply = resource
                    .runtime
                    .call(
                        Op::WriterChunk(p::Handle {
                            handle: resource.handle,
                        }),
                        chunk.to_vec(),
                    )
                    .await
                    .map_err(|error| convert::storage_error(Operation::Write, false, error))?;
                if !matches!(
                    reply
                        .result
                        .ok_or_else(|| bad_response(Operation::Write, false))?,
                    Value::Unit(p::Empty {})
                ) || !reply.data.is_empty()
                {
                    return Err(bad_response(Operation::Write, false));
                }
            }
            self.resource = Some(resource);
            Ok(())
        })
    }
    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let resource = self
                .resource
                .take()
                .ok_or_else(|| closed(Operation::Write))?;
            let reply = resource
                .runtime
                .value(Op::Commit(p::Handle {
                    handle: resource.handle,
                }))
                .await
                .map_err(|error| convert::storage_error(Operation::Write, true, error))?;
            match reply {
                Value::Entry(entry) => {
                    let entry =
                        convert::decode_entry(Operation::Write, entry).map_err(|source| {
                            convert::response_error(Operation::Write, true, source)
                        })?;
                    if entry.path() != &self.path
                        || !matches!(entry.entry_type(), ah_vfs_storage::EntryType::File { .. })
                    {
                        return Err(bad_response(Operation::Write, true));
                    }
                    Ok(entry)
                }
                _ => Err(bad_response(Operation::Write, true)),
            }
        })
    }
    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let resource = self
                .resource
                .take()
                .ok_or_else(|| closed(Operation::Write))?;
            resource
                .runtime
                .unit(Op::Abort(p::Handle {
                    handle: resource.handle,
                }))
                .await
                .map_err(|error| convert::storage_error(Operation::Write, false, error))
        })
    }
}

impl StorageAttributes for RemoteStorage {
    fn support(&self) -> (AttributeSupport, AttributeSupport) {
        let caps = self.driver_info.capabilities();
        (caps.permissions(), caps.xattrs())
    }
}
