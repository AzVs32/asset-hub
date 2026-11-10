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
    BoundListRequest, DirectoryPage, DirectorySelection, Entry, ListRequest, Permissions,
    StoragePath, XattrName, XattrWriteMode,
};
use ah_vfs_storage::{LinkTarget, ReadLinkRequest, ReadRequest, StatRequest, WriteRequest};
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
    pub transport: Arc<dyn Transport>,
    pub generation: String,
    pub storage: p::Storage,
}
pub type OpenFuture<'a> =
    Pin<Box<dyn Future<Output = Result<OpenedStorage, DriverError>> + Send + 'a>>;
pub trait Transport: Send + Sync {
    /// A permanent status token for this connection incarnation. A later
    /// reconnect publishes another token and can never revive existing handles.
    fn disconnected(&self) -> Option<Arc<AtomicBool>> {
        None
    }
    fn open_session<'a>(&'a self, _generation: &'a str, _open: p::Open) -> Option<OpenFuture<'a>> {
        None
    }

    /// Services this execution environment supplies to guest code. Native RPC
    /// plugins own their process resources and do not import host services.
    fn host_capabilities(&self) -> &[&str] {
        &[]
    }
    fn call<'a>(&'a self, generation: &'a str, operation: Op, data: Vec<u8>)
    -> TransportFuture<'a>;
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

struct Runtime {
    transport: Arc<dyn Transport>,
    generation: String,
    resources: Mutex<HashSet<(p::ResourceKind, u64)>>,
    disconnected: Option<Arc<AtomicBool>>,
    executor: Option<tokio::runtime::Handle>,
    stale: AtomicBool,
}
impl Runtime {
    fn new(transport: Arc<dyn Transport>, generation: String) -> Arc<Self> {
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
                let (runtime, storage) = if let Some(open) = runtime
                    .transport
                    .open_session(&runtime.generation, request.clone())
                {
                    let opened = match open.await {
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
                    (
                        Runtime::new(opened.transport, opened.generation),
                        opened.storage,
                    )
                } else {
                    let Value::Storage(storage) = runtime.value(Op::Open(request)).await? else {
                        return Err(convert::protocol_violation());
                    };
                    (runtime, storage)
                };
                let p::Storage {
                    handle,
                    info: Some(info),
                } = storage
                else {
                    return Err(convert::protocol_violation());
                };
                let resource = Resource::new(runtime, p::ResourceKind::Storage, handle)?;
                if info.chunk_size == 0 || info.chunk_size as usize > p::MAX_DATA_BYTES {
                    return Err(convert::protocol_violation());
                }
                let storage = RemoteStorage {
                    resource,
                    driver_info: self.info.clone(),
                    chunk_size: convert::limit(info.chunk_size).map_err(DriverError::from)?,
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
    async fn unit(&self, request: p::StorageOp, data: Vec<u8>) -> Result<(), StorageError> {
        let operation = request.storage_operation();
        let r = self.call(request, data).await?;
        match r
            .result
            .ok_or_else(|| bad_response(operation, operation.may_mutate()))?
        {
            Value::Unit(p::Empty {}) if r.data.is_empty() => Ok(()),
            _ => Err(bad_response(operation, operation.may_mutate())),
        }
    }
}
// When traversal cannot redirect the operand, verify the selected address
// rather than accepting an arbitrary response for this request.
fn expected_lookup_address(
    lookup: ah_vfs_storage::PathLookup<'_>,
    context: &LookupContext,
) -> Option<StoragePath> {
    use ah_vfs_storage::{IntermediateSymlinks, LookupStart, PathComponent, SymlinkAction};
    let operand = lookup.path();
    let components = operand.components();
    if !components.is_empty() && lookup.final_symlink() == SymlinkAction::FollowWithinInstance
        || components.len() > 1 && context.intermediate_symlinks != IntermediateSymlinks::Reject
    {
        return None;
    }
    let mut path = match operand.start() {
        LookupStart::InstanceRoot => StoragePath::root(),
        LookupStart::BaseDirectory => context.working_directory.clone(),
    };
    for component in components {
        match component {
            PathComponent::Name(name) => path = path.join(name).ok()?,
            PathComponent::CurrentDirectory => {}
            PathComponent::ParentDirectory => {
                path = path.parent().unwrap_or_else(StoragePath::root)
            }
        }
    }
    Some(path)
}
impl Storage for RemoteStorage {
    fn is_stale(&self) -> bool {
        self.resource.runtime.is_stale()
    }
    fn chunk_size(&self) -> NonZeroUsize {
        self.chunk_size
    }
    fn permissions(&self) -> Option<&dyn StoragePermissions> {
        self.driver_info
            .capabilities()
            .permissions()
            .can_read()
            .then_some(self)
    }
    fn xattrs(&self) -> Option<&dyn StorageXattrs> {
        self.driver_info
            .capabilities()
            .xattrs()
            .can_read()
            .then_some(self)
    }
    fn stat_entry(&self, request: StatRequest, context: LookupContext) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let op = Operation::Stat;
            if !self.driver_info.capabilities().stat() {
                return Err(StorageError::without_effects(
                    op,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            }
            let reply = self
                .call(
                    p::StorageOp::Stat(p::encode_stat(
                        &request,
                        &context.working_directory,
                        context.intermediate_symlinks,
                    )),
                    vec![],
                )
                .await?;
            match reply.result {
                Some(Value::Entry(entry)) if reply.data.is_empty() => {
                    let entry = convert::decode_entry(op, entry)
                        .map_err(|source| convert::response_error(op, false, source))?;
                    let lookup = request.lookup();
                    if lookup
                        .check_existing_target(entry.path(), entry.entry_type())
                        .is_err()
                        || (lookup.final_symlink()
                            == ah_vfs_storage::SymlinkAction::FollowWithinInstance
                            && entry.entry_type() == ah_vfs_storage::EntryType::Symlink)
                    {
                        return Err(bad_response(op, false));
                    }
                    if let Some(expected) = expected_lookup_address(request.lookup(), &context)
                        && entry.path() != &expected
                    {
                        return Err(bad_response(op, false));
                    }
                    Ok(entry)
                }
                _ => Err(bad_response(op, false)),
            }
        })
    }

    fn read_link(
        &self,
        request: ReadLinkRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, LinkTarget> {
        Box::pin(async move {
            let op = Operation::ReadLink;
            if !self.driver_info.capabilities().read_link() {
                return Err(StorageError::without_effects(
                    op,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            }
            let lookup = p::encode_lookup(
                request.path(),
                &context.working_directory,
                context.intermediate_symlinks,
            );
            let reply = self
                .call(
                    p::StorageOp::ReadLink(p::ReadLink {
                        lookup: Some(lookup),
                    }),
                    vec![],
                )
                .await?;
            match reply.result {
                Some(Value::Bytes(_)) if reply.data.len() <= p::MAX_DATA_BYTES => {
                    Ok(LinkTarget::new(reply.data))
                }
                _ => Err(bad_response(op, false)),
            }
        })
    }
    fn create_dir(
        &self,
        request: ah_vfs_storage::CreateDirectoryRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let op = Operation::CreateDirectory;
            if !self.driver_info.capabilities().directories().can_create() {
                return Err(StorageError::without_effects(
                    op,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            }
            let reply = self
                .call(
                    p::StorageOp::CreateDirectory(p::encode_create_directory(
                        &request,
                        &context.working_directory,
                        context.intermediate_symlinks,
                    )),
                    vec![],
                )
                .await?;
            if !reply.data.is_empty() {
                return Err(bad_response(op, true));
            }
            let Some(Value::Entry(entry)) = reply.result else {
                return Err(bad_response(op, true));
            };
            let entry = convert::decode_entry(op, entry)
                .map_err(|error| convert::response_error(op, true, error))?;
            if entry.entry_type() != ah_vfs_storage::EntryType::Directory
                || expected_lookup_address(request.lookup(), &context)
                    .is_some_and(|path| entry.path() != &path)
            {
                return Err(bad_response(op, true));
            }
            Ok(entry)
        })
    }
    fn move_entry(
        &self,
        request: ah_vfs_storage::RenameRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            if !self
                .driver_info
                .capabilities()
                .rename()
                .supports(request.mode())
            {
                return Err(StorageError::without_effects(
                    Operation::Rename,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            }
            let op = Operation::Rename;
            let reply = self
                .call(
                    p::StorageOp::Rename(p::encode_rename(
                        &request,
                        &context.working_directory,
                        context.intermediate_symlinks,
                    )),
                    vec![],
                )
                .await?;
            let Some(Value::Entry(entry)) = reply.result else {
                return Err(bad_response(op, true));
            };
            let entry = convert::decode_entry(op, entry)
                .map_err(|error| convert::response_error(op, true, error))?;
            if !reply.data.is_empty()
                || entry.path().is_root()
                || expected_lookup_address(request.lookups().1, &context)
                    .is_some_and(|path| entry.path() != &path)
                || (request.source().requires_directory()
                    || request.destination().requires_directory())
                    && entry.entry_type() != ah_vfs_storage::EntryType::Directory
            {
                return Err(bad_response(op, true));
            }
            Ok(entry)
        })
    }
    fn remove_entry(
        &self,
        request: ah_vfs_storage::RemoveRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            if !self.driver_info.capabilities().remove() {
                return Err(StorageError::without_effects(
                    Operation::Remove,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            }
            self.unit(
                p::StorageOp::Remove(p::encode_remove(
                    &request,
                    &context.working_directory,
                    context.intermediate_symlinks,
                )),
                vec![],
            )
            .await
        })
    }
    fn select_directory<'a>(
        &'a self,
        request: &'a ListRequest,
        context: LookupContext,
    ) -> StorageFuture<'a, DirectorySelection> {
        Box::pin(async move {
            if !self.driver_info.capabilities().list() {
                return Err(StorageError::without_effects(
                    Operation::List,
                    Failure::Rejected(Rejection::Unsupported),
                    None,
                ));
            }
            let reply = self
                .call(
                    p::StorageOp::SelectDirectory(p::encode_select_directory(
                        request,
                        &context.working_directory,
                        context.intermediate_symlinks,
                    )),
                    vec![],
                )
                .await?;
            let Some(Value::SelectedDirectory(selection)) = reply.result else {
                return Err(bad_response(Operation::List, false));
            };
            let selection = p::decode_selection(selection).map_err(|source| {
                convert::storage_protocol_violation_with_source(Operation::List, source)
            })?;
            if request
                .lookup()
                .and_then(|lookup| expected_lookup_address(lookup, &context))
                .is_some_and(|address| &address != selection.address())
            {
                return Err(bad_response(Operation::List, false));
            }
            request
                .bind_selection(selection.clone())
                .map_err(|reason| {
                    StorageError::without_effects(Operation::List, Failure::Backend(reason), None)
                })?;
            Ok(selection)
        })
    }

    fn list_page<'a>(
        &'a self,
        bound: &'a BoundListRequest<'_>,
    ) -> StorageFuture<'a, DirectoryPage> {
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
    fn read_file(
        &self,
        request: ReadRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            self.driver_info
                .capabilities()
                .require_read(&request)
                .map_err(|reason| {
                    StorageError::without_effects(Operation::Read, Failure::Rejected(reason), None)
                })?;
            let value = self
                .resource
                .runtime
                .value(Op::Read(p::encode_read(
                    self.resource.handle,
                    &request,
                    &context.working_directory,
                    context.intermediate_symlinks,
                )))
                .await
                .map_err(|error| convert::storage_error(Operation::Read, false, error))?;
            match value {
                Value::Reader(p::Handle { handle }) => Ok(Box::new(RemoteReader {
                    resource: Some(
                        Resource::new(
                            self.resource.runtime.clone(),
                            p::ResourceKind::Reader,
                            handle,
                        )
                        .map_err(|error| convert::storage_error(Operation::Read, false, error))?,
                    ),
                    eof: false,
                    remaining: request.range().length(),
                })
                    as Box<dyn StorageFileReader>),
                _ => Err(bad_response(Operation::Read, false)),
            }
        })
    }
    fn write_file(
        &self,
        request: WriteRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            self.driver_info
                .capabilities()
                .require_write(&request)
                .map_err(|reason| {
                    StorageError::without_effects(Operation::Write, Failure::Rejected(reason), None)
                })?;
            let value = self
                .resource
                .runtime
                .value(Op::Write(p::encode_write(
                    self.resource.handle,
                    &request,
                    &context.working_directory,
                    context.intermediate_symlinks,
                )))
                .await
                .map_err(|error| convert::storage_error(Operation::Write, false, error))?;
            match value {
                Value::Writer(p::Handle { handle }) => Ok(Box::new(RemoteWriter {
                    resource: Some(
                        Resource::new(
                            self.resource.runtime.clone(),
                            p::ResourceKind::Writer,
                            handle,
                        )
                        .map_err(|error| convert::storage_error(Operation::Write, false, error))?,
                    ),
                    path: expected_lookup_address(request.lookup(), &context),
                })
                    as Box<dyn StorageFileWriter>),
                _ => Err(bad_response(Operation::Write, false)),
            }
        })
    }
}
fn closed(operation: Operation) -> StorageError {
    StorageError::without_effects(operation, Failure::Backend(BackendFailure::Closed), None)
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
    path: Option<StoragePath>,
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
                    if self.path.as_ref().is_some_and(|path| entry.path() != path)
                        || entry.path().is_root()
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

impl StoragePermissions for RemoteStorage {
    fn get_permissions(&self, path: StoragePath) -> StorageFuture<'_, Permissions> {
        Box::pin(async move {
            let r = self
                .call(
                    p::StorageOp::GetPermissions(p::Path {
                        path: path.as_str().into(),
                    }),
                    vec![],
                )
                .await?;
            match r
                .result
                .ok_or_else(|| bad_response(Operation::GetPermissions, false))?
            {
                Value::Permissions(p::Permissions { mode }) if r.data.is_empty() => {
                    Permissions::new(mode).map_err(|error| {
                        StorageError::without_effects(
                            Operation::GetPermissions,
                            Failure::Contract(ContractViolation::InvalidResponse),
                            Some(Box::new(error)),
                        )
                    })
                }
                _ => Err(bad_response(Operation::GetPermissions, false)),
            }
        })
    }
    fn set_permissions(
        &self,
        path: StoragePath,
        permissions: Permissions,
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
            let max_bytes = u32::try_from(max_bytes.get()).map_err(|source| {
                StorageError::without_effects(
                    Operation::ListXattrs,
                    Failure::Rejected(Rejection::LimitExceeded),
                    Some(Box::new(source)),
                )
            })?;
            let r = self
                .call(
                    p::StorageOp::ListXattrs(p::ListXattrs {
                        path: path.as_str().into(),
                        max_bytes,
                    }),
                    vec![],
                )
                .await?;
            match r
                .result
                .ok_or_else(|| bad_response(Operation::ListXattrs, false))?
            {
                Value::Names(p::Names { names }) if r.data.is_empty() => {
                    if names.windows(2).any(|v| v[0] >= v[1])
                        || names.iter().map(|n| n.len() + 1).sum::<usize>() > max_bytes as usize
                    {
                        return Err(bad_response(Operation::ListXattrs, false));
                    }
                    names
                        .into_iter()
                        .map(|n| {
                            XattrName::parse(&n).map_err(|error| {
                                StorageError::without_effects(
                                    Operation::ListXattrs,
                                    Failure::Contract(ContractViolation::InvalidResponse),
                                    Some(Box::new(error)),
                                )
                            })
                        })
                        .collect()
                }
                _ => Err(bad_response(Operation::ListXattrs, false)),
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
            let max_bytes = u32::try_from(max_bytes.get()).map_err(|source| {
                StorageError::without_effects(
                    Operation::GetXattr,
                    Failure::Rejected(Rejection::LimitExceeded),
                    Some(Box::new(source)),
                )
            })?;
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
                r.result
                    .ok_or_else(|| bad_response(Operation::GetXattr, false))?,
                Value::Bytes(p::Empty {})
            ) && r.data.len() <= max_bytes as usize
            {
                Ok(r.data)
            } else {
                Err(bad_response(Operation::GetXattr, false))
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
