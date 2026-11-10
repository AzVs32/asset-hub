use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::*;
use ah_vfs::application::port::*;
use ah_vfs::domain::InstanceId;
use ah_vfs::domain::storage::{
    StorageReadFileRequest, StorageRemoveEntryRequest, StorageWriteFileRequest,
};
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
use ah_vfs_storage::{Entry, EntryType, StoragePath};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use vfs_plugin_codec as c;
use vfs_plugin_grpc::GrpcTransport;
use vfs_plugin_grpc_server::{
    GrpcService, PluginService, ServerLimits,
    dispatch::{Dispatcher, DriverFactory},
    serve_with_limits, service, wire,
};
use vfs_plugin_host::{OpenFuture, OpenedStorage, PluginDriver, Transport, TransportFuture};
use vfs_plugin_protocol::{self as p, Operation as Op};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Default)]
struct Faults {
    invalid_stat: AtomicBool,
    corrupt_data: AtomicBool,
    panic_remove: AtomicBool,
    fail_factory: AtomicBool,
    constructions: AtomicUsize,
    initializations: AtomicUsize,
    removes: AtomicUsize,
    suspend_initialize: AtomicBool,
    initializing: tokio::sync::Notify,
    resume: tokio::sync::Notify,
    suspend_read: AtomicBool,
    reading: tokio::sync::Notify,
    chunks: AtomicUsize,
    writing: tokio::sync::Notify,
    storage_drops: AtomicUsize,
    reader_drops: AtomicUsize,
    writer_drops: AtomicUsize,
    commits: AtomicUsize,
}
struct Backend {
    info: DriverInfo,
    marker: usize,
    faults: Arc<Faults>,
}
impl Backend {
    fn new(marker: usize, faults: Arc<Faults>) -> Result<Self> {
        let description = p::Description {
            major: p::PROTOCOL_MAJOR,
            minor: p::PROTOCOL_MINOR,
            required_features: vec![],
            features: p::PROTOCOL_FEATURES.iter().map(|v| (*v).into()).collect(),
            required_host_capabilities: vec![],
            instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
            generation: "fixture".into(),
            key: "native.recovery".into(),
            name: "Recovery fixture".into(),
            version: "1.0.0".into(),
            child_mounts: true,
            capabilities: [
                p::Capability::Stat,
                p::Capability::RangeRead,
                p::Capability::WriteNew,
                p::Capability::Remove,
            ]
            .into_iter()
            .map(Into::into)
            .collect(),

            settings_schema: None,
            instance_schema: None,
            config_version: 1,
        };
        Ok(Self {
            info: c::driver_info(&description)?,
            marker,
            faults,
        })
    }
}
struct Provisioner;
impl InstanceProvisioner for Provisioner {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
        Box::pin(async move { Ok(PreparedConfig::new(config, Default::default())) })
    }
    fn migrate_config(&self, _: u32, config: InstanceConfig) -> DriverFuture<'_, InstanceConfig> {
        Box::pin(async move { Ok(config) })
    }
    fn test_connection<'a>(&'a self, _: &'a InstanceConfig) -> DriverFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
impl Driver for Backend {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        Arc::new(Provisioner)
    }
    fn initialize(&self, _: DriverSettings) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            self.faults.initializations.fetch_add(1, Ordering::SeqCst);
            if self.faults.suspend_initialize.load(Ordering::SeqCst) {
                self.faults.initializing.notify_one();
                self.faults.resume.notified().await;
            }
            Ok(())
        })
    }
    fn open(&self, _: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        Box::pin(async move {
            Ok(Arc::new(BackendStorage {
                marker: self.marker,
                faults: self.faults.clone(),
            }) as Arc<dyn Storage>)
        })
    }
}
struct BackendStorage {
    marker: usize,
    faults: Arc<Faults>,
}
impl Drop for BackendStorage {
    fn drop(&mut self) {
        self.faults.storage_drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl Storage for BackendStorage {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
    fn stat_entry(
        &self,
        request: ah_vfs_storage::StatRequest,
        _: ah_vfs::application::port::LookupContext,
    ) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            let operand = request.path().to_string();
            let path = StoragePath::parse(operand.trim_start_matches('/')).map_err(|cause| {
                StorageError::without_effects(
                    Operation::Stat,
                    Failure::Contract(ContractViolation::InvalidEntry),
                    Some(Box::new(cause)),
                )
            })?;
            if self.faults.invalid_stat.load(Ordering::SeqCst) {
                return Err(StorageError::without_effects(
                    Operation::Stat,
                    Failure::Contract(ContractViolation::InvalidResponse),
                    None,
                ));
            }
            if self.faults.corrupt_data.load(Ordering::SeqCst) {
                return Err(StorageError::without_effects(
                    Operation::Stat,
                    Failure::Backend(BackendFailure::CorruptData),
                    None,
                ));
            }
            Entry::new(path, EntryType::file(Some(self.marker as u64)), None).map_err(|cause| {
                StorageError::without_effects(
                    Operation::Stat,
                    Failure::Contract(ContractViolation::InvalidResponse),
                    Some(Box::new(cause)),
                )
            })
        })
    }
    fn read_file(
        &self,
        _: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            Ok(Box::new(Reader {
                data: Some(vec![self.marker as u8]),
                faults: self.faults.clone(),
            }) as Box<dyn StorageFileReader>)
        })
    }
    fn write_file(
        &self,
        _: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        Box::pin(
            async move { Ok(Box::new(Writer(self.faults.clone())) as Box<dyn StorageFileWriter>) },
        )
    }
    #[allow(clippy::panic)]
    fn remove_entry(&self, _: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            self.faults.removes.fetch_add(1, Ordering::SeqCst);
            if self.faults.panic_remove.swap(false, Ordering::SeqCst) {
                panic!("injected backend panic");
            }
            Ok(())
        })
    }
}
struct Reader {
    data: Option<Vec<u8>>,
    faults: Arc<Faults>,
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.faults.reader_drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl StorageFileReader for Reader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            if self.faults.suspend_read.load(Ordering::SeqCst) {
                self.faults.reading.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(self.data.take())
        })
    }
}
struct Writer(Arc<Faults>);
impl Drop for Writer {
    fn drop(&mut self) {
        self.0.writer_drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl StorageFileWriter for Writer {
    fn write_chunk<'a>(&'a mut self, _: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            self.0.chunks.fetch_add(1, Ordering::SeqCst);
            self.0.writing.notify_one();
            Ok(())
        })
    }
    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        Box::pin(async move {
            self.0.commits.fetch_add(1, Ordering::SeqCst);
            Err(StorageError::after_dispatch(
                Operation::Write,
                Failure::Rejected(Rejection::Unsupported),
                None,
            ))
        })
    }
    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}
