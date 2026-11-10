use ah_vfs_storage::{
    AttributeAction, AttributeRequest, AttributeResult, AttributeSupport, BackendFailure, Failure,
    IntermediateSymlinks, Operation, PathExpr, Permissions, Rejection, StorageError, StoragePath,
};
use std::cell::{Cell, RefCell};
use vfs_plugin_guest::{Dispatcher, api::*};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
std::thread_local! { static OPENS: Cell<usize> = const { Cell::new(0) }; static COMMITS: Cell<usize> = const { Cell::new(0) }; }
std::thread_local! {
    static DECLARE_ATTRIBUTES: Cell<bool> = const { Cell::new(false) };
    static ATTRIBUTE_CALLS: Cell<usize> = const { Cell::new(0) };
}
struct Driver;
impl Guest for Driver {
    fn describe() -> Description {
        Description {
            required_features: vec![],
            required_host_capabilities: vec![],
            key: "fixture".into(),
            name: "Fixture".into(),
            version: "1.0.0".into(),
            child_mounts: true,
            capabilities: ah_vfs_storage::Capabilities::default()
                .with_stat(true)
                .with_read_link(true)
                .with_read(ah_vfs_storage::ReadSupport::Range)
                .with_write(ah_vfs_storage::WriteSupport::staged(
                    ah_vfs_storage::WriteModes::CreateNew,
                    false,
                ))
                .with_permissions(if DECLARE_ATTRIBUTES.with(Cell::get) {
                    AttributeSupport::Read
                } else {
                    AttributeSupport::None
                }),

            settings_schema: None,
            instance_schema: None,
            config_version: 1,
        }
    }
    fn initialize(_: p::Object) -> Result<()> {
        Ok(())
    }
    fn prepare(config: p::Object) -> Result<p::Prepared> {
        Ok(p::Prepared {
            config: Some(config),
            details: Some(p::Object::new()),
        })
    }
    fn migrate(_: u32, config: p::Object) -> Result<p::Object> {
        Ok(config)
    }
    fn test_connection(_: p::Object) -> Result<()> {
        Ok(())
    }
    fn open(_: String, config: p::Object) -> Result<Storage> {
        let config = p::json_object(config)?;
        OPENS.with(|v| v.set(v.get() + 1));
        Ok(Storage::new(Store {
            attributes: config
                .get("attributes")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            fail: config
                .get("fail")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }))
    }
}
fn entry(size: u64) -> Result<ah_vfs_storage::Entry> {
    let path =
        StoragePath::parse("file").map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
    ah_vfs_storage::Entry::new(path, ah_vfs_storage::EntryType::file(Some(size)), None)
        .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest).into())
}
struct Store {
    attributes: bool,
    fail: bool,
}
impl GuestStorage for Store {
    fn chunk_size(&self) -> std::num::NonZeroUsize {
        std::num::NonZeroUsize::new(65536).unwrap_or(std::num::NonZeroUsize::MIN)
    }
    fn attributes(&self) -> Option<&dyn GuestAttributes> {
        self.attributes.then_some(self)
    }
    fn stat(
        &self,
        request: &ah_vfs_storage::StatRequest,
        base: &StoragePath,
        links: IntermediateSymlinks,
    ) -> Result<ah_vfs_storage::Entry> {
        direct_stat(request, base, links, |_| entry(0))
    }
    fn read_link(
        &self,
        request: &ah_vfs_storage::ReadLinkRequest,
        base: &StoragePath,
        links: IntermediateSymlinks,
    ) -> Result<ah_vfs_storage::LinkTarget> {
        assert_eq!(request.path().to_string(), "../opaque");
        assert_eq!(base.as_str(), "work/sub");
        assert_eq!(links, IntermediateSymlinks::Reject);
        Ok(ah_vfs_storage::LinkTarget::new(vec![0xff, b'/']))
    }
    fn read(
        &self,
        _: ah_vfs_storage::ReadRequest,
        _: ah_vfs_storage::StoragePath,
        _: ah_vfs_storage::IntermediateSymlinks,
    ) -> Result<Reader> {
        Ok(Reader::new(Input(Cell::new(false))))
    }
    fn write(
        &self,
        _: ah_vfs_storage::WriteRequest,
        _: ah_vfs_storage::StoragePath,
        _: ah_vfs_storage::IntermediateSymlinks,
    ) -> Result<Writer> {
        Ok(Writer::new(Output {
            fail: self.fail,
            bytes: RefCell::new(vec![]),
        }))
    }
}
impl GuestAttributes for Store {
    fn support(&self) -> (AttributeSupport, AttributeSupport) {
        (AttributeSupport::Read, AttributeSupport::None)
    }

