use ah_vfs::application::driver_error::{DriverError, LifecycleError, LifecycleFailure};
use ah_vfs::application::{driver_info::DriverInfo, port::*};
use ah_vfs::domain::InstanceId;
use ah_vfs::resolve::LookupContext;
use ah_vfs::resolve::StorageOperations;
use ah_vfs::resolve::{
    ResolveFailure,
    operand::{
        CreateDirectoryMode, CreateDirectoryOperand, ListOperand, ReadOperand, RemoveOperand,
        RenameOperand, StatOperand, WriteOperand,
    },
    path::PathExpr,
    policy::{IntermediateSymlinks, SymlinkAction},
};
use ah_vfs_storage as d;
use hyper_util::rt::TokioIo;
use std::{error::Error, future::ready, io, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tonic::{
    Request, Streaming,
    transport::{Channel, Endpoint, Server},
};
use vfs_driver_memory::{Capacity, CapacityUnit, MemoryDriver, MemoryStorage};
use vfs_plugin_grpc_server::dispatch::Dispatcher;
use vfs_plugin_protocol as p;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const TIMEOUT: Duration = Duration::from_secs(5);

struct BoundMemory {
    driver: MemoryDriver,
    storage: Arc<MemoryStorage>,
}
impl Driver for BoundMemory {
    fn info(&self) -> &DriverInfo {
        self.driver.info()
    }

    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        self.driver.provisioner()
    }

    fn open(&self, _: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        Box::pin(async { Ok(self.storage.clone() as Arc<dyn Storage>) })
    }
}

struct Fixture {
    storage: Arc<MemoryStorage>,
    dispatcher: Arc<Dispatcher>,
    channel: Channel,
    generation: String,
    task: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}
impl Fixture {
    async fn new(quota: u64) -> TestResult<Self> {
        let mut links = [
            ("alias", b"docs".as_slice()),
            ("opaque", b"\xff"),
            ("dangling", b"missing"),
            ("loop", b"loop"),
            ("root-link", b"/"),
        ]
        .map(|(name, target)| {
            Ok((
                d::EntryName::parse(name)?,
                d::LinkTarget::new(target.to_vec()),
            ))
        })
        .into_iter()
        .collect::<Result<Vec<_>, d::NameError>>()?;
        for hop in 0..21 {
            let target = if hop == 20 {
                "docs".to_owned()
            } else {
                format!("step{}", hop + 1)
            };
            links.push((
                d::EntryName::parse(&format!("step{hop}"))?,
                d::LinkTarget::new(target.into_bytes()),
            ));
        }
        let storage = Arc::new(MemoryStorage::with_root_links(
            Capacity::new(quota, CapacityUnit::B)?,
            links,
        )?);
        let dispatcher = Arc::new(Dispatcher::new(Arc::new(BoundMemory {
            driver: MemoryDriver::new()?,
            storage: storage.clone(),
        }))?);
        let generation = dispatcher.generation();
        dispatcher
            .call(
                &generation,
                p::Operation::Initialize(p::Initialize {
                    settings: Some(Default::default()),
                }),
                vec![],
            )
            .await?;
        let formal = vfs_plugin_grpc_server::service(dispatcher.clone(), "storage-test")?;
        // Run the actual HTTP/2 and Protobuf stack without listening on a port.
        let (client, server) = tokio::io::duplex(64 * 1024);
        let incoming =
            tokio_stream::once(Ok::<_, io::Error>(server)).chain(tokio_stream::pending());
        let task = tokio::spawn(async move {
            Server::builder()
                .add_service(formal)
                .serve_with_incoming(incoming)
                .await
        });
        let mut client = Some(client);
        let channel = Endpoint::from_static("http://storage.test")
            .connect_timeout(TIMEOUT)
            .connect_with_connector(tower::service_fn(move |_| {
                ready(
                    client
                        .take()
                        .map(TokioIo::new)
                        .ok_or_else(|| io::Error::other("fixture connection already used")),
                )
            }))
            .await?;
        Ok(Self {
            storage,
            dispatcher,
            channel,
            generation,
            task,
        })
    }

    async fn formal_session(&self) -> TestResult<FormalSession> {
        let wire = WireSession::open(
            self.channel.clone(),
            &self.generation,
            p::Open {
                id: "fixture".into(),
                config: Some(Default::default()),
            },
        )
        .await?;
        let driver = vfs_plugin_host::PluginDriver::connect(Arc::new(TestTransport {
            dispatcher: self.dispatcher.clone(),
            channel: self.channel.clone(),
        }))
        .await?;
        driver.initialize(Default::default()).await?;
        let storage = driver
            .open(OpenContext::new(
                InstanceId::parse("fixture")?,
                Default::default(),
            ))
            .await?;
        Ok(FormalSession {
            handle: wire.storage.handle,
            wire,
            storage,
            writer: None,
        })
    }