struct Direct(Arc<Dispatcher>);
impl vfs_plugin_host::TransportFactory for Direct {
    fn connect(self: Arc<Self>) -> vfs_plugin_host::ConnectFuture {
        Box::pin(async move { Ok(self as Arc<dyn Transport>) })
    }
}
impl Transport for Direct {
    fn open_session<'a>(&'a self, generation: &'a str, open: p::Open) -> Option<OpenFuture<'a>> {
        Some(Box::pin(async move {
            let session = self.0.open_session(generation, open).await?;
            let p::Value::Storage(storage) = session
                .opened()
                .into_value()
                .map_err(|_| c::protocol_violation())?
            else {
                return Err(c::protocol_violation());
            };
            Ok(OpenedStorage {
                generation: generation.into(),
                storage,
                transport: Arc::new(DirectSession(session)),
            })
        }))
    }

    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(self.0.call(generation, op, data))
    }
}
struct DirectSession(vfs_plugin_grpc_server::dispatch::Session);
impl Transport for DirectSession {
    fn call<'a>(&'a self, _generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(self.0.call(op, data))
    }
}
async fn open(driver: &PluginDriver) -> Result<Arc<dyn Storage>> {
    Ok(driver
        .open(OpenContext::new(
            InstanceId::parse("instance")?,
            Default::default(),
        ))
        .await?)
}
async fn stat(storage: &dyn Storage) -> Result<u64> {
    let entry = storage
        .stat_entry(
            ah_vfs_storage::StatRequest::new(
                (StoragePath::parse("file")?).to_rooted_expr(),
                ah_vfs_storage::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await?;
    match entry.entry_type() {
        EntryType::File {
            size: Some(size), ..
        } => Ok(size),
        _ => Err("expected file size".into()),
    }
}
async fn read(storage: &dyn Storage) -> Result<Box<dyn StorageFileReader>> {
    Ok(storage
        .read_file(StorageReadFileRequest::new(StoragePath::parse("file")?))
        .await?)
}

#[tokio::test]
async fn panic_recovery_uses_a_fresh_backend_and_never_replays_the_mutation() -> Result {
    let faults = Arc::new(Faults::default());
    let factory: Arc<DriverFactory> = {
        let faults = faults.clone();
        Arc::new(move || {
            let faults = faults.clone();
            Box::pin(async move {
                if faults.fail_factory.load(Ordering::SeqCst) {
                    return Err(ah_vfs::application::driver_error::LifecycleError::new(
                        ah_vfs::application::driver_error::LifecycleFailure::Unavailable,
                    )
                    .into());
                }
                let marker = faults.constructions.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(
                    Arc::new(Backend::new(marker, faults).map_err(|_| c::protocol_violation())?)
                        as Arc<dyn Driver>,
                )
            })
        })
    };
    let dispatcher = Arc::new(Dispatcher::with_factory(factory).await?);
    let original = dispatcher.generation();
    let driver = PluginDriver::connect(Arc::new(Direct(dispatcher.clone()))).await?;
    driver.initialize(Default::default()).await?;
    let old = open(&driver).await?;
    let mut reader = read(old.as_ref()).await?;
    faults.panic_remove.store(true, Ordering::SeqCst);
    let error = old
        .remove_entry(StorageRemoveEntryRequest {
            path: StoragePath::parse("file")?,
        })
        .await
        .err()
        .ok_or("panic result")?;
    assert_eq!(
        error.failure(),
        Failure::Backend(BackendFailure::Unavailable)
    );
    assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unknown);
    faults.fail_factory.store(true, Ordering::SeqCst);
    assert!(open(&driver).await.is_err());
    faults.fail_factory.store(false, Ordering::SeqCst);
    let new = open(&driver).await?;
    assert_eq!(stat(new.as_ref()).await?, 2);
    assert_ne!(dispatcher.generation(), original);
    assert_eq!(
        reader
            .next_chunk()
            .await
            .err()
            .ok_or("old reader")?
            .failure(),
        Failure::Backend(BackendFailure::Stale)
    );
    assert!(old.is_stale());
    drop(old);
    assert_eq!(stat(new.as_ref()).await?, 2);
    assert_eq!(faults.removes.load(Ordering::SeqCst), 1);
    assert_eq!(faults.constructions.load(Ordering::SeqCst), 2);
    assert_eq!(faults.initializations.load(Ordering::SeqCst), 2);
    Ok(())
}

struct ServerTask {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result>,
}
impl ServerTask {
    async fn stop(mut self) -> Result {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        (&mut self.task).await??;
        Ok(())
    }
}
impl Drop for ServerTask {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn start(
    listener: tokio::net::TcpListener,
    dispatcher: Arc<Dispatcher>,
) -> Result<ServerTask> {
    let service = service(dispatcher.clone(), "test-token")?;
    start_service(listener, service, dispatcher).await
}
async fn start_service(
    listener: tokio::net::TcpListener,
    service: GrpcService,
    dispatcher: Arc<Dispatcher>,
) -> Result<ServerTask> {
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _sessions = CloseSessions(dispatcher.clone());
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = stopped.await;
                    dispatcher.shutdown();
                },
            )
            .await?;
        Ok(())
    });
    Ok(ServerTask {
        stop: Some(stop),
        task,
    })
}

