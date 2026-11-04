use ah_vfs::{
    application::port::*,
    domain::{
        DriverError, InstanceId,
        storage::{
            Entry, EntryKind, EntryPermissionsError, StorageCapabilities, StorageErrorKind,
            StoragePath, StorageReadFileRequest, StorageWriteFileRequest, XattrName,
            XattrNameError,
        },
    },
};
use std::{
    error::Error,
    num::NonZeroUsize,
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
    Attributes(String),
}
struct Peer {
    fault: Fault,
    commits: AtomicUsize,
}
impl Peer {
    fn capabilities(&self) -> Vec<i32> {
        let mut capabilities = vec![
            p::Capability::Stat as i32,
            p::Capability::RangeRead as i32,
            p::Capability::WriteNew as i32,
        ];
        if matches!(self.fault, Fault::Attributes(_)) {
            capabilities.extend([
                p::Capability::PermissionsRead as i32,
                p::Capability::XattrsRead as i32,
            ]);
        }
        capabilities
    }
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
                        capabilities: if matches!(self.fault, Fault::Capabilities) {
                            vec![p::Capability::AtomicPublish as i32]
                        } else {
                            self.capabilities()
                        },

                        settings_schema: None,
                        instance_schema: None,
                        config_version: 1,
                    }
                    .into(),
                ),
                Op::Open(_) => Value::Storage(p::Storage {
                    handle: 1,
                    info: Some(p::StorageInfo { chunk_size: 65536 }),
                }),
                Op::Storage(call) => match call.operation {
                    Some(p::StorageOp::GetPermissions(_)) => {
                        Value::Permissions(p::Permissions { mode: 0o100644 })
                    }
                    Some(p::StorageOp::ListXattrs(_)) => Value::Names(p::Names {
                        names: vec![match &self.fault {
                            Fault::Attributes(name) => name.clone(),
                            _ => "user.valid".into(),
                        }],
                    }),
                    _ => Value::Unit(p::Empty {}),
                },
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
    let error = peer(Fault::Capabilities)
        .await
        .err()
        .ok_or("capability rejection")?;
    let error = error.downcast_ref::<DriverError>().ok_or("driver error")?;
    assert_eq!(
        error.storage_error().ok_or("category")?.kind(),
        StorageErrorKind::InvalidData
    );
    Ok(())
}
#[tokio::test]
async fn malformed_commit_response_is_unknown_and_is_never_replayed() -> Result {
    let (driver, peer) = peer(Fault::Commit).await?;
    let storage = driver
        .open(OpenContext::new(
            InstanceId::parse("instance")?,
            Default::default(),
        ))
        .await?;
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
    let storage = driver
        .open(OpenContext::new(
            InstanceId::parse("instance")?,
            Default::default(),
        ))
        .await?;
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
        let entry = Entry {
            path: StoragePath::parse("file")?,
            kind: EntryKind::File,
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
    let caps = convert::decode_capabilities(&[i32::MAX])?;
    assert_eq!(caps, StorageCapabilities::default());
    let caps = caps.to_builder().stat_entry(true).build()?;
    assert_eq!(
        convert::decode_capabilities(&convert::capabilities(caps))?,
        caps
    );
    Ok(())
}

#[tokio::test]
async fn malformed_permissions_response_is_backend_data_with_its_validation_cause() -> Result {
    let (driver, _) = peer(Fault::Attributes("user.valid".into())).await?;
    let storage = driver
        .open(OpenContext::new(
            InstanceId::parse("instance")?,
            Default::default(),
        ))
        .await?;
    let error = storage
        .permissions()
        .ok_or("missing permissions extension")?
        .get_permissions(StoragePath::root())
        .await
        .err()
        .ok_or("accepted entry type bits")?;
    assert_eq!(error.kind(), StorageErrorKind::InvalidData);
    assert_eq!(
        error
            .source()
            .and_then(|e| e.downcast_ref::<EntryPermissionsError>()),
        Some(&EntryPermissionsError::InvalidBits)
    );
    Ok(())
}

#[tokio::test]
async fn malformed_xattr_responses_keep_each_value_validation_cause() -> Result {
    for (name, expected) in [
        (String::new(), XattrNameError::Empty),
        ("a".repeat(XattrName::MAX_LEN + 1), XattrNameError::TooLong),
        ("user.\0name".to_owned(), XattrNameError::InvalidCharacter),
    ] {
        let (driver, _) = peer(Fault::Attributes(name)).await?;
        let storage = driver
            .open(OpenContext::new(
                InstanceId::parse("instance")?,
                Default::default(),
            ))
            .await?;
        let error = storage
            .xattrs()
            .ok_or("missing xattrs extension")?
            .list_xattrs(
                StoragePath::root(),
                NonZeroUsize::new(1024).ok_or("zero limit")?,
            )
            .await
            .err()
            .ok_or("accepted invalid attribute name")?;
        assert_eq!(error.kind(), StorageErrorKind::InvalidData);
        assert_eq!(
            error
                .source()
                .and_then(|e| e.downcast_ref::<XattrNameError>()),
            Some(&expected)
        );
    }
    Ok(())
}