    async fn wait_for_used(&self, expected: u64) -> TestResult {
        tokio::time::timeout(TIMEOUT, async {
            while self.storage.used_bytes() != expected {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct WireSession {
    tx: mpsc::Sender<p::Request>,
    replies: Streaming<p::Reply>,
    storage: p::Storage,
}

impl WireSession {
    async fn open(channel: Channel, generation: &str, open: p::Open) -> Result<Self, DriverError> {
        use vfs_plugin_grpc_server::wire::plugin_client::PluginClient;
        let mut client = PluginClient::new(channel);
        let (tx, rx) = mpsc::channel(1);
        tx.send(
            p::to_request(generation, &p::Operation::Open(open), vec![])
                .map_err(vfs_plugin_codec::protocol_violation_with_source)?,
        )
        .await
        .map_err(wire_error)?;
        let mut request = Request::new(ReceiverStream::new(rx));
        request.metadata_mut().insert(
            "authorization",
            tonic::metadata::MetadataValue::from_static("Bearer storage-test"),
        );
        let mut replies = tokio::time::timeout(TIMEOUT, client.session(request))
            .await
            .map_err(wire_error)?
            .map_err(wire_error)?
            .into_inner();
        let opened = replies
            .message()
            .await
            .map_err(wire_error)?
            .ok_or_else(vfs_plugin_codec::protocol_violation)?;
        let Some(p::Value::Storage(storage)) = opened.result else {
            return Err(vfs_plugin_codec::protocol_violation());
        };
        Ok(Self {
            tx,
            replies,
            storage,
        })
    }

    async fn invoke(&mut self, operation: p::Operation, data: Vec<u8>) -> TestResult<p::Reply> {
        // Deliberately let the server validate malformed test requests.
        self.tx
            .send(p::Request {
                operation: Some(operation),
                data,
                ..Default::default()
            })
            .await?;
        Ok(tokio::time::timeout(TIMEOUT, self.replies.message())
            .await??
            .ok_or("missing response")?)
    }
}

fn wire_error(source: impl Error + Send + Sync + 'static) -> DriverError {
    LifecycleError::with_source(LifecycleFailure::Unavailable, source).into()
}

struct TestTransport {
    dispatcher: Arc<Dispatcher>,
    channel: Channel,
}

impl vfs_plugin_host::TransportFactory for TestTransport {
    fn connect(self: Arc<Self>) -> vfs_plugin_host::ConnectFuture {
        Box::pin(async move { Ok(self as Arc<dyn vfs_plugin_host::Transport>) })
    }
}

impl vfs_plugin_host::SessionTransport for TestTransport {
    fn call<'a>(
        &'a self,
        generation: &'a str,
        op: p::Operation,
        data: Vec<u8>,
    ) -> vfs_plugin_host::TransportFuture<'a> {
        Box::pin(async move { self.dispatcher.call(generation, op, data).await })
    }
}

impl vfs_plugin_host::Transport for TestTransport {
    fn open_session(
        self: Arc<Self>,
        generation: &str,
        open: p::Open,
    ) -> vfs_plugin_host::OpenFuture<'_> {
        Box::pin(async move {
            let session = WireSession::open(self.channel.clone(), generation, open).await?;
            Ok(vfs_plugin_host::OpenedStorage {
                storage: session.storage,
                generation: generation.into(),
                transport: Arc::new(TestSession(Mutex::new(session))),
            })
        })
    }
}

struct TestSession(Mutex<WireSession>);
impl vfs_plugin_host::SessionTransport for TestSession {
    fn call<'a>(
        &'a self,
        _: &'a str,
        op: p::Operation,
        data: Vec<u8>,
    ) -> vfs_plugin_host::TransportFuture<'a> {
        Box::pin(async move {
            let reply = self
                .0
                .lock()
                .await
                .invoke(op.clone(), data)
                .await
                .map_err(|error| {
                    vfs_plugin_codec::call_error(
                        &op,
                        d::Failure::Backend(d::BackendFailure::Unavailable),
                        true,
                        Some(Box::new(io::Error::other(error.to_string()))),
                    )
                })?;
            match reply.result {
                Some(p::Value::Failure(failure)) => {
                    Err(vfs_plugin_codec::driver_error(failure, None))
                }
                _ => Ok(reply),
            }
        })
    }
}

struct FormalSession {
    wire: WireSession,
    storage: Arc<dyn Storage>,
    handle: u64,
    writer: Option<u64>,
}

struct FormalReply {
    result: Option<p::Value>,
    data: Vec<u8>,
    resolution_error: Option<d::StorageError>,
}

impl From<p::Reply> for FormalReply {
    fn from(reply: p::Reply) -> Self {
        Self {
            result: reply.result,
            data: reply.data,
            resolution_error: None,
        }
    }
}

enum Call {
    Stat(StatOperand, LookupContext),
    CreateDirectory(CreateDirectoryOperand, LookupContext),
    Rename(RenameOperand, LookupContext),
    Remove(RemoveOperand, LookupContext),
    Wire(p::StorageOp),
}

impl From<p::StorageOp> for Call {
    fn from(op: p::StorageOp) -> Self {
        Self::Wire(op)
    }
}

impl FormalSession {
    async fn remove(&mut self, path: &str, kind: d::RemoveKind) -> TestResult {
        let request = RemoveOperand::new(PathExpr::parse(path)?, kind)?;
        Ok(self
            .storage
            .remove_entry(request, Default::default())
            .await?)
    }

    async fn mkdir(&mut self, path: &str, mode: CreateDirectoryMode) -> TestResult<d::Entry> {
        Ok(self
            .storage
            .create_dir(
                CreateDirectoryOperand::new(PathExpr::parse(path)?, mode),
                Default::default(),
            )
            .await?)
    }