struct CloseSessions(Arc<Dispatcher>);
impl Drop for CloseSessions {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

#[tokio::test]
async fn grpc_generation_changes_do_not_rebind_streams_or_release_new_handles() -> Result {
    let faults = Arc::new(Faults::default());
    let dispatcher = Arc::new(Dispatcher::new(Arc::new(Backend::new(1, faults.clone())?))?);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = start(listener, dispatcher.clone()).await?;
    let transport = Arc::new(GrpcTransport::connect(endpoint, "test-token").await?);
    let driver = PluginDriver::connect(transport.clone()).await?;
    driver.initialize(Default::default()).await?;
    let old = open(&driver).await?;
    let mut reader = read(old.as_ref()).await?;
    assert_eq!(reader.next_chunk().await?, Some(vec![1]));
    dispatcher
        .replace_driver(Arc::new(Backend::new(2, faults)?))
        .await?;
    until(|| old.is_stale()).await?;
    let new = open(&driver).await?;
    let mut new_reader = read(new.as_ref()).await?;
    assert_eq!(new_reader.next_chunk().await?, Some(vec![2]));
    // Readers use the same local ID in independent streams. Dropping an old
    // stream cannot release resources owned by its replacement.
    assert_eq!(new_reader.next_chunk().await?, None);
    assert_eq!(
        reader
            .next_chunk()
            .await
            .err()
            .ok_or("old reader")?
            .failure(),
        Failure::Backend(BackendFailure::Stale)
    );
    assert!(old.is_stale());
    drop(server);
    Ok(())
}

#[tokio::test]
async fn grpc_server_restart_recovers_without_restarting_the_host() -> Result {
    let faults = Arc::new(Faults::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = start(
        listener,
        Arc::new(Dispatcher::new(Arc::new(Backend::new(1, faults.clone())?))?),
    )
    .await?;
    let transport =
        Arc::new(GrpcTransport::connect(format!("http://{address}"), "test-token").await?);
    let driver = PluginDriver::connect(transport).await?;
    driver.initialize(Default::default()).await?;
    let old = open(&driver).await?;
    server.stop().await?;
    until(|| old.is_stale()).await?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let server = start(
        listener,
        Arc::new(Dispatcher::new(Arc::new(Backend::new(2, faults)?))?),
    )
    .await?;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if open(&driver).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(old.is_stale());
    assert_eq!(stat(open(&driver).await?.as_ref()).await?, 2);
    drop(server);
    Ok(())
}

type Client = wire::plugin_client::PluginClient<tonic::transport::Channel>;
fn request(generation: &str, operation: Op) -> Result<tonic::Request<wire::Request>> {
    let mut request = tonic::Request::new(p::to_request(generation, &operation, vec![])?);
    request
        .metadata_mut()
        .insert("authorization", "Bearer test-token".parse()?);
    Ok(request)
}
async fn rpc(client: &mut Client, generation: &str, operation: Op) -> Result<p::Value> {
    let reply = client
        .invoke(request(generation, operation)?)
        .await?
        .into_inner();
    Ok(p::from_reply(reply)?
        .map_err(|error| c::driver_error(error, None))?
        .into_value()?)
}
async fn describe(client: &mut Client) -> Result<String> {
    match rpc(client, "", Op::Describe(p::Empty {})).await? {
        p::Value::Description(description) => Ok(description.generation),
        _ => Err("description".into()),
    }
}
async fn production(faults: Arc<Faults>, limits: ServerLimits) -> Result<(String, ServerTask)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let driver = Arc::new(Backend::new(1, faults)?);
    let task = tokio::spawn(serve_with_limits(
        listener,
        driver,
        "test-token",
        None,
        limits,
        async {
            let _ = stopped.await;
        },
    ));
    Ok((
        endpoint,
        ServerTask {
            stop: Some(stop),
            task,
        },
    ))
}

#[tokio::test]
async fn cancelled_initialization_requires_a_fresh_backend_and_acks_are_idempotent() -> Result {
    let faults = Arc::new(Faults::default());
    faults.suspend_initialize.store(true, Ordering::SeqCst);
    let original: Arc<dyn Driver> = Arc::new(Backend::new(1, faults.clone())?);
    let dispatcher = Dispatcher::new(original.clone())?;
    let old = dispatcher.generation();
    let mut call = Box::pin(dispatcher.call(
        &old,
        Op::Initialize(p::Initialize {
            settings: Some(Default::default()),
        }),
        vec![],
    ));
    std::future::poll_fn(|cx| {
        assert!(call.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(call);
    assert!(
        dispatcher
            .call("", Op::Describe(p::Empty {}), vec![])
            .await
            .is_err()
    );
    assert!(dispatcher.replace_driver(original).await.is_err());
    faults.suspend_initialize.store(false, Ordering::SeqCst);
    dispatcher
        .replace_driver(Arc::new(Backend::new(2, faults.clone())?))
        .await?;
    let new = dispatcher.generation();
    assert_ne!(new, old);
    for _ in 0..2 {
        dispatcher
            .call(
                &new,
                Op::Initialize(p::Initialize {
                    settings: Some(Default::default()),
                }),
                vec![],
            )
            .await?;
    }
    assert_eq!(faults.initializations.load(Ordering::SeqCst), 2);
    let error = dispatcher
        .call(
            &new,
            Op::Initialize(p::Initialize {
                settings: Some(p::Object::from([("different".into(), true.into())])),
            }),
            vec![],
        )
        .await
        .err()
        .ok_or("different settings")?;
    assert_eq!(
        error.lifecycle_error().ok_or("category")?.failure(),
        ah_vfs::application::driver_error::LifecycleFailure::Conflict
    );
    dispatcher
        .call(
            &new,
            Op::TestConnection(p::Config {
                config: Some(Default::default()),
            }),
            vec![],
        )
        .await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn failed_authentication_consumes_shared_rate_budget_before_decoding() -> Result {
    use wire::plugin_server::Plugin;
    let dispatcher = Arc::new(Dispatcher::new(Arc::new(Backend::new(
        1,
        Arc::new(Faults::default()),
    )?))?);
    let service = PluginService::with_limits(
        dispatcher,
        "test-token",
        ServerLimits {
            operations_per_second: 1,
            operation_burst: 1,
            ..Default::default()
        },
    )?;
    let mut bad = request("", Op::Describe(p::Empty {}))?;
    bad.metadata_mut()
        .insert("authorization", "Bearer wrong-token".parse()?);
    bad.get_mut().operation = None;
    assert_eq!(
        service
            .invoke(bad)
            .await
            .err()
            .ok_or("authentication")?
            .code(),
        tonic::Code::Unauthenticated
    );
    assert_eq!(
        service
            .clone()
            .invoke(request("", Op::Describe(p::Empty {}))?)
            .await
            .err()
            .ok_or("rate limit")?
            .code(),
        tonic::Code::ResourceExhausted
    );
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    service
        .invoke(request("", Op::Describe(p::Empty {}))?)
        .await?;
    Ok(())
}

#[tokio::test]
async fn global_concurrency_is_shared_across_connections() -> Result {
    let faults = Arc::new(Faults::default());
    faults.suspend_initialize.store(true, Ordering::SeqCst);
    let (endpoint, server) = production(
        faults.clone(),
        ServerLimits {
            max_concurrent_requests: 1,
            ..Default::default()
        },
    )
    .await?;
    let mut first = Client::connect(endpoint.clone()).await?;
    let mut second = Client::connect(endpoint).await?;
    let generation = describe(&mut first).await?;
    let initializing = tokio::spawn(async move {
        first
            .invoke(
                request(
                    &generation,
                    Op::Initialize(p::Initialize {
                        settings: Some(Default::default()),
                    }),
                )
                .map_err(|_| tonic::Status::internal("fixture"))?,
            )
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        faults.initializing.notified(),
    )
    .await?;
    assert_eq!(
        second
            .invoke(request("", Op::Describe(p::Empty {}))?)
            .await
            .err()
            .ok_or("global concurrency")?
            .code(),
        tonic::Code::ResourceExhausted
    );
    faults.resume.notify_one();
    initializing.await??;
    describe(&mut second).await?;
    drop(server);
    Ok(())
}

#[tokio::test]
async fn production_server_bounds_connections_and_reuses_released_capacity() -> Result {
    let (endpoint, server) = production(
        Arc::new(Faults::default()),
        ServerLimits {
            max_connections: 1,
            ..Default::default()
        },
    )
    .await?;
    let mut first = Client::connect(endpoint.clone()).await?;
    describe(&mut first).await?;
    let excess = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        let mut client = Client::connect(endpoint.clone()).await?;
        describe(&mut client).await
    })
    .await?;
    assert!(excess.is_err());
    describe(&mut first).await?;
    drop(first);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Ok(mut client) = Client::connect(endpoint.clone()).await
                && describe(&mut client).await.is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    drop(server);
    Ok(())
}

struct RawSession {
    outgoing: Option<tokio::sync::mpsc::Sender<p::Request>>,
    incoming: tonic::Streaming<p::Reply>,
}
impl RawSession {
    async fn open(client: &mut Client, generation: &str) -> Result<Self> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.send(p::to_request(
            generation,
            &Op::Open(p::Open {
                id: "instance".into(),
                config: Some(Default::default()),
            }),
            vec![],
        )?)
        .await?;
        let mut request = tonic::Request::new(tokio_stream::wrappers::ReceiverStream::new(rx));
        request
            .metadata_mut()
            .insert("authorization", "Bearer test-token".parse()?);
        let mut incoming = client.session(request).await?.into_inner();
        let reply = incoming.message().await?.ok_or("opening reply")?;
        assert!(matches!(
            p::from_reply(reply)??.into_value()?,
            p::Value::Storage(p::Storage { handle: 1, .. })
        ));
        Ok(Self {
            outgoing: Some(tx),
            incoming,
        })
    }
    async fn send(&self, operation: Op, data: Vec<u8>) -> Result {
        self.outgoing
            .as_ref()
            .ok_or("request half closed")?
            .send(p::to_request("", &operation, data)?)
            .await?;
        Ok(())
    }
    async fn call(&mut self, operation: Op, data: Vec<u8>) -> Result<p::Reply> {
        self.send(operation, data).await?;
        Ok(p::from_reply(
            self.incoming.message().await?.ok_or("session closed")?,
        )??)
    }
    async fn reader(&mut self) -> Result<u64> {
        match self
            .call(
                Op::Read(p::Read {
                    handle: 1,
                    path: "file".into(),
                    offset: 0,
                    length: None,
                }),
                vec![],
            )
            .await?
            .into_value()?
        {
            p::Value::Reader(p::Handle { handle }) => Ok(handle),
            _ => Err("reader".into()),
        }
    }
    async fn writer(&mut self) -> Result<u64> {
        match self
            .call(
                Op::Write(p::Write {
                    handle: 1,
                    path: "file".into(),
                    replace: false,
                    atomic: false,
                }),
                vec![],
            )
            .await?
            .into_value()?
        {
            p::Value::Writer(p::Handle { handle }) => Ok(handle),
            _ => Err("writer".into()),
        }
    }
}
async fn ready(endpoint: String) -> Result<(Client, String)> {
    let mut client = Client::connect(endpoint).await?;
    let generation = describe(&mut client).await?;
    rpc(
        &mut client,
        &generation,
        Op::Initialize(p::Initialize {
            settings: Some(Default::default()),
        }),
    )
    .await?;
    Ok((client, generation))
}
async fn until(condition: impl Fn() -> bool) -> Result {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !condition() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn disconnect_drops_every_resource_and_ids_are_isolated_between_sessions() -> Result {
    let faults = Arc::new(Faults::default());
    let (endpoint, server) = production(faults.clone(), ServerLimits::default()).await?;
    let (mut client, generation) = ready(endpoint).await?;
    let mut first = RawSession::open(&mut client, &generation).await?;
    let mut second = RawSession::open(&mut client, &generation).await?;
    let first_reader = first.reader().await?;
    let second_reader = second.reader().await?;
    assert_eq!(first_reader, second_reader);
    let writer = first.writer().await?;
    first
        .call(Op::WriterChunk(p::Handle { handle: writer }), vec![1, 2, 3])
        .await?;
    drop(first);
    until(|| {
        faults.storage_drops.load(Ordering::SeqCst) == 1
            && faults.reader_drops.load(Ordering::SeqCst) == 1
            && faults.writer_drops.load(Ordering::SeqCst) == 1
    })
    .await?;
    assert_eq!(faults.commits.load(Ordering::SeqCst), 0);
    assert_eq!(
        second
            .call(
                Op::ReaderNext(p::Handle {
                    handle: second_reader
                }),
                vec![]
            )
            .await?
            .data,
        vec![1]
    );
    drop((second, server));
    Ok(())
}

#[tokio::test]
async fn request_half_close_cancels_pending_backend_and_releases_staged_writes() -> Result {
    let faults = Arc::new(Faults::default());
    faults.suspend_read.store(true, Ordering::SeqCst);
    let (endpoint, server) = production(faults.clone(), ServerLimits::default()).await?;
    let (mut client, generation) = ready(endpoint).await?;
    let mut session = RawSession::open(&mut client, &generation).await?;
    session.writer().await?;
    let reader = session.reader().await?;
    session
        .send(Op::ReaderNext(p::Handle { handle: reader }), vec![])
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(1), faults.reading.notified()).await?;
    session.outgoing.take();
    until(|| {
        faults.storage_drops.load(Ordering::SeqCst) == 1
            && faults.reader_drops.load(Ordering::SeqCst) == 1
            && faults.writer_drops.load(Ordering::SeqCst) == 1
    })
    .await?;
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            session.incoming.message()
        )
        .await??
        .is_none()
    );
    assert_eq!(faults.commits.load(Ordering::SeqCst), 0);
    drop(server);
    Ok(())
}

#[tokio::test]
async fn idle_storage_needs_no_renewal_and_keeps_its_concurrency_slot() -> Result {
    let faults = Arc::new(Faults::default());
    let (endpoint, server) = production(
        faults.clone(),
        ServerLimits {
            max_concurrent_requests: 1,
            stream_idle_timeout: std::time::Duration::from_millis(50),
            ..Default::default()
        },
    )
    .await?;
    let (mut client, generation) = ready(endpoint).await?;
    let mut session = RawSession::open(&mut client, &generation).await?;
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(3601)).await;
    tokio::time::resume();
    assert_eq!(faults.storage_drops.load(Ordering::SeqCst), 0);
    let reply = session
        .call(
            Op::Storage(p::StorageCall {
                handle: 1,
                operation: Some(p::StorageOp::Stat(p::Stat {
                    lookup: Some(p::Lookup {
                        operand: "/file".into(),
                        working_directory: String::new(),
                        intermediate_links: p::IntermediateLink::FollowIntermediate.into(),
                    }),
                    final_link: p::FinalLink::PreserveFinal.into(),
                })),
            }),
            vec![],
        )
        .await?;
    assert!(matches!(reply.into_value()?, p::Value::Entry(_)));
    assert_eq!(
        client
            .invoke(request("", Op::Describe(p::Empty {}))?)
            .await
            .err()
            .ok_or("slot held")?
            .code(),
        tonic::Code::ResourceExhausted
    );
    drop(session);
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if describe(&mut client).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await?;
    drop(server);
    Ok(())
}

