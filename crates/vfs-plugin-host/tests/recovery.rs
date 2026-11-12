use ah_vfs::application::port::*;
use ah_vfs::domain::InstanceId;
use ah_vfs::resolve::LookupContext;
use ah_vfs::resolve::StorageOperations;
use ah_vfs::resolve::path::RootedPath;
use ah_vfs_storage::{BackendFailure, Failure, Operation, Rejection, StorageError};
use ah_vfs_storage::{Entry, StoragePath};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use vfs_plugin_host::{
    OpenFuture, OpenedStorage, PluginDriver, SessionTransport, Transport, TransportFuture,
};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Peer {
    generation: AtomicUsize,
    sessions_disconnected: Mutex<Arc<AtomicBool>>,
    initialized: Mutex<Vec<(usize, p::Object)>>,
    descriptions: AtomicUsize,
    next: AtomicU64,
    changed: AtomicBool,
    fail_describe: AtomicBool,
    commits: AtomicUsize,
    suspend_initialize: AtomicBool,
    initializing: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}
impl Peer {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            generation: AtomicUsize::new(1),
            sessions_disconnected: Mutex::new(Arc::new(AtomicBool::new(false))),
            initialized: Mutex::new(vec![]),
            descriptions: AtomicUsize::new(0),
            next: AtomicU64::new(2),
            changed: AtomicBool::new(false),
            fail_describe: AtomicBool::new(false),
            commits: AtomicUsize::new(0),
            suspend_initialize: AtomicBool::new(false),
            initializing: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        })
    }
    fn restart(&self) {
        let mut sessions = self
            .sessions_disconnected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.generation.fetch_add(1, Ordering::SeqCst);
        sessions.store(true, Ordering::Release);
        *sessions = Arc::new(AtomicBool::new(false));
    }
    fn description(&self) -> p::Description {
        p::Description {
            major: p::PROTOCOL_MAJOR,
            minor: p::PROTOCOL_MINOR,
            required_features: vec![],
            features: p::PROTOCOL_FEATURES
                .iter()
                .map(|feature| (*feature).into())
                .collect(),
            required_host_capabilities: vec![],
            instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
            generation: self.generation.load(Ordering::SeqCst).to_string(),
            key: "recovery".into(),
            name: "Recovery peer".into(),
            version: "1.0.0".into(),
            child_mounts: true,
            capabilities: if self.changed.load(Ordering::SeqCst) {
                vec![]
            } else {
                vec![
                    p::Capability::Stat as i32,
                    p::Capability::ReadLink as i32,
                    p::Capability::RangeRead as i32,
                    p::Capability::WriteNew as i32,
                ]
            },

            settings_schema: None,
            instance_schema: None,
            config_version: 1,
        }
    }
}
impl vfs_plugin_host::TransportFactory for Peer {
    fn connect(self: Arc<Self>) -> vfs_plugin_host::ConnectFuture {
        Box::pin(async move { Ok(self as Arc<dyn Transport>) })
    }
}
impl Transport for Peer {
    fn open_session(self: Arc<Self>, generation: &str, _: p::Open) -> OpenFuture<'_> {
        Box::pin(async move {
            let sessions = self
                .sessions_disconnected
                .lock()
                .map_err(|_| vfs_plugin_host::convert::protocol_violation())?;
            if generation != self.generation.load(Ordering::SeqCst).to_string() {
                return Err(ah_vfs::application::driver_error::LifecycleError::new(
                    ah_vfs::application::driver_error::LifecycleFailure::Stale,
                )
                .into());
            }
            Ok(OpenedStorage {
                transport: Arc::new(PeerSession {
                    peer: self.clone(),
                    disconnected: sessions.clone(),
                }),
                generation: generation.into(),
                storage: p::Storage {
                    handle: 1,
                    info: Some(p::StorageInfo { chunk_size: 65536 }),
                },
            })
        })
    }
}
struct PeerSession {
    peer: Arc<Peer>,
    disconnected: Arc<AtomicBool>,
}
impl SessionTransport for PeerSession {
    fn disconnected(&self) -> Option<Arc<AtomicBool>> {
        Some(self.disconnected.clone())
    }

    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        self.peer.call(generation, op, data)
    }
}
impl SessionTransport for Peer {
    fn call<'a>(&'a self, generation: &'a str, op: Op, _data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            let current = self.generation.load(Ordering::SeqCst);
            if matches!(op, Op::Describe(p::Empty {})) {
                self.descriptions.fetch_add(1, Ordering::SeqCst);
                if self.fail_describe.swap(false, Ordering::SeqCst) {
                    return Err(ah_vfs::application::driver_error::LifecycleError::new(
                        ah_vfs::application::driver_error::LifecycleFailure::Unavailable,
                    )
                    .into());
                }
                return Ok(p::Reply::new(Value::Description(self.description().into())));
            }
            if generation != current.to_string() {
                return Err(vfs_plugin_host::convert::call_error(
                    &op,
                    Failure::Backend(BackendFailure::Stale),
                    false,
                    None,
                ));
            }
            let value = match op {
                Op::Initialize(p::Initialize {
                    settings: Some(settings),
                }) => {
                    self.initialized
                        .lock()
                        .map_err(|_| vfs_plugin_host::convert::protocol_violation())?
                        .push((current, settings));
                    if self.suspend_initialize.load(Ordering::SeqCst) {
                        self.initializing.notify_one();
                        self.resume.notified().await;
                    }
                    Value::Unit(p::Empty {})
                }
                Op::BeginLookup(p::LookupHandle { handle: 1, .. }) => {
                    Value::OpenedLookup(p::OpenedLookup {
                        handle: self.next.fetch_add(1, Ordering::SeqCst),
                        root: Some(observation(100, "", p::EntryKind::Dir)),
                    })
                }
                Op::ObserveChild(p::ObserveChild {
                    lookup,
                    parent,
                    name,
                    purpose,
                }) => {
                    let operation = p::decode_lookup_purpose(&purpose)
                        .ok_or_else(vfs_plugin_host::convert::protocol_violation)?;
                    let observed = match (parent, name.as_str()) {
                        (100, "docs") => {
                            assert_eq!(lookup, 2);
                            observation(101, "docs", p::EntryKind::Dir)
                        }
                        (101, "sub") => observation(102, "docs/sub", p::EntryKind::Dir),
                        (101, "link") => observation(103, "docs/link", p::EntryKind::Symlink),
                        (100, "file") if operation == Operation::Write => {
                            return Err(StorageError::without_effects(
                                operation,
                                Failure::Rejected(Rejection::NotFound),
                                None,
                            )
                            .into());
                        }
                        (100, "file") => {
                            let mut entry = observation(104, "file", p::EntryKind::File);
                            if let Some(entry) = &mut entry.entry {
                                entry.size = Some(current as u64);
                            }
                            entry
                        }
                        _ => return Err(vfs_plugin_host::convert::protocol_violation()),
                    };
                    Value::ObservedEntry(observed)
                }
                Op::ReadObservedLink(p::ReadObservedLink {
                    lookup,
                    entry,
                    final_slot,
                    ..
                }) => {
                    assert_eq!((lookup, entry), (2, 103));
                    assert_eq!(
                        final_slot,
                        Some(p::EntrySlot {
                            parent: 101,
                            name: "link".into(),
                            path: "docs/link".into(),
                        })
                    );
                    return Ok(p::Reply {
                        result: Some(Value::Bytes(p::Empty {})),
                        data: vec![0xff, b'/'],
                    });
                }
                Op::Execute(call) => match call.operation {
                    Some(p::BoundOp::Stat(_)) => Value::Entry(p::Entry {
                        path: "file".into(),
                        kind: p::EntryKind::File.into(),
                        size: Some(current as u64),
                        modified: None,
                    }),
                    Some(p::BoundOp::Read(_)) => Value::Reader(p::Handle {
                        handle: self.next.fetch_add(1, Ordering::SeqCst),
                    }),
                    Some(p::BoundOp::Write(_)) => Value::Writer(p::Handle {
                        handle: self.next.fetch_add(1, Ordering::SeqCst),
                    }),
                    _ => return Err(vfs_plugin_host::convert::protocol_violation()),
                },
                Op::Commit(_) => {
                    self.commits.fetch_add(1, Ordering::SeqCst);
                    self.restart();
                    return Err(StorageError::after_dispatch(
                        Operation::Write,
                        Failure::Backend(BackendFailure::Unavailable),
                        None,
                    )
                    .into());
                }
                _ => Value::Unit(p::Empty {}),
            };
            Ok(p::Reply::new(value))
        })
    }
}
fn observation(id: u64, path: &str, kind: p::EntryKind) -> p::ObservedEntry {
    p::ObservedEntry {
        id,
        entry: Some(p::Entry {
            path: path.into(),
            kind: kind.into(),
            size: None,
            modified: None,
        }),
    }
}