    async fn call(&mut self, operation: impl Into<Call>) -> TestResult<FormalReply> {
        let entry = |result: Result<d::Entry, d::StorageError>| -> TestResult<Result<p::Value, d::StorageError>> {
            match result {
                Ok(entry) => Ok(Ok(p::Value::Entry(p::encode_entry(&entry)?))),
                Err(error) => Ok(Err(error)),
            }
        };
        let value = match operation.into() {
            Call::Stat(operand, context) => entry(self.storage.stat_entry(operand, context).await)?,
            Call::CreateDirectory(operand, context) => {
                entry(self.storage.create_dir(operand, context).await)?
            }
            Call::Rename(operand, context) => {
                entry(self.storage.move_entry(operand, context).await)?
            }
            Call::Remove(operand, context) => self
                .storage
                .remove_entry(operand, context)
                .await
                .map(|_| p::Value::Unit(p::Empty {})),
            Call::Wire(operation) => {
                return self
                    .invoke(p::Operation::Storage(p::StorageCall {
                        handle: self.handle,
                        operation: Some(operation),
                    }))
                    .await
                    .map(FormalReply::from);
            }
        };
        Ok(match value {
            Ok(value) => FormalReply::from(p::Reply::new(value)),
            Err(error) => FormalReply {
                result: None,
                data: vec![],
                resolution_error: Some(error),
            },
        })
    }

    async fn invoke(&mut self, operation: p::Operation) -> TestResult<p::Reply> {
        self.invoke_with_data(operation, vec![]).await
    }

    async fn invoke_with_data(
        &mut self,
        operation: p::Operation,
        data: Vec<u8>,
    ) -> TestResult<p::Reply> {
        self.wire.invoke(operation, data).await
    }

    async fn bound_slot(&mut self, path: &str, purpose: &str) -> TestResult<(u64, p::BoundSlot)> {
        let path = d::StoragePath::parse(path.trim_start_matches('/'))?;
        let p::Value::OpenedLookup(scope) = self
            .invoke(p::Operation::BeginLookup(p::LookupHandle {
                handle: self.handle,
                purpose: purpose.into(),
            }))
            .await?
            .into_value()?
        else {
            return Err("lookup scope".into());
        };
        let mut parent = scope.root.ok_or("lookup root")?.id;
        let mut prefix = d::StoragePath::root();
        let final_name = path.name().ok_or("final name")?;
        let mut final_entry = None;
        for name in path.entry_names() {
            prefix = prefix.join(&name)?;
            let reply = self
                .invoke(p::Operation::ObserveChild(p::ObserveChild {
                    lookup: scope.handle,
                    parent,
                    name: name.to_string(),
                    purpose: purpose.into(),
                }))
                .await?;
            if prefix == path {
                match reply.result {
                    Some(p::Value::ObservedEntry(entry)) => final_entry = Some(entry.id),
                    _ => formal_failure(
                        reply,
                        p::decode_lookup_purpose(purpose).ok_or("purpose")?,
                        d::Rejection::NotFound,
                    )?,
                }
            } else {
                let p::Value::ObservedEntry(entry) = reply.into_value()? else {
                    return Err("parent observation".into());
                };
                parent = entry.id;
            }
        }
        Ok((
            scope.handle,
            p::BoundSlot {
                slot: Some(p::EntrySlot {
                    parent,
                    name: final_name.to_string(),
                    path: path.to_string(),
                }),
                entry: final_entry,
            },
        ))
    }

    async fn execute(&mut self, lookup: u64, operation: p::BoundOp) -> TestResult<p::Reply> {
        let reply = self
            .invoke(p::Operation::Execute(p::BoundCall {
                lookup,
                operation: Some(operation),
            }))
            .await?;
        self.invoke(p::Operation::Release(p::ResourceRef {
            handle: lookup,
            kind: p::ResourceKind::Lookup.into(),
        }))
        .await?;
        Ok(reply)
    }

    async fn read(&mut self, path: &str, range: d::ReadRange) -> TestResult<u64> {
        let (lookup, slot) = self.bound_slot(path, "read").await?;
        let reply = self
            .execute(
                lookup,
                p::BoundOp::Read(p::BoundRead {
                    target: Some(slot),
                    offset: range.offset(),
                    length: range.length(),
                }),
            )
            .await?;
        let Some(p::Value::Reader(reader)) = reply.result else {
            return Err("expected reader".into());
        };
        Ok(reader.handle)
    }

    async fn next_chunk(&mut self, handle: u64) -> TestResult<Option<Vec<u8>>> {
        let reply = self
            .invoke(p::Operation::ReaderNext(p::Handle { handle }))
            .await?;
        match reply.result {
            Some(p::Value::Chunk(p::Chunk { eof: true })) if reply.data.is_empty() => Ok(None),
            Some(p::Value::Chunk(p::Chunk { eof: false })) if !reply.data.is_empty() => {
                Ok(Some(reply.data))
            }
            _ => Err("expected formal read chunk or EOF".into()),
        }
    }