#[tokio::test]
async fn session_frames_share_the_server_rate_budget() -> Result {
    let faults = Arc::new(Faults::default());
    let (endpoint, server) = production(
        faults.clone(),
        ServerLimits {
            operations_per_second: 1,
            operation_burst: 5,
            ..Default::default()
        },
    )
    .await?;
    let (mut client, generation) = ready(endpoint).await?;
    let mut session = RawSession::open(&mut client, &generation).await?;
    let writer = session.writer().await?;
    session
        .call(Op::WriterChunk(p::Handle { handle: writer }), vec![1])
        .await?;
    session
        .send(Op::WriterChunk(p::Handle { handle: writer }), vec![2])
        .await?;
    assert_eq!(
        session
            .incoming
            .message()
            .await
            .err()
            .ok_or("rate budget")?
            .code(),
        tonic::Code::ResourceExhausted
    );
    assert_eq!(faults.chunks.load(Ordering::SeqCst), 1);
    until(|| faults.writer_drops.load(Ordering::SeqCst) == 1).await?;
    drop(server);
    Ok(())
}

#[tokio::test]
async fn cancelling_a_dispatched_host_request_closes_only_its_storage_session() -> Result {
    let faults = Arc::new(Faults::default());
    faults.suspend_read.store(true, Ordering::SeqCst);
    let (endpoint, server) = production(faults.clone(), ServerLimits::default()).await?;
    let driver = PluginDriver::connect(Arc::new(
        GrpcTransport::connect(endpoint, "test-token").await?,
    ))
    .await?;
    driver.initialize(Default::default()).await?;
    let first = open(&driver).await?;
    let second = open(&driver).await?;
    let mut reader = read(first.as_ref()).await?;
    let mut next = Box::pin(reader.next_chunk());
    tokio::select! { _=faults.reading.notified()=>{},_=&mut next=>return Err("read was not suspended".into()) }
    drop(next);
    until(|| first.is_stale() && faults.reader_drops.load(Ordering::SeqCst) == 1).await?;
    assert!(!second.is_stale());
    assert_eq!(stat(second.as_ref()).await?, 1);
    assert_eq!(stat(open(&driver).await?.as_ref()).await?, 1);
    drop(server);
    Ok(())
}

