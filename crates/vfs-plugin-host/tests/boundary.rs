use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::port::*;
use ah_vfs::domain::InstanceId;
use ah_vfs::resolve::LookupContext;
use ah_vfs::resolve::StorageOperations;
use ah_vfs::resolve::operand::{ListOperand, ListOperandTarget, RenameOperand};
use ah_vfs::resolve::path::PathExpr;
use ah_vfs::resolve::path::RootedPath;
use ah_vfs_storage::Capabilities;
use ah_vfs_storage::{BackendFailure, ContractViolation, Failure, Operation};
use ah_vfs_storage::{Entry, EntryType, StoragePath};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};
use vfs_plugin_host::{
    OpenFuture, OpenedStorage, PluginDriver, SessionTransport, Transport, TransportFuture, convert,
};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
enum Fault {
    Version,
    Capabilities,
    Commit,
    Rename,
    Reader,
    DirectoryPage { wrong_address: bool },
    Attributes(Vec<u8>),
}
struct Peer {
    fault: Fault,
    commits: AtomicUsize,
    renames: AtomicUsize,
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
        if matches!(self.fault, Fault::DirectoryPage { .. }) {
            capabilities.push(p::Capability::List as i32);
        }
        if matches!(self.fault, Fault::Rename) {
            capabilities.push(p::Capability::RenameNew as i32);
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
    fn open_session(self: Arc<Self>, generation: &str, _: p::Open) -> OpenFuture<'_> {
        Box::pin(async move {
            Ok(OpenedStorage {
                transport: self,
                generation: generation.into(),
                storage: p::Storage {
                    handle: 1,
                    info: Some(p::StorageInfo { chunk_size: 65536 }),
                },
            })
        })
    }
}
impl SessionTransport for Peer {
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
                Op::Storage(call) => match call.operation {
                    Some(p::StorageOp::List(_)) => {
                        let wrong_address = matches!(
                            self.fault,
                            Fault::DirectoryPage {
                                wrong_address: true
                            }
                        );
                        Value::Page(p::Page {
                            reported: Some(p::DirectorySelection {
                                address: if wrong_address {
                                    "forged".into()
                                } else {
                                    String::new()
                                },
                                identity: vec![if wrong_address { 1 } else { 2 }; 16],
                            }),
                            completion: Some(p::page::Completion::End(p::Empty {})),
                        })
                    }
                    Some(p::StorageOp::GetPermissions(_)) => {
                        Value::Permissions(p::Permissions { mode: 0o100644 })
                    }
                    Some(p::StorageOp::ListXattrs(_)) => {
                        let mut reply = p::Reply::new(Value::Names(p::Empty {}));
                        if let Fault::Attributes(names) = &self.fault {
                            reply.data = names.clone();
                        }
                        return Ok(reply);
                    }
                    _ => Value::Unit(p::Empty {}),
                },
                Op::BeginLookup(_) => Value::OpenedLookup(p::OpenedLookup {
                    handle: 2,
                    root: Some(p::ObservedEntry {
                        id: 3,
                        entry: Some(p::Entry {
                            kind: p::EntryKind::Dir.into(),
                            ..Default::default()
                        }),
                    }),
                }),
                Op::ObserveChild(child) => {
                    let operation = p::decode_lookup_purpose(&child.purpose)
                        .ok_or_else(convert::protocol_violation)?;
                    if operation == Operation::Write || child.name == "destination" {
                        return Err(ah_vfs_storage::StorageError::without_effects(
                            operation,
                            Failure::Rejected(ah_vfs_storage::Rejection::NotFound),
                            None,
                        )
                        .into());
                    }
                    Value::ObservedEntry(p::ObservedEntry {
                        id: 4,
                        entry: Some(p::Entry {
                            path: child.name,
                            kind: p::EntryKind::File.into(),
                            ..Default::default()
                        }),
                    })
                }
                Op::Execute(call) => match call.operation {
                    Some(p::BoundOp::SelectDirectory(_)) => {
                        Value::SelectedDirectory(p::DirectorySelection {
                            address: String::new(),
                            identity: vec![1; 16],
                        })
                    }
                    Some(p::BoundOp::Rename(_)) => {
                        self.renames.fetch_add(1, Ordering::Relaxed);
                        Value::Entry(p::Entry {
                            path: "forged".into(),
                            kind: p::EntryKind::Dir.into(),
                            ..Default::default()
                        })
                    }
                    Some(p::BoundOp::Write(_)) => Value::Writer(p::Handle { handle: 5 }),
                    Some(p::BoundOp::Read(_)) => Value::Reader(p::Handle { handle: 5 }),
                    _ => return Err(convert::protocol_violation()),
                },
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
        renames: AtomicUsize::new(0),
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
            .lifecycle_error()
            .ok_or("category")?
            .failure(),
        ah_vfs::application::driver_error::LifecycleFailure::Unsupported
    );
    let error = peer(Fault::Capabilities)
        .await
        .err()
        .ok_or("capability rejection")?;
    let error = error.downcast_ref::<DriverError>().ok_or("driver error")?;
    assert_eq!(
        error.lifecycle_error().ok_or("category")?.failure(),
        ah_vfs::application::driver_error::LifecycleFailure::InvalidDeclaration
    );
    Ok(())
}
#[tokio::test]
async fn forged_rename_response_leaves_the_outcome_unknown_without_replay() -> Result {
    use ah_vfs_storage::{MutationOutcome, RenameMode};
    let (driver, peer) = peer(Fault::Rename).await?;
    let storage = driver
        .open(OpenContext::new(
            InstanceId::parse("instance")?,
            Default::default(),
        ))
        .await?;
    let request = RenameOperand::new(
        PathExpr::parse("/source")?,
        PathExpr::parse("/destination")?,
        RenameMode::NoReplace,
    )?;
    let error = storage
        .move_entry(request, LookupContext::default())
        .await
        .err()
        .ok_or("forged rename response accepted")?;
    assert_eq!(error.operation(), Operation::Rename);
    assert_eq!(
        error.failure(),
        Failure::Contract(ContractViolation::InvalidResponse)
    );
    assert_eq!(error.outcome(), MutationOutcome::Unknown);
    assert_eq!(peer.renames.load(Ordering::Relaxed), 1);
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
        .write_file(
            ah_vfs::resolve::operand::WriteOperand::new(
                StoragePath::parse("file")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"accepted").await?;
    let error = writer.commit().await.err().ok_or("unknown outcome")?;
    assert_eq!(
        error.failure(),
        Failure::Contract(ContractViolation::InvalidResponse)
    );
    assert_eq!(error.operation(), Operation::Write);
    assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unknown);
    assert_eq!(
        writer.commit().await.err().ok_or("terminal")?.failure(),
        Failure::Backend(BackendFailure::Closed)
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
        .read_file(
            ah_vfs::resolve::operand::ReadOperand::new(
                StoragePath::parse("file")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(
        reader
            .next_chunk()
            .await
            .err()
            .ok_or("invalid chunk")?
            .failure(),
        Failure::Contract(ContractViolation::InvalidResponse)
    );
    assert_eq!(
        reader.next_chunk().await.err().ok_or("closed")?.failure(),
        Failure::Backend(BackendFailure::Closed)
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
        let entry = Entry::new(
            StoragePath::parse("file")?,
            EntryType::file(Some(u64::MAX)),
            Some(time),
        )?;
        let encoded = convert::entry(Operation::Stat, entry)?;
        let bytes = p::encode_reply(Ok(p::Reply::new(Value::Entry(encoded))))?;
        let value = p::decode_reply(&bytes)?
            .map_err(|error| convert::driver_error(error, None))?
            .into_value()?;
        let Value::Entry(entry) = value else {
            return Err("entry reply".into());
        };
        let decoded = convert::decode_entry(Operation::Stat, entry)?;
        assert_eq!(decoded.modified(), Some(time));
        assert_eq!(decoded.entry_type(), EntryType::file(Some(u64::MAX)));
    }
    assert_eq!(
        convert::driver_error(p::Failure::new(p::FailureCode::Unspecified), None)
            .lifecycle_error()
            .ok_or("unknown status accepted")?
            .failure(),
        ah_vfs::application::driver_error::LifecycleFailure::InvalidDeclaration
    );
    assert!(convert::decode_capabilities(&[p::Capability::PermissionsWrite as i32]).is_err());
    let caps = convert::decode_capabilities(&[i32::MAX])?;
    assert_eq!(caps, Capabilities::default());
    let caps = caps.with_stat(true);
    assert_eq!(
        convert::decode_capabilities(&p::encode_capabilities(caps))?,
        caps
    );
    Ok(())
}

#[tokio::test]
async fn invalid_attribute_replies_are_distinct_from_backend_name_errors() -> Result {
    use ah_vfs_storage::{AttributeAction, MutationOutcome};
    let (driver, _) = peer(Fault::Attributes(vec![])).await?;
    let storage = driver
        .open(OpenContext::new(
            InstanceId::parse("instance")?,
            Default::default(),
        ))
        .await?;
    let attributes = storage.attributes().ok_or("missing attributes port")?;
    let error = ah_vfs::resolve::attribute(
        storage.as_ref(),
        PathExpr::parse("/")?,
        Default::default(),
        AttributeAction::GetPermissions,
    )
    .await
    .err()
    .ok_or("accepted entry type bits")?;
    assert_eq!(
        error.failure(),
        Failure::Contract(ContractViolation::InvalidAttributes)
    );
    assert!(matches!(
        std::error::Error::source(&error)
            .and_then(|source| source.downcast_ref::<p::AttributeValueError>()),
        Some(p::AttributeValueError::Permissions(
            ah_vfs_storage::PermissionsError::InvalidBits
        ))
    ));
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    assert_eq!(
        attributes.support(),
        (
            ah_vfs_storage::AttributeSupport::Read,
            ah_vfs_storage::AttributeSupport::Read
        )
    );

    for packed in [
        b"user.a\0user.a\0".as_slice(),
        b"user.b\0user.a\0",
        b"\0",
        b"user.\xff\0",
        b"user.missing-terminator",
    ] {
        let (driver, _) = peer(Fault::Attributes(packed.to_vec())).await?;
        let storage = driver
            .open(OpenContext::new(
                InstanceId::parse("instance")?,
                Default::default(),
            ))
            .await?;
        let error = ah_vfs::resolve::attribute(
            storage.as_ref(),
            PathExpr::parse("/entry")?,
            Default::default(),
            AttributeAction::list_xattrs(NonZeroUsize::new(1024).ok_or("zero limit")?),
        )
        .await
        .err()
        .ok_or("accepted invalid attribute names")?;
        assert_eq!(
            error.failure(),
            Failure::Contract(ContractViolation::InvalidAttributes)
        );
        assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    }
    Ok(())
}

#[tokio::test]
async fn an_empty_remote_page_cannot_attest_its_own_directory() -> Result {
    use ah_vfs_storage::ContractViolation;
    for wrong_address in [false, true] {
        let (driver, _) = peer(Fault::DirectoryPage { wrong_address }).await?;
        let storage = driver
            .open(OpenContext::new(
                InstanceId::parse("fixture")?,
                Default::default(),
            ))
            .await?;
        let request = ListOperand::new(
            ListOperandTarget::Path(StoragePath::root().to_rooted_expr()),
            Default::default(),
        );
        let error = storage
            .list_entries(&request, Default::default())
            .await
            .err()
            .ok_or("forged page accepted")?;
        assert_eq!(
            error.failure(),
            Failure::Contract(ContractViolation::InvalidPage)
        );
    }
    Ok(())
}
