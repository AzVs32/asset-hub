use ah_vfs::{
    application::{dto::*, port::*},
    domain::*,
};
use std::{future::poll_fn, sync::Arc, task::Poll, time::Duration};
use vfs_plugin_host::{PluginDriver, Transport, TransportFactory};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
use vfs_plugin_wasm::WasmTransport;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const INITIALIZE: &str =
    "global.get $initialized if unreachable end i32.const 1 global.set $initialized i32.const 16";
const STAT: &str = "local.get $pos i32.const 6 i32.add i32.load8_u i32.const 104 i32.eq if (loop br 0) unreachable end local.get $pos i32.const 6 i32.add i32.load8_u i32.const 116 i32.eq if unreachable end i32.const 48";
fn fixture(prepare: &str, initialize: &str, stat: &str, commit: &str) -> Result<Vec<u8>> {
    let capabilities = vec![
        p::Capability::Stat as i32,
        p::Capability::RangeRead as i32,
        p::Capability::WriteNew as i32,
    ];
    let description = p::Description {
        major: p::PROTOCOL_MAJOR,
        minor: p::PROTOCOL_MINOR,
        features: p::PROTOCOL_FEATURES.iter().map(|v| (*v).into()).collect(),
        instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
        generation: "overridden".into(),
        key: "wasm.recovery".into(),
        name: "Recovery fixture".into(),
        version: "1.0.0".into(),
        child_mounts: true,
        capabilities: capabilities.clone(),
        ownership: p::Ownership::Host as i32,
        config_version: 1,
        ..Default::default()
    };
    let entry = p::Entry {
        path: "file".into(),
        kind: p::EntryKind::File as i32,
        size: Some(1),
        modified: None,
    };
    let replies = [
        (
            "DESCRIPTION",
            p::Reply::new(Value::Description(description.into())),
        ),
        ("UNIT", p::Reply::unit()),
        (
            "STORAGE",
            p::Reply::new(Value::Storage(p::Storage {
                handle: 1,
                info: Some(p::StorageInfo {
                    capabilities,
                    chunk_size: 65536,
                }),
            })),
        ),
        ("ENTRY", p::Reply::new(Value::Entry(entry))),
        (
            "READER",
            p::Reply::new(Value::Reader(p::Handle { handle: 2 })),
        ),
        (
            "WRITER",
            p::Reply::new(Value::Writer(p::Handle { handle: 2 })),
        ),
        (
            "CHUNK",
            p::Reply {
                result: Some(Value::Chunk(p::Chunk { eof: false })),
                data: vec![0, 255, 1],
            },
        ),
        ("EOF", p::Reply::new(Value::Chunk(p::Chunk { eof: true }))),
    ];
    let mut wat = include_str!("fixtures/recovery.wat").to_owned();
    for (name, reply) in replies {
        let bytes = p::encode_reply(Ok(reply))?;
        let escaped = bytes
            .iter()
            .map(|byte| format!("\\{byte:02x}"))
            .collect::<String>();
        wat = wat
            .replace(&format!("@@{name}@@"), &escaped)
            .replace(&format!("@@{name}_LENGTH@@"), &bytes.len().to_string());
        if name == "READER" || name == "WRITER" {
            let address = if name == "READER" { 3456 } else { 3584 };
            wat = wat.replace(
                &format!("@@{name}_HANDLE@@"),
                &(address + bytes.len() - 1).to_string(),
            );
        }
    }
    let mut bytes = wat::parse_str(
        wat.replace("@@PREPARE@@", prepare)
            .replace("@@INITIALIZE@@", initialize)
            .replace("@@STAT@@", stat)
            .replace("@@COMMIT@@", commit),
    )?;
    let mut resolve = wit_parser::Resolve::default();
    let package = resolve.push_str(
        "plugin.wit",
        include_str!("../../vfs-plugin-protocol/wit/plugin.wit"),
    )?;
    let world = resolve.select_world(&[package], Some("plugin"))?;
    wit_component::embed_component_metadata(
        &mut bytes,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
        false,
    )?;
    Ok(bytes)
}
async fn factory(
    prepare: &str,
    initialize: &str,
    stat: &str,
    commit: &str,
) -> Result<Arc<WasmTransport>> {
    Ok(Arc::new(
        WasmTransport::load(fixture(prepare, initialize, stat, commit)?, vec![]).await?,
    ))
}
async fn generation(transport: &dyn Transport) -> Result<String> {
    match transport
        .call("", Op::Describe(p::Empty {}), vec![])
        .await?
        .into_value()?
    {
        Value::Description(description) => Ok(description.generation),
        _ => Err("description".into()),
    }
}
async fn initialize(transport: &dyn Transport, generation: &str) -> Result {
    transport
        .call(
            generation,
            Op::Initialize(p::Initialize {
                settings: Some(Default::default()),
            }),
            vec![],
        )
        .await?;
    Ok(())
}
async fn probe(transport: &dyn Transport, generation: &str) -> Result {
    transport
        .call(
            generation,
            Op::TestConnection(p::Config {
                config: Some(Default::default()),
            }),
            vec![],
        )
        .await?;
    Ok(())
}
async fn driver(factory: Arc<WasmTransport>) -> Result<PluginDriver> {
    let driver = PluginDriver::connect(factory).await?;
    driver
        .initialize([("setting".into(), "original".into())].into())
        .await?;
    Ok(driver)
}
async fn open(driver: &PluginDriver, id: &str) -> Result<Arc<dyn Storage>> {
    Ok(driver
        .open(OpenContext::new(InstanceId::parse(id)?, Default::default()))
        .await?)
}
async fn stat(
    storage: &dyn Storage,
    path: &str,
) -> std::result::Result<StorageEntry, StorageError> {
    storage
        .stat_entry(StorageStatEntryRequest {
            path: StoragePath::parse(path)?,
        })
        .await
}
fn kind(error: DriverError) -> Result<StorageErrorKind> {
    Ok(error.storage_error().ok_or("storage error")?.kind())
}