#[tokio::test]
async fn storage_resources_are_never_accessible_through_unary_invoke() -> Result {
    let faults = Arc::new(Faults::default());
    let (endpoint, server) = production(faults, ServerLimits::default()).await?;
    let (mut client, generation) = ready(endpoint).await?;
    let mut session = RawSession::open(&mut client, &generation).await?;
    let reader = session.reader().await?;
    for operation in [
        Op::Open(p::Open {
            id: "instance".into(),
            config: Some(Default::default()),
        }),
        Op::ReaderNext(p::Handle { handle: reader }),
    ] {
        assert_eq!(
            client
                .invoke(request(&generation, operation)?)
                .await
                .err()
                .ok_or("unscoped resource")?
                .code(),
            tonic::Code::InvalidArgument
        );
    }
    assert_eq!(
        session
            .call(Op::ReaderNext(p::Handle { handle: reader }), vec![])
            .await?
            .data,
        vec![1]
    );
    drop(server);
    Ok(())
}

#[tokio::test]
async fn production_http2_stream_limit_applies_until_a_storage_session_closes() -> Result {
    let (endpoint, server) = production(
        Arc::new(Faults::default()),
        ServerLimits {
            max_streams_per_connection: 1,
            ..Default::default()
        },
    )
    .await?;
    let (mut client, generation) = ready(endpoint).await?;
    let session = RawSession::open(&mut client, &generation).await?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), describe(&mut client))
            .await
            .is_err()
    );
    drop(session);
    tokio::time::timeout(std::time::Duration::from_secs(1), describe(&mut client)).await??;
    drop(server);
    Ok(())
}

