use ah_vfs::{
    application::{driver_info::DriverInfo, dto::*, port::*},
    domain::*,
};
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
    serve_with_limits, service, service_with_limits, wire,
};
use vfs_plugin_host::{PluginDriver, Transport, TransportFuture};
use vfs_plugin_protocol::{self as p, Operation as Op};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Default)]
struct Faults {
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
            ownership: p::Ownership::Host as i32,
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
    fn instances(&self) -> InstanceManagement {
        InstanceManagement::HostManaged(Arc::new(Provisioner))
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
                capabilities: self.info.capabilities(),
            }) as Arc<dyn Storage>)
        })
    }
}
struct BackendStorage {
    marker: usize,
    faults: Arc<Faults>,
    capabilities: StorageCapabilities,
}
impl Storage for BackendStorage {
    fn capabilities(&self) -> StorageCapabilities {
        self.capabilities
    }
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
    fn stat_entry(&self, request: StorageStatEntryRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            Ok(StorageEntry {
                path: request.path,
                kind: StorageEntryKind::File,
                size: Some(self.marker as u64),
                modified: None,
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
impl StorageFileWriter for Writer {
    fn write_chunk<'a>(&'a mut self, _: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            self.0.chunks.fetch_add(1, Ordering::SeqCst);
            self.0.writing.notify_one();
            Ok(())
        })
    }
    fn commit(&mut self) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation)) })
    }
    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}
