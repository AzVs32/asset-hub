use ah_vfs::{
    application::{dto::*, port::*},
    domain::*,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use vfs_plugin_host::{PluginDriver, Transport, TransportFuture};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Peer {
    generation: AtomicUsize,
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
            initialized: Mutex::new(vec![]),
            descriptions: AtomicUsize::new(0),
            next: AtomicU64::new(1),
            changed: AtomicBool::new(false),
            fail_describe: AtomicBool::new(false),
            commits: AtomicUsize::new(0),
            suspend_initialize: AtomicBool::new(false),
            initializing: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        })
    }
    fn restart(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
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
    fn call<'a>(&'a self, generation: &'a str, op: Op, _data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            let current = self.generation.load(Ordering::SeqCst);
            if matches!(op, Op::Describe(p::Empty {})) {
                self.descriptions.fetch_add(1, Ordering::SeqCst);
                if self.fail_describe.swap(false, Ordering::SeqCst) {
                    return Err(StorageError::new(StorageErrorKind::Unavailable).into());
                }
                return Ok(p::Reply::new(Value::Description(self.description().into())));
            }
            if generation != current.to_string() {
                return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
            }
            let value = match op {
                Op::Initialize(p::Initialize {
                    settings: Some(settings),
                }) => {
                    self.initialized
                        .lock()
                        .map_err(|_| vfs_plugin_host::convert::invalid())?
                        .push((current, settings));
                    if self.suspend_initialize.load(Ordering::SeqCst) {
                        self.initializing.notify_one();
                        self.resume.notified().await;
                    }
                    Value::Unit(p::Empty {})
                }
                Op::Open(_) => Value::Storage(p::Storage {
                    handle: self.next.fetch_add(1, Ordering::SeqCst),
                    info: Some(p::StorageInfo {
                        capabilities: self.description().capabilities,
                        chunk_size: 65536,
                    }),
                }),
                Op::Storage(p::StorageCall {
                    operation: Some(p::StorageOp::Stat(p::Path { path })),
                    ..
                }) => Value::Entry(p::Entry {
                    path,
                    kind: p::EntryKind::File as i32,
                    size: Some(current as u64),
                    modified: None,
                }),
                Op::Read(_) => Value::Reader(p::Handle {
                    handle: self.next.fetch_add(1, Ordering::SeqCst),
                }),
                Op::Write(_) => Value::Writer(p::Handle {
                    handle: self.next.fetch_add(1, Ordering::SeqCst),
                }),
                Op::Commit(_) => {
                    self.commits.fetch_add(1, Ordering::SeqCst);
                    self.restart();
                    return Err(StorageError::new(StorageErrorKind::OutcomeUnknown).into());
                }
                _ => Value::Unit(p::Empty {}),
            };
            Ok(p::Reply::new(value))
        })
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
async fn stat(storage: &dyn Storage) -> std::result::Result<StorageEntry, StorageError> {
    storage
        .stat_entry(StorageStatEntryRequest {
            path: StoragePath::parse("file")?,
        })
        .await
}

#[tokio::test]
async fn new_opens_share_recovery_and_old_resources_stay_stale() -> Result {
    let peer = Peer::new();
    let driver = connect(&peer).await?;
    let old = open(&driver).await?;
    let mut reader = old
        .read_file(StorageReadFileRequest::new(StoragePath::parse("file")?))
        .await?;
    peer.restart();
    assert_eq!(
        stat(old.as_ref())
            .await
            .err()
            .ok_or("old generation")?
            .kind(),
        StorageErrorKind::StaleHandle
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
        assert_eq!(stat(storage.as_ref()).await?.size, Some(2));
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
        reader.next_chunk().await.err().ok_or("old reader")?.kind(),
        StorageErrorKind::StaleHandle
    );
    assert_eq!(
        stat(old.as_ref()).await.err().ok_or("old storage")?.kind(),
        StorageErrorKind::StaleHandle
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
        .write_file(StorageWriteFileRequest::new(StoragePath::parse("file")?))
        .await?;
    assert_eq!(
        writer.commit().await.err().ok_or("lost response")?.kind(),
        StorageErrorKind::OutcomeUnknown
    );
    let new = open(&driver).await?;
    assert_eq!(stat(new.as_ref()).await?.size, Some(2));
    assert_eq!(
        writer.commit().await.err().ok_or("terminal writer")?.kind(),
        StorageErrorKind::SessionClosed
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
            .storage_error()
            .ok_or("category")?
            .kind(),
        StorageErrorKind::InvalidData
    );
    assert!(old.is_stale());
    assert!(driver.info().capabilities().stat_entry);
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
    assert_eq!(stat(new.as_ref()).await?.size, Some(2));
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
impl Transport for ReopeningControl {
    fn call<'a>(
        &'a self,
        generation: &'a str,
        operation: Op,
        data: Vec<u8>,
    ) -> TransportFuture<'a> {
        self.peer.call(generation, operation, data)
    }
    fn open_session<'a>(
        &'a self,
        generation: &'a str,
        open: p::Open,
    ) -> Option<vfs_plugin_host::OpenFuture<'a>> {
        Some(Box::pin(async move {
            if self.reject_open {
                self.peer.restart();
                return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
            }
            let Value::Storage(storage) = self
                .peer
                .call(generation, Op::Open(open), vec![])
                .await?
                .into_value()
                .map_err(|_| vfs_plugin_host::convert::invalid())?
            else {
                return Err(vfs_plugin_host::convert::invalid());
            };
            Ok(vfs_plugin_host::OpenedStorage {
                storage,
                generation: generation.into(),
                transport: self.peer.clone(),
            })
        }))
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
    assert_eq!(stat(storage.as_ref()).await?.size, Some(2));
    assert_eq!(factory.connections.load(Ordering::SeqCst), 2);
    assert_eq!(peer.descriptions.load(Ordering::SeqCst), 2);
    let initialized = peer.initialized.lock().map_err(|_| "settings lock")?;
    assert_eq!(initialized.len(), 2);
    assert_eq!(initialized[0].1, initialized[1].1);
    assert_eq!(peer.commits.load(Ordering::SeqCst), 0);
    Ok(())
}