#[tokio::test]
async fn authentication_precedes_protobuf_decoding_and_applies_to_every_rpc() -> Result {
    let (endpoint, server) =
        production(Arc::new(Faults::default()), ServerLimits::default()).await?;
    let mut client = Client::connect(endpoint)
        .await?
        .max_encoding_message_size(p::MAX_FRAME_BYTES + 4096);
    let mut oversized = request("", Op::Describe(p::Empty {}))?;
    oversized
        .metadata_mut()
        .insert("authorization", "Bearer wrong-token".parse()?);
    oversized.get_mut().data = vec![0; p::MAX_FRAME_BYTES + 2048];
    assert_eq!(
        client
            .invoke(oversized)
            .await
            .err()
            .ok_or("early authentication")?
            .code(),
        tonic::Code::Unauthenticated
    );
    let mut request = tonic::Request::new(tokio_stream::iter([p::Request {
        data: vec![0; p::MAX_FRAME_BYTES + 2048],
        ..Default::default()
    }]));
    request
        .metadata_mut()
        .insert("authorization", "Bearer wrong-token".parse()?);
    assert_eq!(
        client
            .session(request)
            .await
            .err()
            .ok_or("session authentication")?
            .code(),
        tonic::Code::Unauthenticated
    );
    drop(server);
    Ok(())
}

