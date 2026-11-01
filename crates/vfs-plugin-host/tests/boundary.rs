use ah_vfs::{
    application::{dto::*, port::*},
    domain::*,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};
use vfs_plugin_host::{PluginDriver, Transport, TransportFuture, convert};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
enum Fault {
    Version,
    Capabilities,
    Commit,
    Reader,
}
struct Peer {
    fault: Fault,
    commits: AtomicUsize,
}
impl vfs_plugin_host::TransportFactory for Peer {
    fn connect(self: Arc<Self>) -> vfs_plugin_host::ConnectFuture {
        Box::pin(async move { Ok(self as Arc<dyn Transport>) })
    }
}
impl Transport for Peer {
    fn call<'a>(&'a self, _generation: &'a str, op: Op, _data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            Ok(p::Reply::new(match op {
                Op::Describe(p::Empty {}) => Value::Description(
                    p::Description {
                        major: if matches!(self.fault, Fault::Version) {
                            p::PROTOCOL_MAJOR + 1
                        } else {
                            p::PROTOCOL_MAJOR
                        },
                        minor: 0,
                        required_features: vec![],
                        features: vec![],
                        required_host_capabilities: vec![],
                        instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
                        generation: "one-runtime".into(),
                        key: "opaque".into(),
                        name: "Opaque peer".into(),
                        version: "1.0.0".into(),
                        child_mounts: true,
                        capabilities: vec![
                            p::Capability::Stat as i32,
                            p::Capability::RangeRead as i32,
                            p::Capability::WriteNew as i32,
                        ],
                        ownership: p::Ownership::Plugin as i32,
                        settings_schema: None,
                        instance_schema: None,
                        config_version: 1,
                    }
                    .into(),
                ),
                Op::Open(_) => Value::Storage(p::Storage {
                    handle: 1,
                    info: Some(p::StorageInfo {
                        capabilities: if matches!(self.fault, Fault::Capabilities) {
                            vec![p::Capability::RenameReplace as i32]
                        } else {
                            vec![
                                p::Capability::Stat as i32,
                                p::Capability::RangeRead as i32,
                                p::Capability::WriteNew as i32,
                            ]
                        },
                        chunk_size: 65536,
                    }),
                }),
                Op::Write(_) => Value::Writer(p::Handle { handle: 2 }),
                Op::Read(_) => Value::Reader(p::Handle { handle: 3 }),
                Op::ReaderNext(_) => Value::Chunk(p::Chunk { eof: false }),
                Op::Commit(_) => {
                    self.commits.fetch_add(1, Ordering::Relaxed);
                    Value::Unit(p::Empty {})
                }
                _ => Value::Unit(p::Empty {}),
            }))
        })
    }
}
async fn peer(fault: Fault) -> Result<(PluginDriver, Arc<Peer>)> {
    let peer = Arc::new(Peer {
        fault,
        commits: AtomicUsize::new(0),
    });
    let driver = PluginDriver::connect(peer.clone()).await?;
    driver.initialize(Default::default()).await?;
    Ok((driver, peer))
}
#[tokio::test]
async fn incompatible_versions_and_false_capabilities_are_rejected() -> Result {
    let error = peer(Fault::Version)
        .await
        .err()
        .ok_or("version rejection")?;
    assert_eq!(
        error
            .downcast_ref::<DriverError>()
            .ok_or("driver error")?
            .storage_error()
            .ok_or("category")?
            .kind(),
        StorageErrorKind::UnsupportedOperation
    );
    let (driver, _) = peer(Fault::Capabilities).await?;
    let error = driver
        .open(InstanceId::parse("instance")?.into())
        .await
        .err()
        .ok_or("capability rejection")?;
    assert_eq!(
        error.storage_error().ok_or("category")?.kind(),
        StorageErrorKind::InvalidData
    );
    Ok(())
}
#[tokio::test]
async fn malformed_commit_response_is_unknown_and_is_never_replayed() -> Result {
    let (driver, peer) = peer(Fault::Commit).await?;
    let storage = driver.open(InstanceId::parse("instance")?.into()).await?;
    let mut writer = storage
        .write_file(StorageWriteFileRequest::new(StoragePath::parse("file")?))
        .await?;
    writer.write_chunk(b"accepted").await?;
    assert_eq!(
        writer.commit().await.err().ok_or("unknown outcome")?.kind(),
        StorageErrorKind::OutcomeUnknown
    );
    assert_eq!(
        writer.commit().await.err().ok_or("terminal")?.kind(),
        StorageErrorKind::SessionClosed
    );
    assert_eq!(peer.commits.load(Ordering::Relaxed), 1);
    Ok(())
}
#[tokio::test]
async fn empty_non_eof_chunks_close_the_read_session() -> Result {
    let (driver, _) = peer(Fault::Reader).await?;
    let storage = driver.open(InstanceId::parse("instance")?.into()).await?;
    let mut reader = storage
        .read_file(StorageReadFileRequest::new(StoragePath::parse("file")?))
        .await?;
    assert_eq!(
        reader
            .next_chunk()
            .await
            .err()
            .ok_or("invalid chunk")?
            .kind(),
        StorageErrorKind::InvalidData
    );
    assert_eq!(
        reader.next_chunk().await.err().ok_or("closed")?.kind(),
        StorageErrorKind::SessionClosed
    );
    Ok(())
}
#[test]
fn timestamps_and_unknown_error_codes_preserve_boundary_semantics() -> Result {
    for time in [
        UNIX_EPOCH,
        UNIX_EPOCH - Duration::from_nanos(1),
        UNIX_EPOCH - Duration::from_millis(1500),
        UNIX_EPOCH + Duration::from_nanos(1),
    ] {
        let entry = StorageEntry {
            path: StoragePath::parse("file")?,
            kind: StorageEntryKind::File,
            size: Some(u64::MAX),
            modified: Some(time),
        };
        let encoded = convert::entry(entry)?;
        let bytes = p::encode_reply(Ok(p::Reply::new(Value::Entry(encoded))))?;
        let value = p::decode_reply(&bytes)?
            .map_err(|error| convert::driver_error(error, None))?
            .into_value()?;
        let Value::Entry(entry) = value else {
            return Err("entry reply".into());
        };
        let decoded = convert::decode_entry(entry)?;
        assert_eq!(decoded.modified, Some(time));
        assert_eq!(decoded.size, Some(u64::MAX));
    }
    assert_eq!(
        convert::error_kind(p::FailureCode::Unspecified),
        StorageErrorKind::Backend
    );
    assert!(convert::decode_capabilities(&[p::Capability::PermissionsWrite as i32]).is_err());
    let mut caps = convert::decode_capabilities(&[i32::MAX])?;
    assert_eq!(caps, StorageCapabilities::default());
    caps.stat_entry = true;
    assert_eq!(
        convert::decode_capabilities(&convert::capabilities(caps))?,
        caps
    );
    Ok(())
}