#[tokio::test]
async fn control_trap_uses_a_new_factory_instance_and_reinitializes_settings() -> Result {
    let factory = factory("unreachable", INITIALIZE, STAT, "i32.const 48").await?;
    let old = factory.clone().connect().await?;
    let old_generation = generation(old.as_ref()).await?;
    initialize(old.as_ref(), &old_generation).await?;
    assert_eq!(
        kind(
            old.call(
                &old_generation,
                Op::Prepare(p::Config {
                    config: Some(Default::default())
                }),
                vec![]
            )
            .await
            .err()
            .ok_or("trap")?
        )?,
        StorageErrorKind::StaleHandle
    );
    assert!(
        old.disconnected()
            .ok_or("status")?
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert!(generation(old.as_ref()).await.is_err());
    let fresh = factory.clone().connect().await?;
    let fresh_generation = generation(fresh.as_ref()).await?;
    assert_ne!(old_generation, fresh_generation);
    initialize(fresh.as_ref(), &fresh_generation).await?;
    probe(fresh.as_ref(), &fresh_generation).await?;
    let driver = driver(factory).await?;
    let retained = open(&driver, "retained").await?;
    let provisioner = driver.provisioner();
    assert!(provisioner.prepare(Default::default()).await.is_err());
    assert!(!retained.is_stale());
    assert_eq!(stat(retained.as_ref(), "file").await?.size, Some(1));
    provisioner.test_connection(&Default::default()).await?;
    assert_eq!(
        stat(open(&driver, "recovered").await?.as_ref(), "file")
            .await?
            .size,
        Some(1)
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn storage_timeout_is_local_and_other_stores_make_progress_concurrently() -> Result {
    let driver = driver(factory("i32.const 16", INITIALIZE, STAT, "i32.const 48").await?).await?;
    let stalled = open(&driver, "stalled").await?;
    let healthy = open(&driver, "healthy").await?;
    let mut old_reader = stalled
        .read_file(StorageReadFileRequest::new(StoragePath::parse("file")?))
        .await?;
    let mut call = Box::pin(stat(stalled.as_ref(), "hang"));
    poll_fn(|cx| {
        assert!(call.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(stat(healthy.as_ref(), "file").await?.size, Some(1));
    let third = open(&driver, "third").await?;
    assert_eq!(stat(third.as_ref(), "file").await?.size, Some(1));
    assert!(!healthy.is_stale());
    assert!(!stalled.is_stale());
    tokio::time::advance(Duration::from_secs(31)).await;
    assert_eq!(
        call.await.err().ok_or("timeout")?.kind(),
        StorageErrorKind::TimedOut
    );
    assert!(stalled.is_stale());
    assert!(!healthy.is_stale());
    assert_eq!(
        old_reader
            .next_chunk()
            .await
            .err()
            .ok_or("old reader")?
            .kind(),
        StorageErrorKind::StaleHandle
    );
    assert_eq!(stat(healthy.as_ref(), "file").await?.size, Some(1));
    let reopened = open(&driver, "stalled").await?;
    assert_eq!(stat(reopened.as_ref(), "file").await?.size, Some(1));
    assert!(stalled.is_stale());
    Ok(())
}

#[tokio::test]
async fn traps_and_cancellation_retire_only_the_owning_store() -> Result {
    let driver = driver(factory("i32.const 16", INITIALIZE, STAT, "i32.const 48").await?).await?;
    let trapped = open(&driver, "trapped").await?;
    let healthy = open(&driver, "healthy").await?;
    assert_eq!(
        stat(trapped.as_ref(), "trap")
            .await
            .err()
            .ok_or("trap")?
            .kind(),
        StorageErrorKind::StaleHandle
    );
    assert!(trapped.is_stale());
    assert!(!healthy.is_stale());
    let cancelled = open(&driver, "cancelled").await?;
    let mut call = Box::pin(stat(cancelled.as_ref(), "hang"));
    poll_fn(|cx| {
        assert!(call.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(call);
    assert!(cancelled.is_stale());
    assert!(!healthy.is_stale());
    assert_eq!(stat(healthy.as_ref(), "file").await?.size, Some(1));
    assert_eq!(
        stat(open(&driver, "trapped").await?.as_ref(), "file")
            .await?
            .size,
        Some(1)
    );
    Ok(())
}

#[tokio::test]
async fn uncertain_commit_is_terminal_and_does_not_poison_other_storages() -> Result {
    let driver = driver(factory("i32.const 16", INITIALIZE, STAT, "unreachable").await?).await?;
    let storage = open(&driver, "writer").await?;
    let healthy = open(&driver, "healthy").await?;
    let mut writer = storage
        .write_file(StorageWriteFileRequest::new(StoragePath::parse("file")?))
        .await?;
    writer.write_chunk(b"pending").await?;
    assert_eq!(
        writer.commit().await.err().ok_or("commit trap")?.kind(),
        StorageErrorKind::OutcomeUnknown
    );
    assert_eq!(
        writer.commit().await.err().ok_or("terminal")?.kind(),
        StorageErrorKind::SessionClosed
    );
    assert!(storage.is_stale());
    assert!(!healthy.is_stale());
    assert_eq!(stat(healthy.as_ref(), "file").await?.size, Some(1));
    Ok(())
}

#[tokio::test]
async fn poisoned_stores_release_capacity_even_when_stale_proxies_are_retained() -> Result {
    let driver = driver(factory("i32.const 16", INITIALIZE, STAT, "i32.const 48").await?).await?;
    let mut retained = vec![];
    for _ in 0..20 {
        let storage = open(&driver, "faulted").await?;
        assert!(stat(storage.as_ref(), "trap").await.is_err());
        retained.push(storage);
    }
    assert_eq!(
        stat(open(&driver, "fresh").await?.as_ref(), "file")
            .await?
            .size,
        Some(1)
    );
    assert!(retained.iter().all(|storage| storage.is_stale()));
    Ok(())
}

#[tokio::test]
async fn live_store_capacity_is_bounded_and_returned_when_storage_is_dropped() -> Result {
    let driver = driver(factory("i32.const 16", INITIALIZE, STAT, "i32.const 48").await?).await?;
    let mut retained = vec![];
    for _ in 0..vfs_plugin_wasm::MAX_STORAGE_INSTANCES {
        retained.push(open(&driver, "retained").await?);
    }
    let error = driver
        .open(OpenContext::new(
            InstanceId::parse("excess")?,
            Default::default(),
        ))
        .await
        .err()
        .ok_or("capacity")?;
    assert_eq!(kind(error)?, StorageErrorKind::CapacityExceeded);
    retained.pop();
    let replacement_id = InstanceId::parse("replacement")?;
    let replacement = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match driver
                .open(OpenContext::new(replacement_id.clone(), Default::default()))
                .await
            {
                Ok(storage) => return Ok::<_, DriverError>(storage),
                Err(error)
                    if error.storage_error().is_some_and(|error| {
                        error.kind() == StorageErrorKind::CapacityExceeded
                    }) =>
                {
                    tokio::task::yield_now().await
                }
                Err(error) => return Err(error),
            }
        }
    })
    .await??;
    assert_eq!(stat(replacement.as_ref(), "file").await?.size, Some(1));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn control_timeout_and_cancellation_require_a_fresh_control_instance() -> Result {
    let factory = factory("(loop br 0) unreachable", INITIALIZE, STAT, "i32.const 48").await?;
    let control = factory.clone().connect().await?;
    let old = generation(control.as_ref()).await?;
    initialize(control.as_ref(), &old).await?;
    let mut call = control.call(
        &old,
        Op::Prepare(p::Config {
            config: Some(Default::default()),
        }),
        vec![],
    );
    poll_fn(|cx| {
        assert!(call.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    tokio::time::advance(Duration::from_secs(31)).await;
    assert_eq!(
        kind(call.await.err().ok_or("timeout")?)?,
        StorageErrorKind::TimedOut
    );
    assert!(generation(control.as_ref()).await.is_err());
    let fresh = factory.clone().connect().await?;
    let new = generation(fresh.as_ref()).await?;
    assert_ne!(old, new);
    initialize(fresh.as_ref(), &new).await?;
    let mut call = fresh.call(
        &new,
        Op::Prepare(p::Config {
            config: Some(Default::default()),
        }),
        vec![],
    );
    poll_fn(|cx| {
        assert!(call.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(call);
    assert!(generation(fresh.as_ref()).await.is_err());
    Ok(())
}

#[tokio::test]
async fn initialize_acknowledges_identical_settings_without_reexecuting_the_guest() -> Result {
    let factory = factory("i32.const 16", INITIALIZE, STAT, "i32.const 48").await?;
    let control = factory.connect().await?;
    let generation = generation(control.as_ref()).await?;
    initialize(control.as_ref(), &generation).await?;
    initialize(control.as_ref(), &generation).await?;
    let error = control
        .call(
            &generation,
            Op::Initialize(p::Initialize {
                settings: Some(p::Object::from([("different".into(), true.into())])),
            }),
            vec![],
        )
        .await
        .err()
        .ok_or("settings conflict")?;
    assert_eq!(kind(error)?, StorageErrorKind::Conflict);
    probe(control.as_ref(), &generation).await?;
    Ok(())
}

struct CountedFactory {
    inner: Arc<WasmTransport>,
    connections: std::sync::atomic::AtomicUsize,
}
impl TransportFactory for CountedFactory {
    fn connect(self: Arc<Self>) -> vfs_plugin_host::ConnectFuture {
        Box::pin(async move {
            self.connections
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.clone().connect().await
        })
    }
}
#[tokio::test]
async fn failed_storage_open_does_not_replace_a_healthy_control_instance() -> Result {
    let factory = Arc::new(CountedFactory {
        inner: factory("i32.const 16", INITIALIZE, STAT, "i32.const 48").await?,
        connections: std::sync::atomic::AtomicUsize::new(0),
    });
    let driver = PluginDriver::connect(factory.clone()).await?;
    driver.initialize(Default::default()).await?;
    let healthy = open(&driver, "healthy").await?;
    let error = driver
        .open(OpenContext::new(
            InstanceId::parse("bad")?,
            Default::default(),
        ))
        .await
        .err()
        .ok_or("opening trap")?;
    assert_eq!(kind(error)?, StorageErrorKind::Unavailable);
    assert_eq!(
        factory
            .connections
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(stat(healthy.as_ref(), "file").await?.size, Some(1));
    assert_eq!(
        stat(open(&driver, "fresh").await?.as_ref(), "file")
            .await?
            .size,
        Some(1)
    );
    assert!(!healthy.is_stale());
    Ok(())
}
#[tokio::test]
async fn storage_and_reader_keep_the_compiled_factory_alive_after_driver_drop() -> Result {
    let driver = driver(factory("i32.const 16", INITIALIZE, STAT, "i32.const 48").await?).await?;
    let storage = open(&driver, "retained").await?;
    drop(driver);
    assert_eq!(stat(storage.as_ref(), "file").await?.size, Some(1));
    let mut reader = storage
        .read_file(StorageReadFileRequest::new(StoragePath::parse("file")?))
        .await?;
    drop(storage);
    assert_eq!(reader.next_chunk().await?, Some(vec![0, 255, 1]));
    assert_eq!(reader.next_chunk().await?, None);
    Ok(())
}