struct Direct(Arc<Dispatcher>);
impl Transport for Direct {
    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(self.0.call(generation, op, data))
    }
}
async fn open(driver: &PluginDriver) -> Result<Arc<dyn Storage>> {
    Ok(driver
        .open(OpenContext::host_managed(
            InstanceId::parse("instance")?,
            Default::default(),
        ))
        .await?)
}
async fn stat(storage: &dyn Storage) -> Result<u64> {
    Ok(storage
        .stat_entry(StorageStatEntryRequest {
            path: StoragePath::parse("file")?,
        })
        .await?
        .size
        .ok_or("file size")?)
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
                    return Err(StorageError::new(StorageErrorKind::Unavailable).into());
                }
                let marker = faults.constructions.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(
                    Arc::new(Backend::new(marker, faults).map_err(|_| c::invalid())?)
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
    assert_eq!(error.kind(), StorageErrorKind::OutcomeUnknown);
    faults.fail_factory.store(true, Ordering::SeqCst);
    assert!(open(&driver).await.is_err());
    faults.fail_factory.store(false, Ordering::SeqCst);
    let new = open(&driver).await?;
    assert_eq!(stat(new.as_ref()).await?, 2);
    assert_ne!(dispatcher.generation(), original);
    assert_eq!(
        reader.next_chunk().await.err().ok_or("old reader")?.kind(),
        StorageErrorKind::StaleHandle
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
    let service = service(dispatcher, "test-token")?;
    start_service(listener, service).await
}
async fn start_service(
    listener: tokio::net::TcpListener,
    service: GrpcService,
) -> Result<ServerTask> {
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = stopped.await;
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
    let generation = dispatcher.generation();
    dispatcher
        .replace_driver(Arc::new(Backend::new(2, faults)?))
        .await?;
    driver.reconnect().await?;
    let new = open(&driver).await?;
    let mut new_reader = read(new.as_ref()).await?;
    assert_eq!(new_reader.next_chunk().await?, Some(vec![2]));
    // Resource IDs restart at one. Releasing reader 2 in the previous generation
    // must not remove reader 2's transport stream in the replacement generation.
    assert!(
        transport
            .call(
                &generation,
                Op::Release(p::ResourceRef {
                    kind: p::ResourceKind::Reader as i32,
                    handle: 2
                }),
                vec![]
            )
            .await
            .is_err()
    );
    assert_eq!(new_reader.next_chunk().await?, None);
    assert_eq!(
        reader.next_chunk().await.err().ok_or("old reader")?.kind(),
        StorageErrorKind::StaleHandle
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
    let listener = tokio::net::TcpListener::bind(address).await?;
    let server = start(
        listener,
        Arc::new(Dispatcher::new(Arc::new(Backend::new(2, faults)?))?),
    )
    .await?;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if driver.reconnect().await.is_ok() {
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
        error.storage_error().ok_or("category")?.kind(),
        StorageErrorKind::Conflict
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
async fn read_response_keeps_its_global_slot_after_returning_headers() -> Result {
    use wire::plugin_server::Plugin;
    let faults = Arc::new(Faults::default());
    let dispatcher = Arc::new(Dispatcher::new(Arc::new(Backend::new(1, faults)?))?);
    let driver = PluginDriver::connect(Arc::new(Direct(dispatcher.clone()))).await?;
    driver.initialize(Default::default()).await?;
    let storage = open(&driver).await?;
    let reader = read(storage.as_ref()).await?;
    let service = PluginService::with_limits(
        dispatcher.clone(),
        "test-token",
        ServerLimits {
            max_concurrent_requests: 1,
            ..Default::default()
        },
    )?;
    let response = service
        .read(request(
            &dispatcher.generation(),
            Op::ReaderNext(p::Handle { handle: 2 }),
        )?)
        .await?;
    assert_eq!(
        service
            .clone()
            .invoke(request("", Op::Describe(p::Empty {}))?)
            .await
            .err()
            .ok_or("active reader")?
            .code(),
        tonic::Code::ResourceExhausted
    );
    drop(response);
    tokio::task::yield_now().await;
    service
        .invoke(request("", Op::Describe(p::Empty {}))?)
        .await?;
    drop(reader);
    Ok(())
}

#[tokio::test]
async fn streamed_write_frames_cannot_bypass_the_rate_limit() -> Result {
    let faults = Arc::new(Faults::default());
    let dispatcher = Arc::new(Dispatcher::new(Arc::new(Backend::new(1, faults.clone())?))?);
    let driver = PluginDriver::connect(Arc::new(Direct(dispatcher.clone()))).await?;
    driver.initialize(Default::default()).await?;
    let storage = open(&driver).await?;
    let writer = storage
        .write_file(StorageWriteFileRequest::new(StoragePath::parse("file")?))
        .await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let service = service_with_limits(
        dispatcher.clone(),
        "test-token",
        ServerLimits {
            operations_per_second: 1,
            operation_burst: 2,
            ..Default::default()
        },
    )?;
    let server = start_service(listener, service).await?;
    let mut client = Client::connect(endpoint).await?;
    let frame = request(
        &dispatcher.generation(),
        Op::WriterChunk(p::Handle { handle: 2 }),
    )?
    .into_inner();
    let mut request = tonic::Request::new(tokio_stream::iter([frame.clone(), frame]));
    request
        .metadata_mut()
        .insert("authorization", "Bearer test-token".parse()?);
    let error = client
        .write(request)
        .await
        .err()
        .ok_or("frame rate limit")?;
    assert_eq!(error.code(), tonic::Code::ResourceExhausted);
    assert_eq!(faults.chunks.load(Ordering::SeqCst), 1);
    drop((writer, server));
    Ok(())
}

#[tokio::test]
async fn idle_writers_hold_a_global_slot_and_release_it_on_timeout() -> Result {
    let faults = Arc::new(Faults::default());
    let dispatcher = Arc::new(Dispatcher::new(Arc::new(Backend::new(1, faults.clone())?))?);
    let driver = PluginDriver::connect(Arc::new(Direct(dispatcher.clone()))).await?;
    driver.initialize(Default::default()).await?;
    let storage = open(&driver).await?;
    let writer = storage
        .write_file(StorageWriteFileRequest::new(StoragePath::parse("file")?))
        .await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let service = service_with_limits(
        dispatcher.clone(),
        "test-token",
        ServerLimits {
            max_concurrent_requests: 1,
            stream_idle_timeout: std::time::Duration::from_millis(250),
            ..Default::default()
        },
    )?;
    let server = start_service(listener, service).await?;
    let mut client = Client::connect(endpoint).await?;
    let mut writer_client = client.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(
        request(
            &dispatcher.generation(),
            Op::WriterChunk(p::Handle { handle: 2 }),
        )?
        .into_inner(),
    )
    .await?;
    let mut request = tonic::Request::new(tokio_stream::wrappers::ReceiverStream::new(rx));
    request
        .metadata_mut()
        .insert("authorization", "Bearer test-token".parse()?);
    let writing = tokio::spawn(async move { writer_client.write(request).await });
    tokio::time::timeout(std::time::Duration::from_secs(1), faults.writing.notified()).await?;
    assert_eq!(
        client
            .invoke(self::request("", Op::Describe(p::Empty {}))?)
            .await
            .err()
            .ok_or("writer slot")?
            .code(),
        tonic::Code::ResourceExhausted
    );
    let error = tokio::time::timeout(std::time::Duration::from_secs(2), writing)
        .await??
        .err()
        .ok_or("idle timeout")?;
    assert_eq!(error.code(), tonic::Code::DeadlineExceeded);
    describe(&mut client).await?;
    drop((writer, tx, server));
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

#[tokio::test]
async fn production_http2_stream_limit_applies_until_a_read_stream_closes() -> Result {
    let faults = Arc::new(Faults::default());
    faults.suspend_read.store(true, Ordering::SeqCst);
    let (endpoint, server) = production(
        faults.clone(),
        ServerLimits {
            max_streams_per_connection: 1,
            ..Default::default()
        },
    )
    .await?;
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
    let storage = match rpc(
        &mut client,
        &generation,
        Op::Open(p::Open {
            id: "instance".into(),
            config: Some(Default::default()),
        }),
    )
    .await?
    {
        p::Value::Storage(p::Storage { handle, .. }) => handle,
        _ => return Err("storage".into()),
    };
    let reader = match rpc(
        &mut client,
        &generation,
        Op::Read(p::Read {
            handle: storage,
            path: "file".into(),
            offset: 0,
            length: None,
        }),
    )
    .await?
    {
        p::Value::Reader(p::Handle { handle }) => handle,
        _ => return Err("reader".into()),
    };
    let stream = client
        .read(request(
            &generation,
            Op::ReaderNext(p::Handle { handle: reader }),
        )?)
        .await?
        .into_inner();
    tokio::time::timeout(std::time::Duration::from_secs(1), faults.reading.notified()).await?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), describe(&mut client))
            .await
            .is_err()
    );
    drop(stream);
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
        .max_encoding_message_size(p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 4096);
    let mut oversized = request("", Op::Describe(p::Empty {}))?;
    oversized
        .metadata_mut()
        .insert("authorization", "Bearer wrong-token".parse()?);
    oversized.get_mut().data = vec![0; p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 2048];
    assert_eq!(
        client
            .invoke(oversized)
            .await
            .err()
            .ok_or("early authentication")?
            .code(),
        tonic::Code::Unauthenticated
    );
    let mut bad_read = request("", Op::ReaderNext(p::Handle { handle: 1 }))?;
    bad_read
        .metadata_mut()
        .insert("authorization", "Bearer wrong-token".parse()?);
    assert_eq!(
        client
            .read(bad_read)
            .await
            .err()
            .ok_or("reader authentication")?
            .code(),
        tonic::Code::Unauthenticated
    );
    let mut bad_write = tonic::Request::new(tokio_stream::empty::<wire::Request>());
    bad_write
        .metadata_mut()
        .insert("authorization", "Bearer wrong-token".parse()?);
    assert_eq!(
        client
            .write(bad_write)
            .await
            .err()
            .ok_or("writer authentication")?
            .code(),
        tonic::Code::Unauthenticated
    );
    drop(server);
    Ok(())
}
