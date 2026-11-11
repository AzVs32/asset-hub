use ah_vfs::application::{driver_info::DriverInfo, port::*};
use ah_vfs_storage as d;
use hyper_util::rt::TokioIo;
use std::{error::Error, future::ready, io, sync::Arc, time::Duration};
use tokio::sync::mpsc;
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
        let formal = vfs_plugin_grpc_server::service(dispatcher, "storage-test")?;
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
            channel,
            generation,
            task,
        })
    }

    async fn formal_session(&self) -> TestResult<FormalSession> {
        use vfs_plugin_grpc_server::wire::plugin_client::PluginClient;
        let mut client = PluginClient::new(self.channel.clone());
        let (tx, rx) = mpsc::channel(1);
        tx.send(p::to_request(
            &self.generation,
            &p::Operation::Open(p::Open {
                id: "fixture".into(),
                config: Some(Default::default()),
            }),
            vec![],
        )?)
        .await?;
        let mut request = Request::new(ReceiverStream::new(rx));
        request
            .metadata_mut()
            .insert("authorization", "Bearer storage-test".parse()?);
        let mut replies = tokio::time::timeout(TIMEOUT, client.session(request))
            .await??
            .into_inner();
        let opened = tokio::time::timeout(TIMEOUT, replies.message())
            .await??
            .ok_or("missing Open response")?;
        let Some(p::Value::Storage(opened)) = opened.result else {
            return Err("formal session did not open".into());
        };
        Ok(FormalSession {
            tx,
            replies,
            handle: opened.handle,
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

struct FormalSession {
    tx: mpsc::Sender<p::Request>,
    replies: Streaming<p::Reply>,
    handle: u64,
    writer: Option<u64>,
}
impl FormalSession {
    async fn remove(&mut self, path: &str, kind: d::RemoveKind) -> TestResult {
        let request = d::RemoveRequest::new(d::PathExpr::parse(path)?, kind)?;
        let operation = p::StorageOp::Remove(p::encode_remove(
            &request,
            &d::StoragePath::root(),
            d::IntermediateSymlinks::FollowWithinInstance,
        ));
        if !matches!(self.call(operation).await?.result, Some(p::Value::Unit(_))) {
            return Err("expected removal success".into());
        }
        Ok(())
    }

    async fn mkdir(&mut self, path: &str, mode: d::CreateDirectoryMode) -> TestResult<d::Entry> {
        let reply = self.call(mkdir(path, mode)?).await?;
        let Some(p::Value::Entry(entry)) = reply.result else {
            return Err(format!("expected created directory: {:?}", reply.result).into());
        };
        Ok(p::decode_entry(entry)?)
    }

    async fn call(&mut self, operation: p::StorageOp) -> TestResult<p::Reply> {
        self.invoke(p::Operation::Storage(p::StorageCall {
            handle: self.handle,
            operation: Some(operation),
        }))
        .await
    }

    async fn invoke(&mut self, operation: p::Operation) -> TestResult<p::Reply> {
        self.invoke_with_data(operation, vec![]).await
    }

    async fn invoke_with_data(
        &mut self,
        operation: p::Operation,
        data: Vec<u8>,
    ) -> TestResult<p::Reply> {
        // Deliberately let the server validate even malformed test requests.
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

    async fn read(&mut self, path: &str, range: d::ReadRange) -> TestResult<u64> {
        let request = d::ReadRequest::new(d::PathExpr::parse(path)?, range);
        let reply = self
            .invoke(p::Operation::Read(p::encode_read(
                self.handle,
                &request,
                &d::StoragePath::root(),
                d::IntermediateSymlinks::FollowWithinInstance,
            )))
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
        let reply = self.invoke(write(self.handle, path, mode)?).await?;
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

    async fn stat_entry(&mut self, op: p::StorageOp) -> TestResult<d::Entry> {
        let Some(p::Value::Entry(entry)) = self.call(op).await?.result else {
            return Err("expected formal Stat metadata".into());
        };
        Ok(p::decode_entry(entry)?)
    }
}

fn lookup(path: &str, base: &str, links: p::IntermediateLink) -> p::Lookup {
    p::Lookup {
        operand: path.into(),
        working_directory: base.into(),
        intermediate_links: links.into(),
    }
}
fn stat(path: &str, follow: bool) -> p::StorageOp {
    p::StorageOp::Stat(p::Stat {
        lookup: Some(lookup(path, "", p::IntermediateLink::FollowIntermediate)),
        final_link: if follow {
            p::FinalLink::FollowFinal
        } else {
            p::FinalLink::PreserveFinal
        }
        .into(),
    })
}

fn stat_failure(reply: p::Reply, cause: d::Failure) -> TestResult {
    formal_failure(reply, d::Operation::Stat, cause)
}

fn formal_failure(reply: p::Reply, operation: d::Operation, cause: d::Failure) -> TestResult {
    formal_failure_with_outcome(reply, operation, cause, d::MutationOutcome::Unchanged)
}

fn formal_failure_with_outcome(
    reply: p::Reply,
    operation: d::Operation,
    cause: d::Failure,
    outcome: d::MutationOutcome,
) -> TestResult {
    let Some(p::Value::Failure(failure)) = reply.result else {
        return Err("expected formal storage failure".into());
    };
    let failure = p::decode_storage_error(failure.storage.ok_or("missing storage failure")?)?;
    assert_eq!(failure.operation(), operation);
    assert_eq!(failure.failure(), cause);
    assert_eq!(failure.outcome(), outcome);
    Ok(())
}

fn mkdir(path: &str, mode: d::CreateDirectoryMode) -> TestResult<p::StorageOp> {
    let request = d::CreateDirectoryRequest::new(d::PathExpr::parse(path)?, mode);
    Ok(p::StorageOp::CreateDirectory(p::encode_create_directory(
        &request,
        &d::StoragePath::root(),
        d::IntermediateSymlinks::FollowWithinInstance,
    )))
}
fn write(handle: u64, path: &str, mode: d::WriteMode) -> TestResult<p::Operation> {
    Ok(p::Operation::Write(p::encode_write(
        handle,
        &d::WriteRequest::new(d::PathExpr::parse(path)?, mode, d::Publication::Atomic),
        &d::StoragePath::root(),
        d::IntermediateSymlinks::FollowWithinInstance,
    )))
}
async fn select_listing(
    session: &mut FormalSession,
    request: &d::ListRequest,
) -> TestResult<d::DirectorySelection> {
    let reply = session
        .call(p::StorageOp::SelectDirectory(p::encode_select_directory(
            request,
            &d::StoragePath::root(),
            d::IntermediateSymlinks::FollowWithinInstance,
        )))
        .await?;
    let Some(p::Value::SelectedDirectory(selection)) = reply.result else {
        return Err("expected independent selection".into());
    };
    Ok(p::decode_selection(selection)?)
}

async fn wire_page(
    session: &mut FormalSession,
    bound: &d::BoundListRequest<'_>,
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
        .mkdir("/docs/sub", d::CreateDirectoryMode::Recursive)
        .await?;

    use d::CreateDirectoryMode::Recursive;
    let request =
        d::CreateDirectoryRequest::new(d::PathExpr::parse("first/../sibling/")?, Recursive);
    let reply = formal
        .call(p::StorageOp::CreateDirectory(p::encode_create_directory(
            &request,
            &d::StoragePath::parse("docs/sub")?,
            d::IntermediateSymlinks::Reject,
        )))
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

    let request = d::CreateDirectoryRequest::new(d::PathExpr::parse("alias/child")?, Recursive);
    formal_failure(
        formal
            .call(p::StorageOp::CreateDirectory(p::encode_create_directory(
                &request,
                &d::StoragePath::root(),
                d::IntermediateSymlinks::Reject,
            )))
            .await?,
        d::Operation::CreateDirectory,
        d::Failure::Rejected(d::Rejection::SymlinkNotAllowed),
    )?;
    let directory = formal.mkdir("alias/child", Recursive).await?;
    assert_eq!(directory.path().as_str(), "docs/child");
    formal_failure(
        formal.call(mkdir("dangling/child", Recursive)?).await?,
        d::Operation::CreateDirectory,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;
    stat_failure(
        formal.call(stat("/missing", false)).await?,
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
            .stat_entry(p::StorageOp::Stat(p::Stat {
                lookup: Some(lookup(
                    "../",
                    "docs/sub",
                    p::IntermediateLink::FollowIntermediate
                )),
                final_link: p::FinalLink::FollowFinal.into(),
            }))
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
        ("alias/", false, d::Rejection::NotDirectory),
        ("dangling", true, d::Rejection::NotFound),
        ("loop", true, d::Rejection::TooManySymlinks),
        ("missing/../docs", true, d::Rejection::NotFound),
    ] {
        stat_failure(
            formal.call(stat(path, follow)).await?,
            d::Failure::Rejected(cause),
        )?;
    }
    stat_failure(
        formal
            .call(p::StorageOp::Stat(p::Stat {
                lookup: Some(lookup(
                    "alias/sub",
                    "",
                    p::IntermediateLink::RejectIntermediate,
                )),
                final_link: p::FinalLink::FollowFinal.into(),
            }))
            .await?,
        d::Failure::Rejected(d::Rejection::SymlinkNotAllowed),
    )?;
    for (path, bytes) in [("opaque", b"\xff".as_slice()), ("dangling", b"missing")] {
        let reply = formal
            .call(p::StorageOp::BindLink(p::BindLink {
                lookup: Some(lookup(path, "", p::IntermediateLink::FollowIntermediate)),
            }))
            .await?;
        let Some(p::Value::BoundLink(link)) = reply.result else {
            return Err("expected bound link".into());
        };
        let reply = formal.invoke(p::Operation::ReadBoundLink(p::Handle { handle: link.handle })).await?;
        assert!(matches!(reply.result, Some(p::Value::Bytes(_))));
        assert_eq!(reply.data, bytes);
    }
    stat_failure(
        formal.call(stat("opaque", true)).await?,
        d::Failure::Backend(d::BackendFailure::UnrepresentableLinkTarget),
    )?;

    formal_failure(
        formal
            .invoke(write(formal.handle, "root-link/..", d::WriteMode::Replace)?)
            .await?,
        d::Operation::Write,
        d::Failure::Rejected(d::Rejection::RootProtected),
    )?;
    let request = d::AttributeRequest::new(
        d::PathExpr::parse("missing")?,
        d::AttributeAction::GetPermissions,
    );
    let (wire, _) = p::encode_attribute(
        request,
        &d::StoragePath::root(),
        d::IntermediateSymlinks::FollowWithinInstance,
    )?;
    formal_failure(
        formal.call(wire).await?,
        d::Operation::GetPermissions,
        d::Failure::Rejected(d::Rejection::Unsupported),
    )?;
    assert_eq!(fixture.storage.used_bytes(), 0);

    formal.begin("docs/file", d::WriteMode::CreateNew).await?;
    formal.stage(b"scoped read").await?;
    formal.commit().await?;
    let read = p::encode_read(
        formal.handle,
        &d::ReadRequest::new(d::PathExpr::parse("../file")?, d::ReadRange::FULL),
        &d::StoragePath::parse("docs/sub")?,
        d::IntermediateSymlinks::FollowWithinInstance,
    );
    let Some(p::Value::Reader(reader)) = formal.invoke(p::Operation::Read(read)).await?.result
    else {
        return Err("expected relative lookup to open a reader".into());
    };
    assert_eq!(
        formal.next_chunk(reader.handle).await?,
        Some(b"scoped read".to_vec())
    );
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
            .call(p::StorageOp::Stat(p::Stat {
                lookup: Some(lookup(
                    ".",
                    "missing",
                    p::IntermediateLink::FollowIntermediate,
                )),
                final_link: p::FinalLink::PreserveFinal.into(),
            }))
            .await?,
        d::Failure::Backend(d::BackendFailure::Stale),
    )?;
    let mut formal = fixture.formal_session().await?;
    let invalid_request = formal
        .call(stat("", true))
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
    use d::{
        CreateDirectoryMode::Recursive, IntermediateSymlinks::FollowWithinInstance, RemoveKind,
        RenameMode,
    };
    let fixture = Fixture::new(64).await?;
    let mut formal = fixture.formal_session().await?;
    formal.mkdir("/docs/source/child", Recursive).await?;
    formal.mkdir("/docs/empty", Recursive).await?;
    formal.mkdir("/docs/nonempty/child", Recursive).await?;
    formal.begin("/docs/file", d::WriteMode::CreateNew).await?;
    formal.stage(b"original").await?;
    formal.commit().await?;
    let base = d::StoragePath::parse("docs")?;
    let rename = |source: &str, destination: &str, mode| -> TestResult<p::StorageOp> {
        let request = d::RenameRequest::new(
            d::PathExpr::parse(source)?,
            d::PathExpr::parse(destination)?,
            mode,
        )?;
        Ok(p::StorageOp::Rename(p::encode_rename(
            &request,
            &base,
            FollowWithinInstance,
        )))
    };
    for (source, destination, mode, reason) in [
        (
            "missing",
            "file/child",
            RenameMode::Replace,
            d::Rejection::NotFound,
        ),
        (
            "source",
            "/alias/source",
            RenameMode::NoReplace,
            d::Rejection::AlreadyExists,
        ),
        (
            "source",
            "file",
            RenameMode::Replace,
            d::Rejection::NotDirectory,
        ),
        (
            "file",
            "empty",
            RenameMode::Replace,
            d::Rejection::IsDirectory,
        ),
        (
            "source",
            "nonempty",
            RenameMode::Replace,
            d::Rejection::DirectoryNotEmpty,
        ),
        (
            "file",
            "missing/",
            RenameMode::Replace,
            d::Rejection::NotDirectory,
        ),
        (
            "/dangling/",
            "/unused",
            RenameMode::NoReplace,
            d::Rejection::NotDirectory,
        ),
        (
            "/step0/file",
            "/step0/copy",
            RenameMode::NoReplace,
            d::Rejection::TooManySymlinks,
        ),
    ] {
        formal_failure(
            formal.call(rename(source, destination, mode)?).await?,
            d::Operation::Rename,
            d::Failure::Rejected(reason),
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
        formal.call(stat("/docs/source", false)).await?,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;

    let reply = formal
        .call(rename("/dangling", "/moved-link", RenameMode::NoReplace)?)
        .await?;
    assert!(matches!(reply.result, Some(p::Value::Entry(_))));
    let remove = |path: &str, kind| -> TestResult<p::StorageOp> {
        let request = d::RemoveRequest::new(d::PathExpr::parse(path)?, kind)?;
        Ok(p::StorageOp::Remove(p::encode_remove(
            &request,
            &base,
            FollowWithinInstance,
        )))
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
        formal.call(stat("/missing", false)).await?,
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
            .mkdir(path, d::CreateDirectoryMode::Recursive)
            .await?;
    }
    let request = d::ListRequest::new(
        d::ListTarget::Path(d::PathExpr::parse("alias/")?),
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
    let larger = d::ListRequest::new(request.target().clone(), d::PageSize::new(2)?);
    assert_eq!(
        p::decode_page(&larger.bind_selection(selection.clone())?, wire, &duplicate),
        Err(d::ContractViolation::InvalidPage)
    );

    other.remove("alias", d::RemoveKind::NonDirectory).await?;
    listing
        .mkdir("docs/middle", d::CreateDirectoryMode::Single)
        .await?;
    let continuation = d::ListRequest::new(
        d::ListTarget::Continue(first.continuation().ok_or("first cursor")?),
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
        .mkdir("/docs", d::CreateDirectoryMode::Single)
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
        formal.call(stat("file", true)).await?,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;
    assert_eq!(
        first.commit().await?.entry_type(),
        d::EntryType::file(Some(3))
    );
    stat_failure(
        formal.call(stat("file/../", true)).await?,
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
        .mkdir("parent", d::CreateDirectoryMode::Single)
        .await?;
    first.begin("parent/child", d::WriteMode::CreateNew).await?;
    first.stage(b"private").await?;
    formal
        .remove("parent", d::RemoveKind::EmptyDirectory)
        .await?;
    formal
        .mkdir("parent", d::CreateDirectoryMode::Single)
        .await?;
    formal_failure(
        first.commit_reply().await?,
        d::Operation::Write,
        d::Failure::Rejected(d::Rejection::Conflict),
    )?;
    stat_failure(
        formal.call(stat("parent/child", true)).await?,
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
        formal.call(stat("dropped", true)).await?,
        d::Failure::Rejected(d::Rejection::NotFound),
    )?;

    formal_failure_with_outcome(
        formal
            .call(mkdir(
                "created/../file/child",
                d::CreateDirectoryMode::Recursive,
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