#[tokio::test]
async fn retained_streams_invalidate_a_refreshed_connection_to_the_same_backend() -> Result {
    let faults = Arc::new(Faults::default());
    let dispatcher = Arc::new(Dispatcher::new(Arc::new(Backend::new(1, faults.clone())?))?);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = start(listener, dispatcher.clone()).await?;
    let driver = PluginDriver::connect(Arc::new(
        GrpcTransport::connect(endpoint, "test-token").await?,
    ))
    .await?;
    driver.initialize(Default::default()).await?;
    let retained = open(&driver).await?;
    let failing = open(&driver).await?;
    faults.invalid_stat.store(true, Ordering::SeqCst);
    assert!(stat(failing.as_ref()).await.is_err());
    until(|| failing.is_stale()).await?;
    faults.invalid_stat.store(false, Ordering::SeqCst);
    let transient = open(&driver).await?;
    assert_eq!(stat(transient.as_ref()).await?, 1);
    drop(transient);
    dispatcher
        .replace_driver(Arc::new(Backend::new(2, faults)?))
        .await?;
    until(|| retained.is_stale()).await?;
    assert_eq!(stat(open(&driver).await?.as_ref()).await?, 2);
    assert!(failing.is_stale());
    drop(server);
    Ok(())
}

#[tokio::test]
async fn dropping_child_resources_outside_tokio_does_not_need_an_expiry_scan() -> Result {
    let faults = Arc::new(Faults::default());
    let (endpoint, server) = production(faults.clone(), ServerLimits::default()).await?;
    let driver = PluginDriver::connect(Arc::new(
        GrpcTransport::connect(endpoint, "test-token").await?,
    ))
    .await?;
    driver.initialize(Default::default()).await?;
    let storage = open(&driver).await?;
    let reader = read(storage.as_ref()).await?;
    let writer = storage
        .write_file(StorageWriteFileRequest::new(StoragePath::parse("file")?))
        .await?;
    std::thread::spawn(move || drop((reader, writer)))
        .join()
        .map_err(|_| "resource drop thread panicked")?;
    until(|| {
        faults.reader_drops.load(Ordering::SeqCst) == 1
            && faults.writer_drops.load(Ordering::SeqCst) == 1
    })
    .await?;
    assert_eq!(faults.commits.load(Ordering::SeqCst), 0);
    assert!(!storage.is_stale());
    assert_eq!(stat(storage.as_ref()).await?, 1);
    drop(server);
    Ok(())
}

