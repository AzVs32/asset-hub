use ah_vfs_storage as d;
use hyper_util::rt::TokioIo;
use std::{error::Error, future::ready, io, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tonic::{
    Request, Streaming,
    transport::{Channel, Endpoint, Server},
};
use vfs_driver_memory::{Capacity, CapacityUnit, ContractStorage};
use vfs_plugin_grpc_server::{
    ServerLimits,
    storage_contract::{service, wire},
};
use wire::{command::Action, reply::Result as Reply};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const TIMEOUT: Duration = Duration::from_secs(5);

struct Fixture {
    storage: Arc<ContractStorage>,
    channel: Channel,
    task: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}
impl Fixture {
    async fn new(quota: u64) -> TestResult<Self> {
        let links = [
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
        let storage = Arc::new(ContractStorage::new(
            Capacity::new(quota, CapacityUnit::B)?,
            links,
        )?);
        let service = service(storage.clone(), "storage-test", ServerLimits::default())?;
        // Run the actual HTTP/2 and Protobuf stack without listening on a port.
        let (client, server) = tokio::io::duplex(64 * 1024);
        let incoming =
            tokio_stream::once(Ok::<_, io::Error>(server)).chain(tokio_stream::pending());
        let task = tokio::spawn(async move {
            Server::builder()
                .add_service(service)
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
            task,
        })
    }

    async fn session(&self) -> TestResult<Session> {
        let mut client =
            wire::storage_contract_client::StorageContractClient::new(self.channel.clone())
                .max_decoding_message_size(wire::MAX_FRAME_BYTES)
                .max_encoding_message_size(wire::MAX_FRAME_BYTES);
        let (tx, rx) = mpsc::channel(1);
        let mut request = Request::new(ReceiverStream::new(rx));
        request
            .metadata_mut()
            .insert("authorization", "Bearer storage-test".parse()?);
        let replies = tokio::time::timeout(TIMEOUT, client.session(request))
            .await??
            .into_inner();
        Ok(Session { tx, replies })
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

struct Session {
    tx: mpsc::Sender<wire::Command>,
    replies: Streaming<wire::Reply>,
}
impl Session {
    async fn exchange(
        &mut self,
        action: Action,
        reject_intermediate_links: bool,
    ) -> TestResult<Reply> {
        self.tx
            .send(wire::Command {
                action: Some(action),
                reject_intermediate_links,
            })
            .await?;
        Ok(tokio::time::timeout(TIMEOUT, self.replies.message())
            .await??
            .ok_or("session closed")?
            .result
            .ok_or("missing reply")?)
    }
    async fn call(&mut self, action: Action) -> TestResult<Reply> {
        self.exchange(action, false).await
    }
    async fn entry(&mut self, action: Action) -> TestResult<d::Entry> {
        let Reply::Entry(entry) = self.call(action).await? else {
            return Err("expected entry".into());
        };
        Ok(wire::decode_entry(entry)?)
    }
    async fn unit(&mut self, action: Action) -> TestResult {
        if !matches!(self.call(action).await?, Reply::Unit(_)) {
            return Err("expected unit response".into());
        }
        Ok(())
    }
    async fn begin(&mut self, path: &str, mode: wire::WriteMode) -> TestResult {
        self.unit(write(path, mode)).await
    }
    async fn stage(&mut self, bytes: &[u8]) -> TestResult {
        self.unit(Action::Stage(bytes.to_vec())).await
    }
    async fn commit(&mut self) -> TestResult<d::Entry> {
        self.entry(Action::Commit(wire::Unit {})).await
    }
}

fn stat(path: &str, follow: bool) -> Action {
    let policy = if follow {
        wire::FinalLink::Follow
    } else {
        wire::FinalLink::Preserve
    };
    Action::Stat(wire::Stat {
        path: path.into(),
        final_link: policy.into(),
    })
}
fn mkdir(path: &str, recursive: bool) -> Action {
    Action::CreateDirectory(wire::CreateDirectory {
        path: path.into(),
        recursive,
    })
}
fn rmdir(path: &str) -> Action {
    Action::Remove(wire::Remove {
        path: path.into(),
        kind: wire::RemoveKind::EmptyDirectory.into(),
    })
}
fn write(path: &str, mode: wire::WriteMode) -> Action {
    Action::BeginWrite(wire::Write {
        path: path.into(),
        mode: mode.into(),
        publication: wire::Publication::Atomic.into(),
    })
}
fn select(path: &str, page_size: u32) -> Action {
    Action::SelectDirectory(wire::List {
        path: path.into(),
        page_size,
    })
}
fn read(path: &str, offset: u64, length: Option<u64>) -> Action {
    Action::BeginRead(wire::Read {
        path: path.into(),
        offset,
        length,
    })
}
fn error(
    reply: Reply,
    operation: d::Operation,
    cause: d::Failure,
    outcome: d::MutationOutcome,
) -> TestResult {
    let Reply::Failure(wire) = reply else {
        return Err("expected storage failure".into());
    };
    let error = wire::decode_failure(operation, wire)?;
    assert_eq!(
        (error.operation(), error.failure(), error.outcome()),
        (operation, cause, outcome)
    );
    assert!(error.source().is_none());
    Ok(())
}
fn rejected(reply: Reply, operation: d::Operation, cause: d::Rejection) -> TestResult {
    error(
        reply,
        operation,
        d::Failure::Rejected(cause),
        d::MutationOutcome::Unchanged,
    )
}
async fn start_listing(
    session: &mut Session,
    path: &str,
    size: u32,
) -> TestResult<(d::ListRequest, d::DirectorySelection)> {
    let Reply::SelectedDirectory(selection) = session.call(select(path, size)).await? else {
        return Err("expected independent selection".into());
    };
    let request = d::ListRequest::new(
        d::ListTarget::Path(d::PathExpr::parse(path)?),
        d::PageSize::new(usize::try_from(size)?)?,
    );
    Ok((request, wire::decode_selection(selection)?))
}
async fn wire_page(session: &mut Session) -> TestResult<wire::Page> {
    let Reply::Page(page) = session.call(Action::NextPage(wire::Unit {})).await? else {
        return Err("expected page".into());
    };
    Ok(page)
}

#[tokio::test]
async fn paths_links_admission_and_wire_metadata() -> TestResult {
    let fixture = Fixture::new(64).await?;
    let mut session = fixture.session().await?;
    session.entry(mkdir("/docs/sub", true)).await?;
    let root = session.entry(stat("/../../", true)).await?;
    assert!(root.path().is_root());
    assert_eq!(root.entry_type(), d::EntryType::Directory);
    assert!(root.modified().is_some());

    let link = session.entry(stat("alias", false)).await?;
    assert_eq!(link.entry_type(), d::EntryType::Symlink);
    assert_eq!(
        session
            .entry(stat("alias//./sub/..", true))
            .await?
            .path()
            .as_str(),
        "docs"
    );
    rejected(
        session.call(stat("alias/", false)).await?,
        d::Operation::Stat,
        d::Rejection::NotDirectory,
    )?;
    rejected(
        session.exchange(stat("alias/sub", true), true).await?,
        d::Operation::Stat,
        d::Rejection::SymlinkNotAllowed,
    )?;
    assert_eq!(
        session.entry(stat("alias", true)).await?.path().as_str(),
        "docs"
    );

    let Reply::LinkTarget(bytes) = session
        .call(Action::ReadLink(wire::Operand {
            path: "opaque".into(),
        }))
        .await?
    else {
        return Err("expected raw link target".into());
    };
    assert_eq!(bytes, b"\xff");
    error(
        session.call(stat("opaque", true)).await?,
        d::Operation::Stat,
        d::Failure::Backend(d::BackendFailure::UnrepresentableLinkTarget),
        d::MutationOutcome::Unchanged,
    )?;
    for (path, cause) in [
        ("dangling", d::Rejection::NotFound),
        ("loop", d::Rejection::TooManySymlinks),
        ("missing/../docs", d::Rejection::NotFound),
    ] {
        rejected(
            session.call(stat(path, true)).await?,
            d::Operation::Stat,
            cause,
        )?;
    }

    rejected(
        session
            .call(write("root-link/..", wire::WriteMode::Replace))
            .await?,
        d::Operation::Write,
        d::Rejection::RootProtected,
    )?;
    rejected(
        session
            .call(Action::GetPermissions(wire::Operand {
                path: "missing".into(),
            }))
            .await?,
        d::Operation::GetPermissions,
        d::Rejection::Unsupported,
    )?;
    assert_eq!(fixture.storage.used_bytes(), 0);

    let mut invalid = wire::encode_entry(root)?;
    invalid.kind = vfs_plugin_protocol::EntryKind::File.into();
    assert_eq!(
        wire::decode_entry(invalid),
        Err(d::ContractViolation::InvalidEntry)
    );
    let invalid_request = session
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
async fn listing_checks_separate_binding_and_rejects_replaced_directory() -> TestResult {
    let fixture = Fixture::new(64).await?;
    let mut listing = fixture.session().await?;
    let mut other = fixture.session().await?;
    for path in ["docs/é", "docs/Report", "docs/report"] {
        other.entry(mkdir(path, true)).await?;
    }
    let (request, selection) = start_listing(&mut listing, "alias/", 1).await?;
    assert_eq!(selection.address().as_str(), "docs");
    let bound = request.bind_selection(selection.clone())?;
    let first_wire = wire_page(&mut listing).await?;
    let first = wire::decode_page(&bound, first_wire.clone())?;
    assert_eq!(first.entries()[0].path().as_str(), "docs/Report");

    let mut forged = first_wire.clone();
    forged.reported = Some(wire::Selection {
        address: "alias".into(),
        identity: selection.identity().as_bytes().to_vec(),
    });
    assert_eq!(
        wire::decode_page(&bound, forged),
        Err(d::ContractViolation::InvalidPage)
    );
    let mut missing_end = first_wire.clone();
    missing_end.completion = None;
    assert_eq!(
        wire::decode_page(&bound, missing_end),
        Err(d::ContractViolation::InvalidPage)
    );
    let mut too_many = first_wire;
    let duplicate = too_many.entries.clone();
    too_many.entries.extend(duplicate);
    assert_eq!(
        wire::decode_page(&bound, too_many),
        Err(d::ContractViolation::InvalidPage)
    );

    let continuation = d::ListRequest::new(
        d::ListTarget::Continue(first.continuation().ok_or("first cursor")?),
        request.size(),
    );
    let bound = continuation.bind_selection(selection)?;
    let second = wire::decode_page(&bound, wire_page(&mut listing).await?)?;
    assert_eq!(second.entries()[0].path().as_str(), "docs/report");

    for path in ["docs/Report", "docs/report", "docs/é", "docs"] {
        other.unit(rmdir(path)).await?;
    }
    other.entry(mkdir("/docs", false)).await?;
    error(
        listing.call(Action::NextPage(wire::Unit {})).await?,
        d::Operation::List,
        d::Failure::Backend(d::BackendFailure::Stale),
        d::MutationOutcome::Unchanged,
    )?;
    Ok(())
}

#[tokio::test]
async fn staging_releases_resources_and_commit_revalidates_slot_and_parent() -> TestResult {
    let fixture = Fixture::new(24).await?;
    let mut first = fixture.session().await?;
    let mut second = fixture.session().await?;
    first.begin("/file", wire::WriteMode::CreateNew).await?;
    first.stage(b"old").await?;
    rejected(
        second.call(stat("file", true)).await?,
        d::Operation::Stat,
        d::Rejection::NotFound,
    )?;
    assert_eq!(
        first.commit().await?.entry_type(),
        d::EntryType::file(Some(3))
    );
    rejected(
        second.call(stat("file/../", true)).await?,
        d::Operation::Stat,
        d::Rejection::NotDirectory,
    )?;

    first.begin("file", wire::WriteMode::Replace).await?;
    first.stage(b"staged").await?;
    first.unit(read("file", 0, None)).await?;
    second.begin("file", wire::WriteMode::Replace).await?;
    second.stage(b"winner").await?;
    second.stage(b"!").await?;
    assert_eq!(
        second.commit().await?.entry_type(),
        d::EntryType::file(Some(7))
    );
    rejected(
        first.call(Action::Commit(wire::Unit {})).await?,
        d::Operation::Write,
        d::Rejection::Conflict,
    )?;
    assert_eq!(
        first.entry(stat("file", true)).await?.entry_type(),
        d::EntryType::file(Some(7))
    );
    assert!(matches!(
        first.call(Action::ReadNext(wire::Unit {})).await?,
        Reply::Chunk(bytes) if bytes == b"old"
    ));
    assert!(matches!(
        first.call(Action::ReadNext(wire::Unit {})).await?,
        Reply::EndOfFile(_)
    ));
    second.unit(read("file", 1, Some(3))).await?;
    assert!(matches!(
        second.call(Action::ReadNext(wire::Unit {})).await?,
        Reply::Chunk(bytes) if bytes == b"inn"
    ));
    assert!(matches!(
        second.call(Action::ReadNext(wire::Unit {})).await?,
        Reply::EndOfFile(_)
    ));
    assert_eq!(fixture.storage.used_bytes(), 7);

    second.entry(mkdir("parent", false)).await?;
    first
        .begin("parent/child", wire::WriteMode::CreateNew)
        .await?;
    first.stage(b"private").await?;
    second.unit(rmdir("parent")).await?;
    second.entry(mkdir("parent", false)).await?;
    rejected(
        first.call(Action::Commit(wire::Unit {})).await?,
        d::Operation::Write,
        d::Rejection::Conflict,
    )?;
    rejected(
        second.call(stat("parent/child", true)).await?,
        d::Operation::Stat,
        d::Rejection::NotFound,
    )?;

    first.begin("abort", wire::WriteMode::CreateNew).await?;
    first.stage(b"private").await?;
    first.unit(Action::Abort(wire::Unit {})).await?;
    assert_eq!(fixture.storage.used_bytes(), 7);

    first.begin("overflow", wire::WriteMode::CreateNew).await?;
    first.stage(b"private").await?;
    error(
        first.call(Action::Stage(vec![0; 24])).await?,
        d::Operation::Write,
        d::Failure::Backend(d::BackendFailure::CapacityExceeded),
        d::MutationOutcome::Unchanged,
    )?;
    error(
        first.call(Action::Commit(wire::Unit {})).await?,
        d::Operation::Write,
        d::Failure::Backend(d::BackendFailure::Closed),
        d::MutationOutcome::Unchanged,
    )?;
    assert_eq!(fixture.storage.used_bytes(), 7);

    first.begin("dropped", wire::WriteMode::CreateNew).await?;
    first.stage(b"private").await?;
    assert_eq!(fixture.storage.used_bytes(), 14);
    drop(first);
    fixture.wait_for_used(7).await?;
    rejected(
        second.call(stat("dropped", true)).await?,
        d::Operation::Stat,
        d::Rejection::NotFound,
    )?;

    error(
        second.call(mkdir("created/../file/child", true)).await?,
        d::Operation::CreateDirectory,
        d::Failure::Rejected(d::Rejection::NotDirectory),
        d::MutationOutcome::Unknown,
    )?;
    assert_eq!(
        second.entry(stat("created", true)).await?.entry_type(),
        d::EntryType::Directory
    );
    Ok(())
}
