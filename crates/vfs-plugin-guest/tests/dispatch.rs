use ah_vfs_storage::{BackendFailure, Failure, Operation, Rejection, StorageError};
use std::cell::{Cell, RefCell};
use vfs_plugin_guest::{Dispatcher, api::*};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
std::thread_local! { static OPENS: Cell<usize> = const { Cell::new(0) }; static COMMITS: Cell<usize> = const { Cell::new(0) }; }
struct Driver;
impl Guest for Driver {
    fn describe() -> p::Description {
        p::Description {
            major: p::PROTOCOL_MAJOR,
            minor: p::PROTOCOL_MINOR,
            required_features: vec![],
            features: p::PROTOCOL_FEATURES.iter().map(|v| (*v).into()).collect(),
            required_host_capabilities: vec![],
            instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
            generation: String::new(),
            key: "fixture".into(),
            name: "Fixture".into(),
            version: "1.0.0".into(),
            child_mounts: true,
            capabilities: vec![
                p::Capability::Stat as i32,
                p::Capability::WriteNew as i32,
                p::Capability::RangeRead as i32,
            ],

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
            fail: config
                .get("fail")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }))
    }
}
fn entry(size: u64) -> p::Entry {
    p::Entry {
        path: "file".into(),
        kind: p::EntryKind::File as i32,
        size: Some(size),
        modified: None,
    }
}
struct Store {
    fail: bool,
}
impl GuestStorage for Store {
    fn info(&self) -> p::StorageInfo {
        p::StorageInfo { chunk_size: 65536 }
    }
    fn call(&self, request: p::StorageOp, _: Vec<u8>) -> Result<p::Reply> {
        match request {
            p::StorageOp::Stat(_) => Ok(p::Reply::new(Value::Entry(entry(0)))),
            _ => Err(StorageError::without_effects(
                request.storage_operation(),
                Failure::Rejected(Rejection::Unsupported),
                None,
            )
            .into()),
        }
    }
    fn read(
        &self,
        _: ah_vfs_storage::ReadRequest,
        _: ah_vfs_storage::StoragePath,
        _: ah_vfs_storage::IntermediateSymlinks,
    ) -> Result<Reader> {
        Ok(Reader::new(Input(Cell::new(false))))
    }
    fn write(&self, _: String, _: bool, _: bool) -> Result<Writer> {
        Ok(Writer::new(Output {
            fail: self.fail,
            bytes: RefCell::new(vec![]),
        }))
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
    fn commit(&self) -> Result<p::Entry> {
        COMMITS.with(|v| v.set(v.get() + 1));
        if self.fail {
            Err(StorageError::after_dispatch(
                Operation::Write,
                Failure::Backend(BackendFailure::Other),
                None,
            )
            .into())
        } else {
            Ok(entry(self.bytes.borrow().len() as u64))
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
        Op::Write(p::Write {
            handle: storage,
            path: "file".into(),
            replace: false,
            atomic: false,
        }),
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
        Value::Entry(entry(3))
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
        Op::Write(p::Write {
            handle: storage,
            path: "file".into(),
            replace: false,
            atomic: false,
        }),
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
        Op::Write(p::Write {
            handle: storage,
            path: "file".into(),
            replace: false,
            atomic: false,
        }),
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
fn capacity_is_checked_before_open_and_released_ids_are_not_reused() -> TestResult {
    let mut dispatcher = setup()?;
    for _ in 0..p::MAX_RESOURCES {
        open(&mut dispatcher, false)?;
    }
    let request = p::encode_request(
        "generation",
        &Op::Open(p::Open {
            id: "one".into(),
            config: Some(p::Object::new()),
        }),
        vec![],
    )?;
    assert_eq!(
        p::decode_reply(&dispatcher.call(&request))?,
        Err(p::Failure::new(p::FailureCode::LifecycleCapacityExceeded))
    );
    assert_eq!(OPENS.with(Cell::get), p::MAX_RESOURCES);
    call(
        &mut dispatcher,
        Op::Release(p::ResourceRef {
            kind: p::ResourceKind::Storage as i32,
            handle: 1,
        }),
        vec![],
    )?;
    assert_eq!(open(&mut dispatcher, false)?, p::MAX_RESOURCES as u64 + 1);
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