    async fn begin(&mut self, path: &str, mode: d::WriteMode) -> TestResult {
        let (lookup, slot) = self.bound_slot(path, "write").await?;
        let mode = match mode {
            d::WriteMode::CreateNew => p::WriteMode::CreateNew,
            d::WriteMode::Replace => p::WriteMode::Replace,
        };
        let reply = self
            .execute(
                lookup,
                p::BoundOp::Write(p::BoundWrite {
                    target: Some(slot),
                    mode: mode.into(),
                    publication: p::Publication::Atomic.into(),
                }),
            )
            .await?;
        let Some(p::Value::Writer(handle)) = reply.result else {
            return Err("expected writer".into());
        };
        self.writer = Some(handle.handle);
        Ok(())
    }
    async fn stage_reply(&mut self, bytes: &[u8]) -> TestResult<p::Reply> {
        let handle = self.writer.ok_or("missing writer")?;
        self.invoke_with_data(
            p::Operation::WriterChunk(p::Handle { handle }),
            bytes.to_vec(),
        )
        .await
    }
    async fn stage(&mut self, bytes: &[u8]) -> TestResult {
        if !matches!(
            self.stage_reply(bytes).await?.result,
            Some(p::Value::Unit(_))
        ) {
            return Err("expected stage acknowledgement".into());
        }
        Ok(())
    }
    async fn commit_reply(&mut self) -> TestResult<p::Reply> {
        let handle = self.writer.take().ok_or("missing writer")?;
        self.invoke(p::Operation::Commit(p::Handle { handle }))
            .await
    }
    async fn commit(&mut self) -> TestResult<d::Entry> {
        let Some(p::Value::Entry(entry)) = self.commit_reply().await?.result else {
            return Err("expected commit metadata".into());
        };
        Ok(p::decode_entry(entry)?)
    }
    async fn abort(&mut self) -> TestResult {
        let handle = self.writer.take().ok_or("missing writer")?;
        if !matches!(
            self.invoke(p::Operation::Abort(p::Handle { handle }))
                .await?
                .result,
            Some(p::Value::Unit(_))
        ) {
            return Err("expected abort acknowledgement".into());
        }
        Ok(())
    }

    async fn stat_entry(&mut self, op: TestResult<Call>) -> TestResult<d::Entry> {
        let Some(p::Value::Entry(entry)) = self.call(op?).await?.result else {
            return Err("expected formal Stat metadata".into());
        };
        Ok(p::decode_entry(entry)?)
    }
}

fn stat_at(path: &str, follow: bool, base: &str, links: IntermediateSymlinks) -> TestResult<Call> {
    let follow = if follow {
        SymlinkAction::FollowWithinInstance
    } else {
        SymlinkAction::Preserve
    };
    Ok(Call::Stat(
        StatOperand::new(PathExpr::parse(path)?, follow),
        LookupContext {
            working_directory: d::StoragePath::parse(base)?,
            intermediate_symlinks: links,
        },
    ))
}
fn stat(path: &str, follow: bool) -> TestResult<Call> {
    stat_at(path, follow, "", IntermediateSymlinks::FollowWithinInstance)
}

fn stat_failure(reply: impl Into<FormalReply>, cause: impl Into<ResolveFailure>) -> TestResult {
    formal_failure(reply, d::Operation::Stat, cause)
}

fn formal_failure(
    reply: impl Into<FormalReply>,
    operation: d::Operation,
    cause: impl Into<ResolveFailure>,
) -> TestResult {
    formal_failure_with_outcome(reply, operation, cause, d::MutationOutcome::Unchanged)
}

fn formal_failure_with_outcome(
    reply: impl Into<FormalReply>,
    operation: d::Operation,
    cause: impl Into<ResolveFailure>,
    outcome: d::MutationOutcome,
) -> TestResult {
    let reply = reply.into();
    let failure = match reply.resolution_error {
        Some(error) => error,
        None => {
            let Some(p::Value::Failure(failure)) = reply.result else {
                return Err("expected formal storage failure".into());
            };
            p::decode_storage_error(failure.storage.ok_or("missing storage failure")?)?
        }
    };
    assert_eq!(failure.operation(), operation);
    assert_eq!(ResolveFailure::from_error(&failure), cause.into());
    assert_eq!(failure.outcome(), outcome);
    Ok(())
}

fn mkdir(path: &str, mode: CreateDirectoryMode) -> TestResult<Call> {
    Ok(Call::CreateDirectory(
        CreateDirectoryOperand::new(PathExpr::parse(path)?, mode),
        Default::default(),
    ))
}
async fn select_listing(
    session: &mut FormalSession,
    request: &ListOperand,
) -> TestResult<d::DirectorySelection> {
    Ok(session
        .storage
        .select_directory(request, Default::default())
        .await?)
}

async fn wire_page(
    session: &mut FormalSession,
    bound: &d::BoundListRequest,
) -> TestResult<(p::Page, Vec<u8>)> {
    let reply = session
        .call(p::StorageOp::List(p::encode_list(bound)))
        .await?;
    let Some(p::Value::Page(page)) = reply.result else {
        return Err("expected page".into());
    };
    Ok((page, reply.data))
}