    fn execute(
        &self,
        request: AttributeRequest,
        base: StoragePath,
        links: IntermediateSymlinks,
    ) -> Result<AttributeResult> {
        ATTRIBUTE_CALLS.with(|v| v.set(v.get() + 1));
        assert_eq!(base.as_str(), "work/sub");
        assert_eq!(links, IntermediateSymlinks::Reject);
        assert_eq!(request.path().to_string(), "../file/");
        assert!(matches!(request.action(), AttributeAction::GetPermissions));
        let mode =
            Permissions::new(0o440).map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
        Ok(AttributeResult::Permissions(mode))
    }
}

struct Input(Cell<bool>);
impl GuestReader for Input {
    fn next(&self) -> Result<Option<Vec<u8>>> {
        Ok(if self.0.replace(true) {
            None
        } else {
            Some(vec![0, 255, 1])
        })
    }
}
struct Output {
    fail: bool,
    bytes: RefCell<Vec<u8>>,
}
impl GuestWriter for Output {
    fn write(&self, data: Vec<u8>) -> Result<()> {
        self.bytes.borrow_mut().extend(data);
        Ok(())
    }
    fn commit(&self) -> Result<ah_vfs_storage::Entry> {
        COMMITS.with(|v| v.set(v.get() + 1));
        if self.fail {
            Err(StorageError::after_dispatch(
                Operation::Write,
                Failure::Backend(BackendFailure::Other),
                None,
            )
            .into())
        } else {
            entry(self.bytes.borrow().len() as u64)
        }
    }
    fn abort(&self) -> Result<()> {
        Ok(())
    }
}
fn call(dispatcher: &mut Dispatcher<Driver>, operation: Op, data: Vec<u8>) -> TestResult<p::Reply> {
    let bytes = p::encode_request("generation", &operation, data)?;
    let reply = p::decode_reply(&dispatcher.call(&bytes))??;
    assert!(
        reply
            .result
            .as_ref()
            .is_some_and(|value| operation.accepts_reply(value))
    );
    Ok(reply)
}
fn setup() -> TestResult<Dispatcher<Driver>> {
    OPENS.with(|v| v.set(0));
    COMMITS.with(|v| v.set(0));
    let mut dispatcher = Dispatcher::new();
    call(&mut dispatcher, Op::Describe(p::Empty {}), vec![])?;
    call(
        &mut dispatcher,
        Op::Initialize(p::Initialize {
            settings: Some(p::Object::new()),
        }),
        vec![],
    )?;
    Ok(dispatcher)
}
fn open(dispatcher: &mut Dispatcher<Driver>, fail: bool) -> TestResult<u64> {
    let reply = call(
        dispatcher,
        Op::Open(p::Open {
            id: "one".into(),
            config: Some(p::Object::from([("fail".into(), fail.into())])),
        }),
        vec![],
    )?;
    match reply.into_value()? {
        Value::Storage(p::Storage {
            handle,
            info: Some(info),
        }) => {
            assert_eq!(info.chunk_size, 65536);
            Ok(handle)
        }
        _ => Err("storage envelope".into()),
    }
}
#[test]
fn metadata_and_commit_are_tagged_protobuf_results() -> TestResult {
    let mut dispatcher = setup()?;
    assert!(matches!(
        call(&mut dispatcher, Op::Describe(p::Empty {}), vec![])?.into_value()?,
        Value::Description(_)
    ));
    let storage = open(&mut dispatcher, false)?;
    let reply = call(
        &mut dispatcher,
        Op::Write(p::encode_write(
            storage,
            &ah_vfs_storage::WriteRequest::new(
                ah_vfs_storage::PathExpr::parse("/file")?,
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            &ah_vfs_storage::StoragePath::root(),
            ah_vfs_storage::IntermediateSymlinks::FollowWithinInstance,
        )),
        vec![],
    )?;
    let Value::Writer(p::Handle { handle }) = reply.into_value()? else {
        return Err("writer".into());
    };
    call(
        &mut dispatcher,
        Op::WriterChunk(p::Handle { handle }),
        vec![0, 255, 1],
    )?;
    assert_eq!(COMMITS.with(Cell::get), 0);
    assert_eq!(
        call(&mut dispatcher, Op::Commit(p::Handle { handle }), vec![])?.into_value()?,
        Value::Entry(p::encode_entry(&entry(3)?)?)
    );
    assert_eq!(COMMITS.with(Cell::get), 1);
    let repeated = p::encode_request("generation", &Op::Commit(p::Handle { handle }), vec![])?;
    assert_eq!(
        p::decode_reply(&dispatcher.call(&repeated))?,
        Err(storage_failure(StorageError::without_effects(
            Operation::Write,
            Failure::Backend(BackendFailure::Closed),
            None
        )))
    );
    Ok(())
}
#[test]
fn failed_commit_is_terminal_and_release_never_publishes() -> TestResult {
    let mut dispatcher = setup()?;
    let storage = open(&mut dispatcher, true)?;
    let Value::Writer(p::Handle { handle }) = call(
        &mut dispatcher,
        Op::Write(p::encode_write(
            storage,
            &ah_vfs_storage::WriteRequest::new(
                ah_vfs_storage::PathExpr::parse("/file")?,
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            &ah_vfs_storage::StoragePath::root(),
            ah_vfs_storage::IntermediateSymlinks::FollowWithinInstance,
        )),
        vec![],
    )?
    .into_value()?
    else {
        return Err("writer".into());
    };
    let request = p::encode_request("generation", &Op::Commit(p::Handle { handle }), vec![])?;
    assert_eq!(
        p::decode_reply(&dispatcher.call(&request))?,
        Err(storage_failure(StorageError::after_dispatch(
            Operation::Write,
            Failure::Backend(BackendFailure::Other),
            None
        )))
    );
    assert_eq!(
        p::decode_reply(&dispatcher.call(&request))?,
        Err(storage_failure(StorageError::without_effects(
            Operation::Write,
            Failure::Backend(BackendFailure::Closed),
            None
        )))
    );
    let Value::Writer(p::Handle { handle }) = call(
        &mut dispatcher,
        Op::Write(p::encode_write(
            storage,
            &ah_vfs_storage::WriteRequest::new(
                ah_vfs_storage::PathExpr::parse("/file")?,
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            &ah_vfs_storage::StoragePath::root(),
            ah_vfs_storage::IntermediateSymlinks::FollowWithinInstance,
        )),
        vec![],
    )?
    .into_value()?
    else {
        return Err("writer".into());
    };
    call(
        &mut dispatcher,
        Op::Release(p::ResourceRef {
            kind: p::ResourceKind::Writer as i32,
            handle,
        }),
        vec![],
    )?;
    assert_eq!(COMMITS.with(Cell::get), 1);
    Ok(())
}
#[test]
fn readers_binary_data_and_generation_checks_do_not_rebind_resources() -> TestResult {
    let mut dispatcher = setup()?;
    let storage = open(&mut dispatcher, false)?;
    let Value::Reader(p::Handle { handle }) = call(
        &mut dispatcher,
        Op::Read(p::encode_read(
            storage,
            &ah_vfs_storage::ReadRequest::new(
                ah_vfs_storage::PathExpr::parse("/file")?,
                ah_vfs_storage::ReadRange::new(0, None)?,
            ),
            &ah_vfs_storage::StoragePath::root(),
            ah_vfs_storage::IntermediateSymlinks::FollowWithinInstance,
        )),
        vec![],
    )?
    .into_value()?
    else {
        return Err("reader".into());
    };
    assert_eq!(
        call(
            &mut dispatcher,
            Op::ReaderNext(p::Handle { handle }),
            vec![]
        )?
        .data,
        vec![0, 255, 1]
    );
    assert_eq!(
        call(
            &mut dispatcher,
            Op::ReaderNext(p::Handle { handle }),
            vec![]
        )?
        .into_value()?,
        Value::Chunk(p::Chunk { eof: true })
    );
    let stale = p::encode_request(
        "another",
        &Op::Release(p::ResourceRef {
            kind: p::ResourceKind::Storage as i32,
            handle: storage,
        }),
        vec![],
    )?;
    assert_eq!(
        p::decode_reply(&dispatcher.call(&stale))?,
        Err(p::Failure::new(p::FailureCode::LifecycleStale))
    );
    let wrong = p::encode_request(
        "generation",
        &Op::Release(p::ResourceRef {
            kind: p::ResourceKind::Reader as i32,
            handle: storage,
        }),
        vec![],
    )?;
    assert_eq!(
        p::decode_reply(&dispatcher.call(&wrong))?,
        Err(p::Failure::new(p::FailureCode::InvalidRequest))
    );
    call(
        &mut dispatcher,
        Op::Storage(p::StorageCall {
            handle: storage,
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
    )?;
    Ok(())
}
#[test]
fn each_store_has_one_storage_and_released_reader_ids_are_not_reused() -> TestResult {
    let mut dispatcher = setup()?;
    let storage = open(&mut dispatcher, false)?;
    assert_eq!(storage, 1);
    let second_open = Op::Open(p::Open {
        id: "other".into(),
        config: Some(p::Object::new()),
    });
    let bytes = p::encode_request("generation", &second_open, vec![])?;
    assert_eq!(
        p::decode_reply(&dispatcher.call(&bytes))?,
        Err(p::Failure::new(p::FailureCode::InvalidRequest))
    );
    assert_eq!(OPENS.with(Cell::get), 1);
    let read = Op::Read(p::encode_read(
        storage,
        &ah_vfs_storage::ReadRequest::new(
            PathExpr::parse("/file")?,
            ah_vfs_storage::ReadRange::FULL,
        ),
        &StoragePath::root(),
        IntermediateSymlinks::FollowWithinInstance,
    ));
    for id in 2..=p::MAX_RESOURCES as u64 {
        assert_eq!(
            call(&mut dispatcher, read.clone(), vec![])?.into_value()?,
            Value::Reader(p::Handle { handle: id })
        );
    }
    let request = p::encode_request("generation", &read, vec![])?;
    assert_eq!(
        p::decode_reply(&dispatcher.call(&request))?,
        Err(storage_failure(StorageError::without_effects(
            Operation::Read,
            Failure::Backend(BackendFailure::CapacityExceeded),
            None,
        )))
    );
    call(
        &mut dispatcher,
        Op::Release(p::ResourceRef {
            kind: p::ResourceKind::Reader.into(),
            handle: 2,
        }),
        vec![],
    )?;
    assert_eq!(
        call(&mut dispatcher, read, vec![])?.into_value()?,
        Value::Reader(p::Handle {
            handle: p::MAX_RESOURCES as u64 + 1,
        })
    );
    assert_eq!(
        p::decode_reply(&dispatcher.call(b"{\"operation\":\"describe\"}"))?,
        Err(p::Failure::new(p::FailureCode::InvalidRequest))
    );
    Ok(())
}

#[test]
fn missing_open_configuration_is_invalid_request_without_opening_storage() -> TestResult {
    let mut dispatcher = setup()?;
    // Independently encoded Request: generation="g", Open(id="one"), no config.
    let bytes = [10, 1, b'g', 66, 5, 10, 3, b'o', b'n', b'e'];
    let error = p::decode_reply(&dispatcher.call(&bytes))?
        .err()
        .ok_or("missing config must fail")?;
    assert_eq!(error.code, p::FailureCode::InvalidRequest as i32);
    OPENS.with(|opens| assert_eq!(opens.get(), 0));
    Ok(())
}

fn storage_failure(error: StorageError) -> p::Failure {
    vfs_plugin_guest::api::Failure::from(error).into()
}

#[test]
fn read_link_uses_the_typed_guest_port_and_preserves_opaque_bytes() -> TestResult {
    let mut dispatcher = setup()?;
    let storage = open(&mut dispatcher, false)?;
    let request = ah_vfs_storage::ReadLinkRequest::new(PathExpr::parse("../opaque")?);
    let reply = call(
        &mut dispatcher,
        Op::Storage(p::StorageCall {
            handle: storage,
            operation: Some(p::StorageOp::ReadLink(p::encode_read_link(
                &request,
                &StoragePath::parse("work/sub")?,
                IntermediateSymlinks::Reject,
            ))),
        }),
        vec![],
    )?;
    assert!(matches!(reply.result, Some(Value::Bytes(_))));
    assert_eq!(reply.data, [0xff, b'/']);
    Ok(())
}

#[test]
fn guest_dispatch_checks_attribute_interfaces_and_admits_read_only_requests() -> TestResult {
    DECLARE_ATTRIBUTES.with(|v| v.set(true));
    ATTRIBUTE_CALLS.with(|v| v.set(0));
    let mut dispatcher = setup()?;
    let request = Op::Open(p::Open {
        id: "one".into(),
        config: Some(p::Object::new()),
    });
    let bytes = p::encode_request("generation", &request, vec![])?;
    assert_eq!(
        p::decode_reply(&dispatcher.call(&bytes))?,
        Err(p::Failure::new(p::FailureCode::LifecycleInvalidDeclaration))
    );
    let reply = call(
        &mut dispatcher,
        Op::Open(p::Open {
            id: "one".into(),
            config: Some(p::Object::from([("attributes".into(), true.into())])),
        }),
        vec![],
    )?;
    let Value::Storage(storage) = reply.into_value()? else {
        return Err("missing storage".into());
    };
    let operation = |action| -> TestResult<_> {
        let request = AttributeRequest::new(PathExpr::parse("../file/")?, action);
        let (op, data) = p::encode_attribute(
            request,
            &StoragePath::parse("work/sub")?,
            IntermediateSymlinks::Reject,
        )?;
        Ok((
            Op::Storage(p::StorageCall {
                handle: storage.handle,
                operation: Some(op),
            }),
            data,
        ))
    };
    let (read, data) = operation(AttributeAction::GetPermissions)?;
    let reply = call(&mut dispatcher, read, data)?;
    assert_eq!(
        p::decode_attribute_result(ah_vfs_storage::AttributeExpectation::Permissions, reply)?,
        AttributeResult::Permissions(Permissions::new(0o440)?)
    );
    let (write, data) = operation(AttributeAction::SetPermissions(Permissions::new(0o600)?))?;
    let bytes = p::encode_request("generation", &write, data)?;
    assert_eq!(
        p::decode_reply(&dispatcher.call(&bytes))?,
        Err(storage_failure(StorageError::without_effects(
            Operation::SetPermissions,
            Failure::Rejected(Rejection::Unsupported),
            None,
        )))
    );
    assert_eq!(ATTRIBUTE_CALLS.with(Cell::get), 1);
    DECLARE_ATTRIBUTES.with(|v| v.set(false));
    Ok(())
}
