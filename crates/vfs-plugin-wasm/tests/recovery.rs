use ah_vfs::{application::port::*, domain::*};
use std::{future::poll_fn, task::Poll, time::Duration};
use vfs_plugin_host::{PluginDriver, Transport};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
use vfs_plugin_wasm::WasmTransport;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn fixture(prepare: &str, initialize: &str) -> Result<Vec<u8>> {
    let description = p::encode_reply(Ok(p::Reply::new(Value::Description(
        p::Description {
            major: p::PROTOCOL_MAJOR,
            minor: p::PROTOCOL_MINOR,
            required_features: vec![],
            features: p::PROTOCOL_FEATURES.iter().map(|v| (*v).into()).collect(),
            required_host_capabilities: vec![],
            instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
            generation: "overridden".into(),
            key: "wasm.recovery".into(),
            name: "Recovery fixture".into(),
            version: "1.0.0".into(),
            child_mounts: true,
            capabilities: vec![],
            ownership: p::Ownership::Host as i32,
            settings_schema: None,
            instance_schema: None,
            config_version: 1,
        }
        .into(),
    ))))?;
    let unit = p::encode_reply(Ok(p::Reply::unit()))?;
    let unit_escaped = unit
        .iter()
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<String>();
    let escaped = description
        .iter()
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<String>();
    let wat = include_str!("fixtures/recovery.wat")
        .replace("@@DESCRIPTION@@", &escaped)
        .replace("@@DESCRIPTION_LENGTH@@", &description.len().to_string())
        .replace("@@UNIT@@", &unit_escaped)
        .replace("@@UNIT_LENGTH@@", &unit.len().to_string())
        .replace("@@PREPARE@@", prepare)
        .replace("@@INITIALIZE@@", initialize);
    let mut bytes = wat::parse_str(wat)?;
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
async fn generation(transport: &WasmTransport) -> Result<String> {
    let reply = transport
        .call("", Op::Describe(p::Empty {}), vec![])
        .await?;
    match reply.into_value()? {
        Value::Description(description) => Ok(description.generation),
        _ => Err("description".into()),
    }
}
async fn initialize(transport: &WasmTransport, generation: &str) -> Result {
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
async fn probe(transport: &WasmTransport, generation: &str) -> Result {
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

#[tokio::test]
async fn trap_rebuilds_the_store_and_host_provisioning_recovers() -> Result {
    let transport = std::sync::Arc::new(
        WasmTransport::load(fixture("unreachable", "i32.const 16")?, vec![]).await?,
    );
    let old = generation(&transport).await?;
    let driver = PluginDriver::connect(transport.clone()).await?;
    driver.initialize(Default::default()).await?;
    let InstanceManagement::HostManaged(provisioner) = driver.instances() else {
        return Err("ownership".into());
    };
    let config = Default::default();
    assert_eq!(
        provisioner
            .prepare(config)
            .await
            .err()
            .ok_or("trap")?
            .storage_error()
            .ok_or("category")?
            .kind(),
        StorageErrorKind::StaleHandle
    );
    // The next independent request performs recovery; the failed operation is
    // returned to its caller and is never replayed against the fresh store.
    provisioner.test_connection(&Default::default()).await?;
    assert_ne!(generation(&transport).await?, old);
    assert_eq!(
        transport
            .call(
                &old,
                Op::TestConnection(p::Config {
                    config: Some(Default::default())
                }),
                vec![]
            )
            .await
            .err()
            .ok_or("old generation")?
            .storage_error()
            .ok_or("category")?
            .kind(),
        StorageErrorKind::StaleHandle
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn timeout_rebuilds_instead_of_reusing_incomplete_guest_execution() -> Result {
    let transport =
        WasmTransport::load(fixture("(loop br 0) unreachable", "i32.const 16")?, vec![]).await?;
    let old = generation(&transport).await?;
    initialize(&transport, &old).await?;
    let mut call = transport.call(
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
        call.await
            .err()
            .ok_or("timeout")?
            .storage_error()
            .ok_or("category")?
            .kind(),
        StorageErrorKind::TimedOut
    );
    let new = generation(&transport).await?;
    assert_ne!(old, new);
    initialize(&transport, &new).await?;
    probe(&transport, &new).await?;
    Ok(())
}

#[tokio::test]
async fn cancelled_execution_and_mutation_traps_also_allow_a_new_generation() -> Result {
    let transport =
        WasmTransport::load(fixture("(loop br 0) unreachable", "i32.const 16")?, vec![]).await?;
    let old = generation(&transport).await?;
    initialize(&transport, &old).await?;
    let mut call = transport.call(
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
    drop(call);
    let new = generation(&transport).await?;
    assert_ne!(old, new);
    initialize(&transport, &new).await?;
    probe(&transport, &new).await?;

    let transport = WasmTransport::load(fixture("unreachable", "unreachable")?, vec![]).await?;
    let old = generation(&transport).await?;
    let error = transport
        .call(
            &old,
            Op::Initialize(p::Initialize {
                settings: Some(p::Object::new()),
            }),
            vec![],
        )
        .await
        .err()
        .ok_or("mutation trap")?;
    assert_eq!(
        error.storage_error().ok_or("category")?.kind(),
        StorageErrorKind::OutcomeUnknown
    );
    assert_ne!(generation(&transport).await?, old);
    Ok(())
}

#[tokio::test]
async fn initialize_acknowledges_identical_settings_without_reexecuting_the_guest() -> Result {
    let body = "global.get $initialized if unreachable end i32.const 1 global.set $initialized i32.const 16";
    let transport = WasmTransport::load(fixture("unreachable", body)?, vec![]).await?;
    let generation = generation(&transport).await?;
    initialize(&transport, &generation).await?;
    initialize(&transport, &generation).await?;
    let error = transport
        .call(
            &generation,
            Op::Initialize(p::Initialize {
                settings: Some(p::Object::from([("different".into(), true.into())])),
            }),
            vec![],
        )
        .await
        .err()
        .ok_or("different settings")?;
    assert_eq!(
        error.storage_error().ok_or("category")?.kind(),
        StorageErrorKind::Conflict
    );
    probe(&transport, &generation).await?;
    Ok(())
}