#[tokio::test]
async fn formal_lookup_preserves_policy_instance_and_link_bytes() -> TestResult {
    let fixture = Fixture::new(64).await?;
    let mut formal = fixture.formal_session().await?;
    formal
        .mkdir(
            "/docs/sub",
            ah_vfs::resolve::operand::CreateDirectoryMode::Recursive,
        )
        .await?;

    use ah_vfs::resolve::operand::CreateDirectoryMode::Recursive;
    let request = ah_vfs::resolve::operand::CreateDirectoryOperand::new(
        ah_vfs::resolve::path::PathExpr::parse("first/../sibling/")?,
        Recursive,
    );
    let reply = formal
        .call(Call::CreateDirectory(
            request,
            LookupContext {
                working_directory: d::StoragePath::parse("docs/sub")?,
                intermediate_symlinks: IntermediateSymlinks::Reject,
            },
        ))
        .await?;
    let Some(p::Value::Entry(directory)) = reply.result else {
        return Err("expected a directory at the bound working directory".into());
    };
    assert_eq!(
        p::decode_entry(directory)?.path().as_str(),
        "docs/sub/sibling"
    );
    assert_eq!(
        formal
            .stat_entry(stat("/docs/sub/first", false))
            .await?
            .entry_type(),
        d::EntryType::Directory
    );

    let request = ah_vfs::resolve::operand::CreateDirectoryOperand::new(
        ah_vfs::resolve::path::PathExpr::parse("alias/child")?,
        Recursive,
    );
    formal_failure(
        formal
            .call(Call::CreateDirectory(
                request,
                LookupContext {
                    working_directory: d::StoragePath::root(),
                    intermediate_symlinks: IntermediateSymlinks::Reject,
                },
            ))
            .await?,
        d::Operation::CreateDirectory,
        ResolveFailure::SymlinkNotAllowed,
    )?;
    let directory = formal.mkdir("alias/child", Recursive).await?;
    assert_eq!(directory.path().as_str(), "docs/child");
    formal_failure(
        formal.call(mkdir("dangling/child", Recursive)?).await?,
        d::Operation::CreateDirectory,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;
    stat_failure(
        formal.call(stat("/missing", false)?).await?,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;

    let root = formal.stat_entry(stat("/../../", true)).await?;
    assert!(root.path().is_root());
    assert_eq!(root.entry_type(), d::EntryType::Directory);
    assert!(root.modified().is_some());

    let link = formal.stat_entry(stat("alias", false)).await?;
    assert_eq!(link.entry_type(), d::EntryType::Symlink);
    assert_eq!(
        formal
            .stat_entry(stat("alias//./sub/..", true))
            .await?
            .path()
            .as_str(),
        "docs"
    );
    assert_eq!(
        formal
            .stat_entry(stat("alias", true))
            .await?
            .path()
            .as_str(),
        "docs"
    );
    assert_eq!(
        formal
            .stat_entry(stat_at(
                "../",
                true,
                "docs/sub",
                IntermediateSymlinks::FollowWithinInstance
            ))
            .await?
            .path()
            .as_str(),
        "docs"
    );
    assert!(
        formal
            .stat_entry(stat("root-link/..", true))
            .await?
            .path()
            .is_root()
    );

    for (path, follow, cause) in [
        ("alias/", false, d::Rejection::NotDirectory.into()),
        ("dangling", true, d::Rejection::NotFound.into()),
        ("loop", true, ResolveFailure::TooManySymlinks),
        ("missing/../docs", true, d::Rejection::NotFound.into()),
    ] {
        stat_failure(formal.call(stat(path, follow)?).await?, cause)?;
    }
    stat_failure(
        formal
            .call(stat_at(
                "alias/sub",
                true,
                "",
                IntermediateSymlinks::Reject,
            )?)
            .await?,
        ResolveFailure::SymlinkNotAllowed,
    )?;
    for (path, bytes) in [("opaque", b"\xff".as_slice()), ("dangling", b"missing")] {
        let reply = formal
            .invoke(p::Operation::BeginLookup(p::LookupHandle {
                handle: formal.handle,
                purpose: "read-link".into(),
            }))
            .await?;
        let Some(p::Value::OpenedLookup(lookup)) = reply.result else {
            return Err("expected lookup binding".into());
        };
        let root = lookup.root.ok_or("expected root observation")?;
        let reply = formal
            .invoke(p::Operation::ObserveChild(p::ObserveChild {
                purpose: "read-link".into(),
                lookup: lookup.handle,
                parent: root.id,
                name: path.into(),
            }))
            .await?;
        let Some(p::Value::ObservedEntry(entry)) = reply.result else {
            return Err("expected entry observation".into());
        };
        let reply = formal
            .invoke(p::Operation::ReadObservedLink(p::ReadObservedLink {
                purpose: "read-link".into(),
                lookup: lookup.handle,
                entry: entry.id,
                final_slot: Some(p::EntrySlot {
                    parent: root.id,
                    name: path.into(),
                    path: path.into(),
                }),
            }))
            .await?;
        assert!(matches!(reply.result, Some(p::Value::Bytes(_))));
        assert_eq!(reply.data, bytes);
    }
    stat_failure(
        formal.call(stat("opaque", true)?).await?,
        ResolveFailure::UnrepresentableLinkTarget,
    )?;

    let error = formal
        .storage
        .write_file(
            WriteOperand::new(
                PathExpr::parse("root-link/..")?,
                d::WriteMode::Replace,
                d::Publication::Atomic,
            ),
            Default::default(),
        )
        .await
        .err()
        .ok_or("root mutation accepted")?;
    assert_eq!(
        ResolveFailure::from_error(&error),
        ResolveFailure::RootProtected
    );
    assert_eq!(error.outcome(), d::MutationOutcome::Unchanged);
    formal_failure(
        formal
            .invoke(p::Operation::BeginLookup(p::LookupHandle {
                handle: formal.handle,
                purpose: "get-permissions".into(),
            }))
            .await?,
        d::Operation::GetPermissions,
        d::Failure::Rejected(d::Rejection::Unsupported),
    )?;
    assert_eq!(fixture.storage.used_bytes(), 0);

    formal.begin("docs/file", d::WriteMode::CreateNew).await?;
    formal.stage(b"scoped read").await?;
    formal.commit().await?;
    let mut relative = formal
        .storage
        .read_file(
            ReadOperand::new(PathExpr::parse("../file")?, d::ReadRange::FULL),
            LookupContext {
                working_directory: d::StoragePath::parse("docs/sub")?,
                ..Default::default()
            },
        )
        .await?;
    assert_eq!(relative.next_chunk().await?, Some(b"scoped read".to_vec()));
    let reader = p::Handle {
        handle: formal.read("/docs/file", d::ReadRange::FULL).await?,
    };
    let released = formal
        .invoke(p::Operation::Release(p::ResourceRef {
            kind: p::ResourceKind::Reader.into(),
            handle: reader.handle,
        }))
        .await?;
    assert!(matches!(released.result, Some(p::Value::Unit(_))));
    formal_failure(
        formal.invoke(p::Operation::ReaderNext(reader)).await?,
        d::Operation::Read,
        d::Failure::Backend(d::BackendFailure::Closed),
    )?;

    let mut invalid = p::encode_entry(&root)?;
    invalid.kind = p::EntryKind::File.into();
    assert!(p::decode_entry(invalid).is_err());

    stat_failure(
        formal
            .call(stat_at(
                ".",
                false,
                "missing",
                IntermediateSymlinks::FollowWithinInstance,
            )?)
            .await?,
        d::Failure::Backend(d::BackendFailure::Stale),
    )?;
    let mut formal = fixture.formal_session().await?;
    let invalid_request = formal
        .invoke(p::Operation::Execute(p::BoundCall {
            lookup: 0,
            operation: None,
        }))
        .await
        .err()
        .ok_or("invalid operand accepted")?;
    assert_eq!(
        invalid_request
            .downcast_ref::<tonic::Status>()
            .map(tonic::Status::code),
        Some(tonic::Code::InvalidArgument)
    );
    Ok(())
}

#[tokio::test]
async fn formal_remove_and_rename_share_bindings_and_apply_the_destination_matrix() -> TestResult {
    use ah_vfs::resolve::{
        operand::CreateDirectoryMode::Recursive, policy::IntermediateSymlinks::FollowWithinInstance,
    };
    use d::{RemoveKind, RenameMode};
    let fixture = Fixture::new(64).await?;
    let mut formal = fixture.formal_session().await?;
    formal.mkdir("/docs/source/child", Recursive).await?;
    formal.mkdir("/docs/empty", Recursive).await?;
    formal.mkdir("/docs/nonempty/child", Recursive).await?;
    formal.begin("/docs/file", d::WriteMode::CreateNew).await?;
    formal.stage(b"original").await?;
    formal.commit().await?;
    let base = d::StoragePath::parse("docs")?;
    let rename = |source: &str, destination: &str, mode| -> TestResult<Call> {
        let request = ah_vfs::resolve::operand::RenameOperand::new(
            ah_vfs::resolve::path::PathExpr::parse(source)?,
            ah_vfs::resolve::path::PathExpr::parse(destination)?,
            mode,
        )?;
        Ok(Call::Rename(
            request,
            LookupContext {
                working_directory: base.clone(),
                intermediate_symlinks: FollowWithinInstance,
            },
        ))
    };
    for (source, destination, mode, reason) in [
        (
            "missing",
            "file/child",
            RenameMode::Replace,
            d::Rejection::NotFound.into(),
        ),
        (
            "source",
            "/alias/source",
            RenameMode::NoReplace,
            d::Rejection::AlreadyExists.into(),
        ),
        (
            "source",
            "file",
            RenameMode::Replace,
            d::Rejection::NotDirectory.into(),
        ),
        (
            "file",
            "empty",
            RenameMode::Replace,
            d::Rejection::IsDirectory.into(),
        ),
        (
            "source",
            "nonempty",
            RenameMode::Replace,
            d::Rejection::DirectoryNotEmpty.into(),
        ),
        (
            "file",
            "missing/",
            RenameMode::Replace,
            d::Rejection::NotDirectory.into(),
        ),
        (
            "/dangling/",
            "/unused",
            RenameMode::NoReplace,
            d::Rejection::NotDirectory.into(),
        ),
        (
            "/step0/file",
            "/step0/copy",
            RenameMode::NoReplace,
            ResolveFailure::TooManySymlinks,
        ),
    ] {
        formal_failure(
            formal.call(rename(source, destination, mode)?).await?,
            d::Operation::Rename,
            reason,
        )?;
    }
    let reply = formal
        .call(rename("source/", "/alias/source", RenameMode::Replace)?)
        .await?;
    assert!(matches!(reply.result, Some(p::Value::Entry(_))));
    assert_eq!(
        formal
            .stat_entry(stat("/docs/source/child", false))
            .await?
            .entry_type(),
        d::EntryType::Directory
    );
    let reply = formal
        .call(rename("source", "/alias/empty/", RenameMode::Replace)?)
        .await?;
    assert!(matches!(reply.result, Some(p::Value::Entry(_))));
    assert_eq!(
        formal
            .stat_entry(stat("/docs/empty/child", false))
            .await?
            .entry_type(),
        d::EntryType::Directory
    );
    stat_failure(
        formal.call(stat("/docs/source", false)?).await?,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;

    let reply = formal
        .call(rename("/dangling", "/moved-link", RenameMode::NoReplace)?)
        .await?;
    assert!(matches!(reply.result, Some(p::Value::Entry(_))));
    let remove = |path: &str, kind| -> TestResult<Call> {
        let request = ah_vfs::resolve::operand::RemoveOperand::new(
            ah_vfs::resolve::path::PathExpr::parse(path)?,
            kind,
        )?;
        Ok(Call::Remove(
            request,
            LookupContext {
                working_directory: base.clone(),
                intermediate_symlinks: FollowWithinInstance,
            },
        ))
    };
    for (path, kind, reason) in [
        (
            "/moved-link/",
            RemoveKind::NonDirectory,
            d::Rejection::NotDirectory,
        ),
        (
            "/moved-link",
            RemoveKind::EmptyDirectory,
            d::Rejection::NotDirectory,
        ),
        ("empty", RemoveKind::NonDirectory, d::Rejection::IsDirectory),
        (
            "empty/",
            RemoveKind::EmptyDirectory,
            d::Rejection::DirectoryNotEmpty,
        ),
    ] {
        formal_failure(
            formal.call(remove(path, kind)?).await?,
            d::Operation::Remove,
            d::Failure::Rejected(reason),
        )?;
    }
    formal
        .remove("/moved-link", RemoveKind::NonDirectory)
        .await?;
    stat_failure(
        formal.call(stat("/missing", false)?).await?,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;
    Ok(())
}

#[tokio::test]
async fn listing_checks_separate_binding_and_rejects_replaced_directory() -> TestResult {
    let fixture = Fixture::new(64).await?;
    let mut listing = fixture.formal_session().await?;
    let mut other = fixture.formal_session().await?;
    for path in ["docs/é", "docs/Report", "docs/report"] {
        listing
            .mkdir(
                path,
                ah_vfs::resolve::operand::CreateDirectoryMode::Recursive,
            )
            .await?;
    }
    let request = ah_vfs::resolve::operand::ListOperand::new(
        ah_vfs::resolve::operand::ListOperandTarget::Path(ah_vfs::resolve::path::PathExpr::parse(
            "alias/",
        )?),
        d::PageSize::new(1)?,
    );
    let selection = select_listing(&mut listing, &request).await?;
    assert_eq!(selection.address().as_str(), "docs");
    let bound = request.bind_selection(selection.clone())?;
    let (wire, data) = wire_page(&mut listing, &bound).await?;
    let first = p::decode_page(&bound, wire.clone(), &data)?;
    assert_eq!(first.entries()[0].path().as_str(), "docs/Report");
    for reported in [
        p::DirectorySelection {
            address: "alias".into(),
            identity: selection.identity().as_bytes().to_vec(),
        },
        p::DirectorySelection {
            address: "docs".into(),
            identity: vec![9; 16],
        },
    ] {
        let mut forged = wire.clone();
        forged.reported = Some(reported);
        assert_eq!(
            p::decode_page(&bound, forged, &data),
            Err(d::ContractViolation::InvalidPage)
        );
    }
    let mut missing_end = wire.clone();
    missing_end.completion = None;
    assert_eq!(
        p::decode_page(&bound, missing_end, &data),
        Err(d::ContractViolation::InvalidPage)
    );
    let mut duplicate = data.clone();
    duplicate.extend_from_slice(&data);
    assert_eq!(
        p::decode_page(&bound, wire.clone(), &duplicate),
        Err(d::ContractViolation::InvalidPage)
    );
    let larger =
        ah_vfs::resolve::operand::ListOperand::new(request.target().clone(), d::PageSize::new(2)?);
    assert_eq!(
        p::decode_page(&larger.bind_selection(selection.clone())?, wire, &duplicate),
        Err(d::ContractViolation::InvalidPage)
    );

    other.remove("alias", d::RemoveKind::NonDirectory).await?;
    listing
        .mkdir(
            "docs/middle",
            ah_vfs::resolve::operand::CreateDirectoryMode::Single,
        )
        .await?;
    let continuation = ah_vfs::resolve::operand::ListOperand::new(
        ah_vfs::resolve::operand::ListOperandTarget::Continue(
            first.continuation().ok_or("first cursor")?,
        ),
        request.size(),
    );
    let resumed = select_listing(&mut listing, &continuation).await?;
    assert_eq!(resumed, selection);
    let bound = continuation.bind_selection(resumed)?;
    let (wire, data) = wire_page(&mut listing, &bound).await?;
    let second = p::decode_page(&bound, wire, &data)?;
    assert_eq!(second.entries()[0].path().as_str(), "docs/middle");

    for path in [
        "docs/Report",
        "docs/middle",
        "docs/report",
        "docs/é",
        "docs",
    ] {
        other.remove(path, d::RemoveKind::EmptyDirectory).await?;
    }
    listing
        .mkdir(
            "/docs",
            ah_vfs::resolve::operand::CreateDirectoryMode::Single,
        )
        .await?;
    formal_failure(
        listing
            .call(p::StorageOp::List(p::encode_list(&bound)))
            .await?,
        d::Operation::List,
        d::Failure::Backend(d::BackendFailure::Stale),
    )?;
    assert!(listing.stat_entry(stat("/", true)).await?.path().is_root());
    Ok(())
}

#[tokio::test]
async fn staging_releases_resources_and_commit_revalidates_slot_and_parent() -> TestResult {
    let fixture = Fixture::new(24).await?;
    let mut first = fixture.formal_session().await?;
    let mut second = fixture.formal_session().await?;
    let mut formal = fixture.formal_session().await?;
    first.begin("/file", d::WriteMode::CreateNew).await?;
    first.stage(b"old").await?;
    stat_failure(
        formal.call(stat("file", true)?).await?,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;
    assert_eq!(
        first.commit().await?.entry_type(),
        d::EntryType::file(Some(3))
    );
    stat_failure(
        formal.call(stat("file/../", true)?).await?,
        d::Failure::Rejected(d::Rejection::NotDirectory),
    )?;

    first.begin("file", d::WriteMode::Replace).await?;
    first.stage(b"staged").await?;
    let retained = formal.read("/file", d::ReadRange::FULL).await?;
    second.begin("file", d::WriteMode::Replace).await?;
    second.stage(b"winner").await?;
    second.stage(b"!").await?;
    assert_eq!(
        second.commit().await?.entry_type(),
        d::EntryType::file(Some(7))
    );
    formal_failure(
        first.commit_reply().await?,
        d::Operation::Write,
        d::Failure::Rejected(d::Rejection::Conflict),
    )?;
    assert_eq!(
        formal.stat_entry(stat("file", true)).await?.entry_type(),
        d::EntryType::file(Some(7))
    );
    assert_eq!(formal.next_chunk(retained).await?, Some(b"old".to_vec()));
    assert_eq!(formal.next_chunk(retained).await?, None);
    let range = formal.read("/file", d::ReadRange::new(1, Some(3))?).await?;
    assert_eq!(formal.next_chunk(range).await?, Some(b"inn".to_vec()));
    assert_eq!(formal.next_chunk(range).await?, None);
    for range in [
        d::ReadRange::new(0, Some(0))?,
        d::ReadRange::new(u64::MAX, None)?,
    ] {
        let empty = formal.read("/file", range).await?;
        assert_eq!(formal.next_chunk(empty).await?, None);
    }
    assert_eq!(fixture.storage.used_bytes(), 7);

    formal
        .mkdir(
            "parent",
            ah_vfs::resolve::operand::CreateDirectoryMode::Single,
        )
        .await?;
    first.begin("parent/child", d::WriteMode::CreateNew).await?;
    first.stage(b"private").await?;
    formal
        .remove("parent", d::RemoveKind::EmptyDirectory)
        .await?;
    formal
        .mkdir(
            "parent",
            ah_vfs::resolve::operand::CreateDirectoryMode::Single,
        )
        .await?;
    formal_failure(
        first.commit_reply().await?,
        d::Operation::Write,
        d::Failure::Rejected(d::Rejection::Conflict),
    )?;
    stat_failure(
        formal.call(stat("parent/child", true)?).await?,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;

    first.begin("abort", d::WriteMode::CreateNew).await?;
    first.stage(b"private").await?;
    first.abort().await?;
    assert_eq!(fixture.storage.used_bytes(), 7);

    first.begin("overflow", d::WriteMode::CreateNew).await?;
    first.stage(b"private").await?;
    formal_failure(
        first.stage_reply(&[0; 24]).await?,
        d::Operation::Write,
        d::Failure::Backend(d::BackendFailure::CapacityExceeded),
    )?;
    formal_failure(
        first.commit_reply().await?,
        d::Operation::Write,
        d::Failure::Backend(d::BackendFailure::Closed),
    )?;
    assert_eq!(fixture.storage.used_bytes(), 7);

    first.begin("dropped", d::WriteMode::CreateNew).await?;
    first.stage(b"private").await?;
    assert_eq!(fixture.storage.used_bytes(), 14);
    drop(first);
    fixture.wait_for_used(7).await?;
    stat_failure(
        formal.call(stat("dropped", true)?).await?,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;

    formal_failure_with_outcome(
        formal
            .call(mkdir(
                "created/../file/child",
                ah_vfs::resolve::operand::CreateDirectoryMode::Recursive,
            )?)
            .await?,
        d::Operation::CreateDirectory,
        d::Failure::Rejected(d::Rejection::NotDirectory),
        d::MutationOutcome::Unknown,
    )?;
    assert_eq!(
        formal.stat_entry(stat("created", true)).await?.entry_type(),
        d::EntryType::Directory
    );
    Ok(())
}