#[tokio::test]
async fn resource_capacity_is_shared_across_sessions_and_released_on_disconnect() -> Result {
    let dispatcher = Dispatcher::new(Arc::new(Backend::new(1, Arc::new(Faults::default()))?))?;
    let generation = dispatcher.generation();
    dispatcher
        .call(
            &generation,
            Op::Initialize(p::Initialize {
                settings: Some(Default::default()),
            }),
            vec![],
        )
        .await?;
    let opening = || p::Open {
        id: "instance".into(),
        config: Some(Default::default()),
    };
    let first = dispatcher.open_session(&generation, opening()).await?;
    let second = dispatcher.open_session(&generation, opening()).await?;
    let read = || {
        Op::Read(p::Read {
            handle: 1,
            path: "file".into(),
            offset: 0,
            length: None,
        })
    };
    for _ in 0..p::MAX_RESOURCES - 2 {
        first.call(read(), vec![]).await?;
    }
    let error = second
        .call(read(), vec![])
        .await
        .err()
        .ok_or("global resource capacity")?;
    assert_eq!(
        error.storage_error().ok_or("category")?.failure(),
        Failure::Backend(BackendFailure::CapacityExceeded)
    );
    drop(first);
    assert!(matches!(
        second.call(read(), vec![]).await?.into_value()?,
        p::Value::Reader(_)
    ));
    Ok(())
}

#[tokio::test]
async fn shutdown_cannot_be_undone_by_an_inflight_backend_recovery() -> Result {
    let faults = Arc::new(Faults::default());
    let started = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let factory: Arc<DriverFactory> = {
        let faults = faults.clone();
        let started = started.clone();
        let resume = resume.clone();
        Arc::new(move || {
            let faults = faults.clone();
            let started = started.clone();
            let resume = resume.clone();
            Box::pin(async move {
                let marker = faults.constructions.fetch_add(1, Ordering::SeqCst) + 1;
                if marker == 2 {
                    started.notify_one();
                    resume.notified().await;
                }
                Ok(
                    Arc::new(Backend::new(marker, faults).map_err(|_| c::protocol_violation())?)
                        as Arc<dyn Driver>,
                )
            })
        })
    };
    let dispatcher = Arc::new(Dispatcher::with_factory(factory).await?);
    let driver = PluginDriver::connect(Arc::new(Direct(dispatcher.clone()))).await?;
    driver.initialize(Default::default()).await?;
    let storage = open(&driver).await?;
    faults.panic_remove.store(true, Ordering::SeqCst);
    assert!(
        storage
            .remove_entry(StorageRemoveEntryRequest {
                path: StoragePath::parse("file")?
            })
            .await
            .is_err()
    );
    let recovering = {
        let dispatcher = dispatcher.clone();
        tokio::spawn(async move { dispatcher.call("", Op::Describe(p::Empty {}), vec![]).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified()).await?;
    dispatcher.shutdown();
    resume.notify_one();
    assert!(recovering.await?.is_err());
    assert!(
        dispatcher
            .call("", Op::Describe(p::Empty {}), vec![])
            .await
            .is_err()
    );
    assert!(
        dispatcher
            .open_session(
                &dispatcher.generation(),
                p::Open {
                    id: "instance".into(),
                    config: Some(Default::default())
                }
            )
            .await
            .is_err()
    );
    assert_eq!(faults.constructions.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn grpc_rejects_missing_open_configuration_before_allocating_storage() -> Result {
    let faults = Arc::new(Faults::default());
    let (endpoint, server) = production(faults.clone(), ServerLimits::default()).await?;
    let (mut client, generation) = ready(endpoint).await?;
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(p::Request {
        generation,
        data: vec![],
        operation: Some(Op::Open(p::Open {
            id: "instance".into(),
            config: None,
        })),
    })
    .await?;
    let mut request = tonic::Request::new(tokio_stream::wrappers::ReceiverStream::new(rx));
    request
        .metadata_mut()
        .insert("authorization", "Bearer test-token".parse()?);
    match client.session(request).await {
        Err(error) => assert_eq!(error.code(), tonic::Code::InvalidArgument),
        Ok(response) => {
            let error = response
                .into_inner()
                .message()
                .await
                .err()
                .ok_or("missing config accepted")?;
            assert_eq!(error.code(), tonic::Code::InvalidArgument);
        }
    }
    drop(tx);
    drop(server);
    Ok(())
}

#[tokio::test]
async fn corrupt_backend_data_does_not_retire_a_valid_protocol_session() -> Result {
    let faults = Arc::new(Faults::default());
    let (endpoint, server) = production(faults.clone(), ServerLimits::default()).await?;
    let driver = PluginDriver::connect(Arc::new(
        GrpcTransport::connect(endpoint, "test-token").await?,
    ))
    .await?;
    driver.initialize(Default::default()).await?;
    let storage = open(&driver).await?;
    faults.corrupt_data.store(true, Ordering::SeqCst);
    let error = stat(storage.as_ref())
        .await
        .err()
        .ok_or("corrupt data accepted")?;
    assert_eq!(
        error
            .downcast_ref::<StorageError>()
            .ok_or("storage error")?
            .failure(),
        Failure::Backend(BackendFailure::CorruptData)
    );
    assert!(!storage.is_stale());
    faults.corrupt_data.store(false, Ordering::SeqCst);
    assert_eq!(stat(storage.as_ref()).await?, 1);
    drop(server);
    Ok(())
}