async fn connect(peer: &Arc<Peer>) -> Result<Arc<PluginDriver>> {
    let driver = Arc::new(PluginDriver::connect(peer.clone()).await?);
    driver
        .initialize([("setting".into(), "original".into())].into())
        .await?;
    Ok(driver)
}
async fn open(driver: &PluginDriver) -> Result<Arc<dyn Storage>> {
    Ok(driver
        .open(OpenContext::new(
            InstanceId::parse("instance")?,
            Default::default(),
        ))
        .await?)
}
async fn stat(storage: &dyn Storage) -> std::result::Result<Entry, StorageError> {
    let path = StoragePath::parse("file").map_err(|cause| {
        StorageError::without_effects(
            Operation::Stat,
            Failure::Rejected(Rejection::UnsupportedName),
            Some(Box::new(cause)),
        )
    })?;
    storage
        .stat_entry(
            ah_vfs::resolve::operand::StatOperand::new(
                path.to_rooted_expr(),
                ah_vfs::resolve::policy::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await
}

#[tokio::test]
async fn new_opens_share_recovery_and_old_resources_stay_stale() -> Result {
    let peer = Peer::new();
    let driver = connect(&peer).await?;
    let old = open(&driver).await?;
    let mut reader = old
        .read_file(
            ah_vfs::resolve::operand::ReadOperand::new(
                StoragePath::parse("file")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    peer.restart();
    assert_eq!(
        stat(old.as_ref())
            .await
            .err()
            .ok_or("old generation")?
            .failure(),
        Failure::Backend(BackendFailure::Stale)
    );
    assert!(old.is_stale());
    let mut opens = vec![];
    for _ in 0..8 {
        let driver = driver.clone();
        let id = InstanceId::parse("instance")?;
        opens.push(tokio::spawn(async move {
            driver.open(OpenContext::new(id, Default::default())).await
        }));
    }
    for task in opens {
        let storage = task.await??;
        assert_eq!(
            stat(storage.as_ref()).await?.entry_type(),
            ah_vfs_storage::EntryType::file(Some(2))
        );
    }
    assert_eq!(peer.descriptions.load(Ordering::SeqCst), 2);
    let initialized = peer
        .initialized
        .lock()
        .map_err(|_| "settings lock")?
        .clone();
    assert_eq!(initialized.len(), 2);
    assert_eq!(initialized[0].1, initialized[1].1);
    assert_eq!(
        reader
            .next_chunk()
            .await
            .err()
            .ok_or("old reader")?
            .failure(),
        Failure::Backend(BackendFailure::Stale)
    );
    assert_eq!(
        stat(old.as_ref())
            .await
            .err()
            .ok_or("old storage")?
            .failure(),
        Failure::Backend(BackendFailure::Stale)
    );
    driver.reconnect().await?;
    // A fresh control transport replays settings even for the same backend.
    assert_eq!(
        peer.initialized.lock().map_err(|_| "settings lock")?.len(),
        3
    );
    Ok(())
}

#[tokio::test]
async fn unknown_commit_is_not_replayed_during_recovery() -> Result {
    let peer = Peer::new();
    let driver = connect(&peer).await?;
    let old = open(&driver).await?;
    let mut writer = old
        .write_file(
            ah_vfs::resolve::operand::WriteOperand::new(
                StoragePath::parse("file")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    let error = writer.commit().await.err().ok_or("lost response")?;
    assert_eq!(error.operation(), Operation::Write);
    assert_eq!(
        error.failure(),
        Failure::Backend(BackendFailure::Unavailable)
    );
    assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unknown);
    let new = open(&driver).await?;
    assert_eq!(
        stat(new.as_ref()).await?.entry_type(),
        ah_vfs_storage::EntryType::file(Some(2))
    );
    assert_eq!(
        writer
            .commit()
            .await
            .err()
            .ok_or("terminal writer")?
            .failure(),
        Failure::Backend(BackendFailure::Closed)
    );
    assert_eq!(peer.commits.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn changed_declarations_require_a_new_registration() -> Result {
    let peer = Peer::new();
    let driver = connect(&peer).await?;
    let old = open(&driver).await?;
    peer.restart();
    peer.changed.store(true, Ordering::SeqCst);
    assert_eq!(
        driver
            .reconnect()
            .await
            .err()
            .ok_or("changed description")?
            .lifecycle_error()
            .ok_or("category")?
            .failure(),
        ah_vfs::application::driver_error::LifecycleFailure::InvalidDeclaration
    );
    assert!(old.is_stale());
    assert!(driver.info().capabilities().stat());
    assert!(open(&driver).await.is_err());
    assert_eq!(
        peer.initialized.lock().map_err(|_| "settings lock")?.len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn failed_and_cancelled_recovery_can_be_followed_by_new_opens() -> Result {
    let peer = Peer::new();
    let driver = connect(&peer).await?;
    let old = open(&driver).await?;
    peer.restart();
    let _ = stat(old.as_ref()).await;
    peer.fail_describe.store(true, Ordering::SeqCst);
    assert!(driver.reconnect().await.is_err());
    peer.suspend_initialize.store(true, Ordering::SeqCst);
    let task = {
        let driver = driver.clone();
        tokio::spawn(async move { driver.reconnect().await })
    };
    peer.initializing.notified().await;
    task.abort();
    peer.resume.notify_one();
    let new = open(&driver).await?;
    assert_eq!(
        stat(new.as_ref()).await?.entry_type(),
        ah_vfs_storage::EntryType::file(Some(2))
    );
    assert_eq!(
        peer.initialized.lock().map_err(|_| "settings lock")?.len(),
        2
    );
    Ok(())
}

struct ReopeningFactory {
    peer: Arc<Peer>,
    connections: AtomicUsize,
}
struct ReopeningControl {
    peer: Arc<Peer>,
    reject_open: bool,
}
impl vfs_plugin_host::TransportFactory for ReopeningFactory {
    fn connect(self: Arc<Self>) -> vfs_plugin_host::ConnectFuture {
        Box::pin(async move {
            let first = self.connections.fetch_add(1, Ordering::SeqCst) == 0;
            Ok(Arc::new(ReopeningControl {
                peer: self.peer.clone(),
                reject_open: first,
            }) as Arc<dyn Transport>)
        })
    }
}
impl SessionTransport for ReopeningControl {
    fn call<'a>(
        &'a self,
        generation: &'a str,
        operation: Op,
        data: Vec<u8>,
    ) -> TransportFuture<'a> {
        self.peer.call(generation, operation, data)
    }
}
impl Transport for ReopeningControl {
    fn open_session(self: Arc<Self>, generation: &str, open: p::Open) -> OpenFuture<'_> {
        Box::pin(async move {
            if self.reject_open {
                self.peer.restart();
                return Err(ah_vfs::application::driver_error::LifecycleError::new(
                    ah_vfs::application::driver_error::LifecycleFailure::Stale,
                )
                .into());
            }
            self.peer.clone().open_session(generation, open).await
        })
    }
}
#[tokio::test]
async fn stale_open_recreates_transport_describes_initializes_and_opens_once_more() -> Result {
    let peer = Peer::new();
    let factory = Arc::new(ReopeningFactory {
        peer: peer.clone(),
        connections: AtomicUsize::new(0),
    });
    let driver = PluginDriver::connect(factory.clone()).await?;
    driver
        .initialize([("setting".into(), "original".into())].into())
        .await?;
    let storage = open(&driver).await?;
    assert_eq!(
        stat(storage.as_ref()).await?.entry_type(),
        ah_vfs_storage::EntryType::file(Some(2))
    );
    assert_eq!(factory.connections.load(Ordering::SeqCst), 2);
    assert_eq!(peer.descriptions.load(Ordering::SeqCst), 2);
    let initialized = peer.initialized.lock().map_err(|_| "settings lock")?;
    assert_eq!(initialized.len(), 2);
    assert_eq!(initialized[0].1, initialized[1].1);
    assert_eq!(peer.commits.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn host_resolves_the_operand_using_only_bound_child_observations() -> Result {
    let driver = connect(&Peer::new()).await?;
    let storage = open(&driver).await?;
    let target = ah_vfs::resolve::read_link(
        storage.as_ref(),
        ah_vfs::resolve::path::PathExpr::parse("../link")?,
        LookupContext {
            working_directory: StoragePath::parse("docs/sub")?,
            intermediate_symlinks: ah_vfs::resolve::policy::IntermediateSymlinks::Reject,
        },
    )
    .await?;
    assert_eq!(target.as_bytes(), b"\xff/");
    Ok(())
}
