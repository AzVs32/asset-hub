//! Public-port parity first; transport-specific failures have explicitly scoped
//! assertions because a native unwind, RPC disconnect and WASM trap differ.
mod support;
use ah_vfs::{
    application::{driver_catalog::DriverCatalog, dto::*, port::*},
    domain::*,
};
use std::{num::NonZeroUsize, sync::Arc, time::Duration};
use support::*;
use vfs_plugin_host::Transport;
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

#[tokio::test]
async fn same_declarations_preparation_and_migration() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        assert_eq!(f.driver.info().key().as_str(), "conformance", "{mode:?}");
        assert_eq!(f.driver.info().name(), "ConformanceDriver");
        assert_eq!(
            f.driver.info().capabilities(),
            vfs_plugin_codec::decode_capabilities(&conformance_model::capabilities(false))?
        );
        let InstanceManagement::HostManaged(provisioner) = f.driver.instances() else {
            panic!("ownership changed")
        };
        let config = f.config("full");
        let prepared = provisioner.prepare(config.clone()).await?;
        assert_eq!(prepared.config(), &config);
        assert!(prepared.details().is_empty());
        assert_eq!(provisioner.migrate_config(1, config.clone()).await?, config);
        provisioner.test_connection(&config).await?;
        let bad: InstanceConfig = [("profile".into(), "not-a-profile-private-value".into())].into();
        let error = provisioner
            .prepare(bad)
            .await
            .err()
            .ok_or("invalid config accepted")?;
        assert!(
            matches!(error, DriverError::InvalidInstanceConfig { ref field, kind: InstanceConfigErrorKind::InvalidValue } if field == "profile")
        );
        assert!(!format!("{error:?}").contains("private-value"));
        assert_eq!(
            category(
                &provisioner
                    .migrate_config(2, config)
                    .await
                    .err()
                    .ok_or("migration accepted")?
            ),
            Some(StorageErrorKind::UnsupportedOperation)
        );
    }
    Ok(())
}

#[tokio::test]
async fn binary_ranges_eof_and_metadata_match() -> Result {
    let mut observations = vec![];
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let storage = f.open("full").await?;
        let metadata = stat(storage.as_ref(), "seed").await?;
        assert_eq!(metadata.size, Some(10));
        assert_eq!(metadata.kind, StorageEntryKind::File);
        assert_eq!(
            storage.chunk_size().get(),
            conformance_model::CHUNK as usize
        );
        assert!(storage.permissions().is_none() && storage.xattrs().is_none());
        let mut reader = storage
            .read_file(StorageReadFileRequest {
                path: path("seed"),
                offset: 2,
                length: Some(5),
            })
            .await?;
        let mut chunks = vec![];
        while let Some(chunk) = reader.next_chunk().await? {
            chunks.push(chunk);
        }
        assert!(reader.next_chunk().await?.is_none());
        assert_eq!(chunks.concat(), b"23456");
        for (offset, length) in [(u64::MAX, None), (0, Some(0))] {
            let mut empty = storage
                .read_file(StorageReadFileRequest {
                    path: path("seed"),
                    offset,
                    length,
                })
                .await?;
            assert!(empty.next_chunk().await?.is_none());
            assert!(empty.next_chunk().await?.is_none());
        }
        observations.push((chunks, metadata.modified));
    }
    assert!(observations.windows(2).all(|w| w[0] == w[1]));
    Ok(())
}

#[tokio::test]
async fn staging_publication_replacement_and_live_pagination_match() -> Result {
    let payload = [0, 255, 1, 0, 128, 7, 9];
    let mut transcripts = vec![];
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let storage = f.open("full").await?;
        let mut w = writer(storage.as_ref(), "new").await?;
        w.write_chunk(&payload[..2]).await?;
        w.write_chunk(&payload[2..]).await?;
        assert_eq!(
            kind(stat(storage.as_ref(), "new").await),
            StorageErrorKind::NotFound
        );
        assert_eq!(w.commit().await?.size, Some(payload.len() as u64));
        assert_eq!(read(storage.as_ref(), "new").await?, payload);
        assert_eq!(kind(w.commit().await), StorageErrorKind::SessionClosed);
        assert_eq!(kind(w.abort().await), StorageErrorKind::SessionClosed);
        assert_eq!(
            kind(w.write_chunk(b"late").await),
            StorageErrorKind::SessionClosed
        );
        assert_eq!(
            kind(
                storage
                    .write_file(StorageWriteFileRequest::new(path("new")))
                    .await
            ),
            StorageErrorKind::AlreadyExists
        );
        let mut replacement = storage
            .write_file(StorageWriteFileRequest {
                path: path("new"),
                mode: StorageWriteFileMode::Replace,
                require_atomic_publish: true,
            })
            .await?;
        replacement.write_chunk(b"replacement").await?;
        assert_eq!(read(storage.as_ref(), "new").await?, payload);
        replacement.commit().await?;
        assert_eq!(read(storage.as_ref(), "new").await?, b"replacement");
        let mut names = vec![];
        let mut after = None;
        loop {
            let page = storage
                .list_entries(StorageDirQuery {
                    path: path(""),
                    after,
                    limit: NonZeroUsize::MIN,
                })
                .await?;
            assert!(page.entries.len() <= 1);
            names.extend(page.entries.iter().map(|e| e.path.as_str().to_owned()));
            let Some(next) = page.next_after else { break };
            assert_eq!(
                page.entries.last().map(|e| e.path.as_str()),
                Some(next.as_str())
            );
            after = Some(next);
        }
        assert_eq!(names, ["fail-next", "new", "seed", "wait-read"]);
        storage
            .remove_entry(StorageRemoveEntryRequest { path: path("new") })
            .await?;
        assert_eq!(
            kind(stat(storage.as_ref(), "new").await),
            StorageErrorKind::NotFound
        );
        transcripts.push(names);
    }
    assert!(transcripts.windows(2).all(|w| w[0] == w[1]));
    Ok(())
}

#[tokio::test]
async fn effective_capabilities_and_unsupported_operations_agree() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let storage = f.open("read-only").await?;
        assert!(
            f.driver
                .info()
                .capabilities()
                .supports(storage.capabilities())
        );
        assert!(!storage.capabilities().write_file.create_new);
        assert!(!storage.capabilities().create_dir);
        assert_eq!(
            kind(
                storage
                    .write_file(StorageWriteFileRequest::new(path("absent")))
                    .await
            ),
            StorageErrorKind::UnsupportedOperation
        );
        assert_eq!(
            kind(
                storage
                    .create_dir(StorageCreateDirRequest {
                        path: path("dir"),
                        recursive: false
                    })
                    .await
            ),
            StorageErrorKind::UnsupportedOperation
        );
        assert_eq!(
            kind(
                storage
                    .move_entry(StorageMoveEntryRequest {
                        source: path("seed"),
                        destination: path("moved"),
                        replace: false
                    })
                    .await
            ),
            StorageErrorKind::UnsupportedOperation
        );
        assert_eq!(
            kind(
                storage
                    .remove_entry(StorageRemoveEntryRequest { path: path("seed") })
                    .await
            ),
            StorageErrorKind::UnsupportedOperation
        );
        assert_eq!(metric(storage.as_ref(), "writer").await?, 0);
        assert_eq!(
            read(storage.as_ref(), "seed").await?,
            conformance_model::SEED
        );
        assert_eq!(
            category(
                f.open("escalated")
                    .await
                    .err()
                    .ok_or("false capability accepted")?
                    .as_ref()
            ),
            Some(StorageErrorKind::InvalidData)
        );
    }
    Ok(())
}

#[tokio::test]
async fn domain_error_categories_survive_every_adapter() -> Result {
    use StorageErrorKind::*;
    let cases = [
        ("not_found", NotFound),
        ("already_exists", AlreadyExists),
        ("permission_denied", PermissionDenied),
        ("invalid_data", InvalidData),
        ("unsupported_operation", UnsupportedOperation),
        ("timed_out", TimedOut),
        ("unavailable", Unavailable),
        ("cancelled", Cancelled),
        ("capacity_exceeded", CapacityExceeded),
        ("conflict", Conflict),
        ("outcome_unknown", OutcomeUnknown),
        ("backend", Backend),
        ("future_remote_error", Backend),
    ];
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        for (code, expected) in cases {
            let error = stat(s.as_ref(), &format!("error-{code}"))
                .await
                .err()
                .ok_or("error became success")?;
            assert_eq!(error.kind(), expected, "{mode:?}: {code}");
            if mode != Mode::Native {
                assert!(!format!("{error:?}").contains("private-diagnostic"));
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn failed_and_uncertain_writes_are_terminal() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        for (name, operation) in [
            ("fail-write", "write"),
            ("fail-commit", "commit"),
            ("fail-abort", "abort"),
        ] {
            let mut w = writer(s.as_ref(), name).await?;
            let error = match operation {
                "write" => w.write_chunk(b"data").await.err(),
                "commit" => w.commit().await.err(),
                _ => w.abort().await.err(),
            }
            .ok_or("injected error missing")?;
            assert_eq!(error.kind(), StorageErrorKind::Backend, "{mode:?}: {name}");
            assert_eq!(
                kind(w.write_chunk(b"late").await),
                StorageErrorKind::SessionClosed
            );
            assert_eq!(kind(w.commit().await), StorageErrorKind::SessionClosed);
            assert_eq!(kind(w.abort().await), StorageErrorKind::SessionClosed);
            assert_eq!(
                kind(stat(s.as_ref(), name).await),
                StorageErrorKind::NotFound
            );
        }
        let mut w = writer(s.as_ref(), "uncertain-commit").await?;
        w.write_chunk(b"published-once").await?;
        assert_eq!(kind(w.commit().await), StorageErrorKind::OutcomeUnknown);
        assert_eq!(kind(w.commit().await), StorageErrorKind::SessionClosed);
        assert_eq!(
            read(s.as_ref(), "uncertain-commit").await?,
            b"published-once"
        );
        assert_eq!(metric(s.as_ref(), "commits").await?, 1);
    }
    Ok(())
}

#[tokio::test]
async fn unpolled_futures_do_not_change_writer_state() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        let mut w = writer(s.as_ref(), "unpolled").await?;
        drop(w.commit());
        drop(w.abort());
        drop(w.write_chunk(b"discarded"));
        w.write_chunk(b"kept").await?;
        w.commit().await?;
        assert_eq!(read(s.as_ref(), "unpolled").await?, b"kept");
    }
    Ok(())
}

#[tokio::test]
async fn drop_disposes_pending_writes_and_independent_reader_resources() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let monitor = f.open("full").await?;
        let s = f.open("full").await?;
        let mut w = writer(s.as_ref(), "abandoned").await?;
        w.write_chunk(b"never published").await?;
        assert_eq!(metric(monitor.as_ref(), "writer").await?, 1);
        drop(w);
        eventually_metric(monitor.as_ref(), "writer", 0).await?;
        assert_eq!(
            kind(stat(monitor.as_ref(), "abandoned").await),
            StorageErrorKind::NotFound
        );
        let mut reader = s
            .read_file(StorageReadFileRequest::new(path("seed")))
            .await?;
        drop(s);
        eventually_metric(monitor.as_ref(), "storage", 1).await?;
        assert_eq!(reader.next_chunk().await?.as_deref(), Some(&b"012"[..]));
        drop(reader);
        eventually_metric(monitor.as_ref(), "reader", 0).await?;
        assert_eq!(metric(monitor.as_ref(), "commits").await?, 0);
    }
    Ok(())
}

#[tokio::test]
async fn reader_failure_closes_session_and_releases_resource() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        let mut reader = s
            .read_file(StorageReadFileRequest::new(path("fail-next")))
            .await?;
        assert_eq!(kind(reader.next_chunk().await), StorageErrorKind::Backend);
        assert_eq!(
            kind(reader.next_chunk().await),
            StorageErrorKind::SessionClosed
        );
        eventually_metric(s.as_ref(), "reader", 0).await?;
    }
    Ok(())
}

#[tokio::test]
async fn dispatched_reader_cancellation_closes_local_session() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        let mut reader = s
            .read_file(StorageReadFileRequest::new(path("wait-read")))
            .await?;
        let mut pending = reader.next_chunk();
        tokio::select! {
            result = &mut pending => panic!("read completed before cancellation: {result:?}"),
            entered = f.gate.entered("read") => entered?,
        }
        assert_eq!(s.chunk_size().get(), 3);
        drop(pending);
        assert_eq!(
            kind(reader.next_chunk().await),
            StorageErrorKind::SessionClosed
        );
        f.gate.release("read");
        if mode == Mode::Wasm {
            // Dropping a suspended component call makes its whole store unusable.
            assert_eq!(
                kind(stat(s.as_ref(), "seed").await),
                StorageErrorKind::StaleHandle
            );
        } else {
            eventually_metric(s.as_ref(), "reader", 0).await?;
            assert_eq!(read(s.as_ref(), "seed").await?, conformance_model::SEED);
        }
    }
    Ok(())
}

#[tokio::test]
async fn dispatched_commit_and_abort_cancellation_are_terminal() -> Result {
    for mode in MODES {
        for phase in ["commit", "abort", "reply"] {
            let f = Fixture::new(mode).await?;
            let s = f.open("full").await?;
            let name = format!("wait-{phase}");
            let mut w = writer(s.as_ref(), &name).await?;
            w.write_chunk(b"once").await?;
            if phase == "abort" {
                let mut pending = w.abort();
                tokio::select! { result = &mut pending => panic!("abort completed: {result:?}"), entered = f.gate.entered(phase) => entered? }
                drop(pending);
            } else {
                let mut pending = w.commit();
                tokio::select! { result = &mut pending => panic!("commit completed: {result:?}"), entered = f.gate.entered(phase) => entered? }
                drop(pending);
            }
            assert_eq!(
                kind(w.commit().await),
                StorageErrorKind::SessionClosed,
                "{mode:?}/{phase}"
            );
            assert_eq!(kind(w.abort().await), StorageErrorKind::SessionClosed);
            assert_eq!(
                kind(w.write_chunk(b"retry").await),
                StorageErrorKind::SessionClosed
            );
            f.gate.release(phase);
            if mode == Mode::Wasm {
                assert_eq!(
                    kind(stat(s.as_ref(), "seed").await),
                    StorageErrorKind::StaleHandle
                );
            } else {
                eventually_metric(s.as_ref(), "writer", 0).await?;
                // Cancellation never promises rollback or permits automatic replay.
                assert!(metric(s.as_ref(), "commits").await? <= 1);
                if phase == "reply" {
                    assert_eq!(read(s.as_ref(), &name).await?, b"once");
                }
                if phase == "abort" {
                    assert_eq!(
                        kind(stat(s.as_ref(), &name).await),
                        StorageErrorKind::NotFound
                    );
                }
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn grpc_process_loss_during_commit_is_outcome_unknown() -> Result {
    let mut f = Fixture::new(Mode::Grpc).await?;
    let s = f.open("full").await?;
    let mut w = writer(s.as_ref(), "wait-commit").await?;
    let mut pending = w.commit();
    tokio::select! { result = &mut pending => panic!("commit completed: {result:?}"), entered = f.gate.entered("commit") => entered? }
    f.kill().await?;
    assert_eq!(
        kind(tokio::time::timeout(DEADLINE, pending).await?),
        StorageErrorKind::OutcomeUnknown
    );
    assert_eq!(kind(w.commit().await), StorageErrorKind::SessionClosed);
    assert_eq!(
        kind(stat(s.as_ref(), "seed").await),
        StorageErrorKind::Unavailable
    );
    Ok(())
}

#[tokio::test]
async fn exceptions_are_contained_at_the_adapter_boundary() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        let copy = s.clone();
        let call = tokio::spawn(async move { stat(copy.as_ref(), "panic").await });
        match mode {
            Mode::Native => assert!(call.await.err().ok_or("native panic swallowed")?.is_panic()),
            Mode::Grpc => {
                assert_eq!(
                    kind(tokio::time::timeout(DEADLINE, call).await??),
                    StorageErrorKind::Unavailable
                );
                assert_eq!(
                    kind(stat(s.as_ref(), "seed").await),
                    StorageErrorKind::StaleHandle
                );
            }
            Mode::Wasm => {
                assert_eq!(
                    kind(tokio::time::timeout(DEADLINE, call).await??),
                    StorageErrorKind::StaleHandle
                );
                assert_eq!(
                    kind(stat(s.as_ref(), "seed").await),
                    StorageErrorKind::StaleHandle
                );
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn plugin_loader_accepts_a_new_backend_from_configuration_only() -> Result {
    use vfs_plugin_loader::{PluginConfig, PluginLoader};
    if std::env::var_os("CONFORMANCE_LOADER_CHILD").is_none() {
        // Set credentials before a fresh process starts, without unsafe global
        // environment mutation or a requirement on the developer's environment.
        let output = tokio::task::spawn_blocking(|| {
            std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "plugin_loader_accepts_a_new_backend_from_configuration_only",
                    "--nocapture",
                ])
                .env("CONFORMANCE_LOADER_CHILD", "1")
                .env("CONFORMANCE_LOADER_TOKEN", TOKEN)
                .output()
        })
        .await??;
        assert!(
            output.status.success(),
            "isolated loader test failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    for mode in [Mode::Grpc, Mode::Wasm] {
        let f = Fixture::uninitialized(mode).await?;
        let directory = tempfile::tempdir()?;
        let source = match mode {
            Mode::Grpc => format!(
                "transport = 'grpc'\nendpoint = {}\ntoken_env = 'CONFORMANCE_LOADER_TOKEN'\n",
                serde_json::to_string(f.endpoint.as_ref().ok_or("missing endpoint")?)?
            ),
            Mode::Wasm => {
                tokio::fs::copy(artifact(), directory.path().join("guest.wasm")).await?;
                format!(
                    "transport = 'wasm'\nmodule = 'guest.wasm'\nprofile = '{}'\n[vfs.plugins.entries.grants]\nhttp_origins = [{}]\n",
                    p::WASM_HOST_PROFILE,
                    serde_json::to_string(&f.gate.origin)?
                )
            }
            _ => unreachable!(),
        };
        let file = directory.path().join("host.toml");
        tokio::fs::write(
            &file,
            format!("[[vfs.plugins.entries]]\nexpected_key = 'conformance'\n{source}"),
        )
        .await?;
        let loaded = ah_config::Registry::for_space("default")?.load_required(file)?;
        assert_eq!(loaded.get::<PluginConfig>()?.entries.len(), 1);
        let catalog = PluginLoader::load_from(&loaded, DriverCatalog::new()).await?;
        let driver = catalog
            .get(&DriverKey::parse("conformance")?)
            .ok_or("driver not loaded")?;
        let s = driver
            .open(OpenContext::host_managed(
                InstanceId::parse("instance")?,
                f.config("full"),
            ))
            .await?;
        assert_eq!(read(s.as_ref(), "seed").await?, conformance_model::SEED);
    }
    Ok(())
}

#[tokio::test]
async fn cancelled_write_chunk_is_closed_and_never_published() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        let mut w = writer(s.as_ref(), "wait-write").await?;
        let mut pending = w.write_chunk(b"not published");
        tokio::select! { result = &mut pending => panic!("write completed: {result:?}"), entered = f.gate.entered("write") => entered? }
        drop(pending);
        assert_eq!(kind(w.commit().await), StorageErrorKind::SessionClosed);
        f.gate.release("write");
        if mode == Mode::Wasm {
            assert_eq!(
                kind(stat(s.as_ref(), "seed").await),
                StorageErrorKind::StaleHandle
            );
        } else {
            eventually_metric(s.as_ref(), "writer", 0).await?;
            assert_eq!(
                kind(stat(s.as_ref(), "wait-write").await),
                StorageErrorKind::NotFound
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn cancelling_a_queued_wasm_call_does_not_poison_the_active_call() -> Result {
    use std::{future::Future, task::Poll};
    let f = Fixture::new(Mode::Wasm).await?;
    let s = f.open("full").await?;
    let mut reader = s
        .read_file(StorageReadFileRequest::new(path("wait-read")))
        .await?;
    let mut active = reader.next_chunk();
    tokio::select! { result = &mut active => panic!("read completed: {result:?}"), entered = f.gate.entered("read") => entered? }
    let mut queued = Box::pin(stat(s.as_ref(), "seed"));
    std::future::poll_fn(|cx| {
        assert!(queued.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(queued);
    f.gate.release("read");
    assert_eq!(active.await?.as_deref(), Some(&b"012"[..]));
    assert_eq!(stat(s.as_ref(), "seed").await?.size, Some(10));
    Ok(())
}

#[tokio::test]
async fn streams_retain_the_runtime_after_driver_and_storage_are_dropped() -> Result {
    for mode in MODES {
        let mut f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        let mut reader = s
            .read_file(StorageReadFileRequest::new(path("seed")))
            .await?;
        drop(s);
        let old = std::mem::replace(
            &mut f.driver,
            Arc::new(conformance_native::ConformanceDriver::new()),
        );
        drop(old);
        f.transport.take();
        let mut bytes = vec![];
        while let Some(chunk) = reader.next_chunk().await? {
            bytes.extend(chunk);
        }
        assert_eq!(bytes, conformance_model::SEED, "{mode:?}");
    }
    Ok(())
}

#[tokio::test]
async fn mutating_plugin_panics_never_report_a_safe_retry() -> Result {
    for mode in [Mode::Grpc, Mode::Wasm] {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        let mut w = writer(s.as_ref(), "panic-commit").await?;
        w.write_chunk(b"may be published").await?;
        assert_eq!(
            kind(tokio::time::timeout(DEADLINE, w.commit()).await?),
            StorageErrorKind::OutcomeUnknown
        );
        assert_eq!(kind(w.commit().await), StorageErrorKind::SessionClosed);
    }
    Ok(())
}

#[tokio::test]
async fn authentication_and_generation_checks_reject_before_dispatch() -> Result {
    for mode in [Mode::Grpc, Mode::Wasm] {
        let f = Fixture::new(mode).await?;
        let transport = f.transport.as_ref().ok_or("missing transport")?;
        let error = transport
            .call(
                "obsolete-generation",
                Op::Open {
                    id: "instance".into(),
                    config: Some(Default::default()),
                },
                vec![],
            )
            .await
            .err()
            .ok_or("wrong generation accepted")?;
        assert_eq!(category(&error), Some(StorageErrorKind::StaleHandle));
        if let Some(endpoint) = &f.endpoint {
            let bad =
                vfs_plugin_grpc::GrpcTransport::connect(endpoint.clone(), "wrong-token").await?;
            let error = bad
                .call("", Op::Describe, vec![])
                .await
                .err()
                .ok_or("unauthenticated description accepted")?;
            assert_eq!(category(&error), Some(StorageErrorKind::PermissionDenied));
        }
        let s = f.open("full").await?;
        assert_eq!(metric(s.as_ref(), "storage").await?, 1);
    }
    Ok(())
}

#[tokio::test]
async fn version_features_schema_and_host_imports_are_negotiated() -> Result {
    use vfs_plugin_host::{PluginDriver, Transport, TransportFuture};
    struct Altered {
        inner: Arc<dyn Transport>,
        fault: u8,
    }
    impl Transport for Altered {
        fn host_capabilities(&self) -> &[&str] {
            self.inner.host_capabilities()
        }
        fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
            Box::pin(async move {
                let mut reply = self.inner.call(generation, op, data).await?;
                if let Value::Description { description } = &mut reply.value {
                    match self.fault {
                        0 => description.major += 1,
                        1 => {
                            description.features.push("future.required.v1".into());
                            description
                                .required_features
                                .push("future.required.v1".into());
                        }
                        2 => description.instance_schema_profile = "future.schema.v2".into(),
                        3 => description
                            .required_host_capabilities
                            .push("filesystem.unsupplied.v1".into()),
                        4 => {
                            description
                                .instance_schema
                                .as_mut()
                                .expect("fixture schema")
                                .insert("$ref".into(), "https://unresolved.example/schema".into());
                        }
                        _ => {
                            description.minor += 1;
                            description.features.push("future.optional.v1".into());
                        }
                    }
                }
                Ok(reply)
            })
        }
    }
    for mode in [Mode::Grpc, Mode::Wasm] {
        let f = Fixture::uninitialized(mode).await?;
        for fault in 0..5 {
            let transport = Arc::new(Altered {
                inner: f.transport.as_ref().ok_or("missing transport")?.clone(),
                fault,
            });
            let error = PluginDriver::connect(transport)
                .await
                .err()
                .ok_or("incompatible peer accepted")?;
            assert_eq!(
                category(&error),
                Some(StorageErrorKind::UnsupportedOperation),
                "{mode:?}/{fault}"
            );
        }
        let accepted = PluginDriver::connect(Arc::new(Altered {
            inner: f.transport.as_ref().ok_or("missing transport")?.clone(),
            fault: 5,
        }))
        .await?;
        assert_eq!(accepted.negotiated_protocol().minor, p::PROTOCOL_MINOR);
        assert!(
            !accepted
                .negotiated_protocol()
                .features
                .contains(&"future.optional.v1".into())
        );
        accepted.initialize(Default::default()).await?;
    }
    Ok(())
}

async fn invalid_release_kind_case(mode: Mode) -> Result {
    let f = Fixture::new(mode).await?;
    let Value::Storage { handle, .. } = f
        .raw(Op::Open {
            id: "instance".into(),
            config: Some(f.config("full").into_object()),
        })
        .await?
        .value
    else {
        panic!("missing storage")
    };
    let error = f
        .raw(Op::Release {
            kind: p::ResourceKind::Reader,
            handle,
        })
        .await
        .err()
        .ok_or("wrong-kind release accepted")?;
    assert_eq!(
        category(error.as_ref()),
        Some(StorageErrorKind::InvalidData),
        "{mode:?}"
    );
    assert!(matches!(
        f.raw(Op::Storage {
            handle,
            request: p::StorageOp::Stat {
                path: "seed".into()
            }
        })
        .await?
        .value,
        Value::Entry { .. }
    ));
    f.raw(Op::Release {
        kind: p::ResourceKind::Storage,
        handle,
    })
    .await?;
    f.raw(Op::Release {
        kind: p::ResourceKind::Storage,
        handle,
    })
    .await?;
    assert_eq!(
        category(
            f.raw(Op::Storage {
                handle,
                request: p::StorageOp::Stat {
                    path: "seed".into()
                }
            })
            .await
            .err()
            .ok_or("released handle usable")?
            .as_ref()
        ),
        Some(StorageErrorKind::SessionClosed)
    );
    Ok(())
}

#[tokio::test]
async fn grpc_invalid_release_kind_preserves_live_resource() -> Result {
    invalid_release_kind_case(Mode::Grpc).await
}

#[tokio::test]
async fn wasm_invalid_release_kind_preserves_live_resource() -> Result {
    invalid_release_kind_case(Mode::Wasm).await
}

#[tokio::test]
async fn multi_frame_write_and_create_new_races_preserve_atomic_publication() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        let mut bytes = vec![0xA5; p::MAX_DATA_BYTES + 7];
        bytes[p::MAX_DATA_BYTES..].copy_from_slice(b"THE-END");
        let mut first = writer(s.as_ref(), "large").await?;
        let mut competing = writer(s.as_ref(), "large").await?;
        first.write_chunk(&bytes).await?;
        competing.write_chunk(b"must not overwrite").await?;
        assert_eq!(
            kind(stat(s.as_ref(), "large").await),
            StorageErrorKind::NotFound
        );
        assert_eq!(first.commit().await?.size, Some(bytes.len() as u64));
        assert_eq!(
            kind(competing.commit().await),
            StorageErrorKind::AlreadyExists
        );
        assert_eq!(
            kind(competing.commit().await),
            StorageErrorKind::SessionClosed
        );
        let mut tail = s
            .read_file(StorageReadFileRequest {
                path: path("large"),
                offset: p::MAX_DATA_BYTES as u64,
                length: Some(7),
            })
            .await?;
        let mut actual = vec![];
        while let Some(chunk) = tail.next_chunk().await? {
            actual.extend(chunk);
        }
        assert_eq!(actual, b"THE-END");
        assert_eq!(metric(s.as_ref(), "commits").await?, 1);
    }
    Ok(())
}

#[tokio::test]
async fn explicit_abort_never_publishes_and_is_terminal() -> Result {
    for mode in MODES {
        let f = Fixture::new(mode).await?;
        let s = f.open("full").await?;
        let mut w = writer(s.as_ref(), "aborted").await?;
        w.write_chunk(b"discard").await?;
        w.abort().await?;
        assert_eq!(kind(w.abort().await), StorageErrorKind::SessionClosed);
        assert_eq!(kind(w.commit().await), StorageErrorKind::SessionClosed);
        assert_eq!(
            kind(stat(s.as_ref(), "aborted").await),
            StorageErrorKind::NotFound
        );
        assert_eq!(metric(s.as_ref(), "commits").await?, 0);
        eventually_metric(s.as_ref(), "writer", 0).await?;
    }
    Ok(())
}

#[tokio::test]
async fn wasm_rejected_info_and_zero_chunks_release_the_guest_resource() -> Result {
    let f = Fixture::new(Mode::Wasm).await?;
    let monitor = f.open("full").await?;
    for profile in ["zero-chunk", "malformed-info"] {
        let error = f.open(profile).await.err().ok_or("invalid info accepted")?;
        assert_eq!(
            category(error.as_ref()),
            Some(StorageErrorKind::InvalidData)
        );
        eventually_metric(monitor.as_ref(), "storage", 1).await?;
    }
    Ok(())
}

#[tokio::test]
async fn wasm_destructor_traps_poison_the_generation() -> Result {
    let f = Fixture::new(Mode::Wasm).await?;
    let Value::Storage { handle, .. } = f
        .raw(Op::Open {
            id: "instance".into(),
            config: Some(f.config("drop-trap").into_object()),
        })
        .await?
        .value
    else {
        panic!("missing storage")
    };
    let error = f
        .raw(Op::Release {
            kind: p::ResourceKind::Storage,
            handle,
        })
        .await
        .err()
        .ok_or("destructor trap swallowed")?;
    assert_eq!(
        category(error.as_ref()),
        Some(StorageErrorKind::StaleHandle)
    );
    assert_eq!(
        category(
            f.open("full")
                .await
                .err()
                .ok_or("poisoned generation reused")?
                .as_ref()
        ),
        Some(StorageErrorKind::StaleHandle)
    );
    Ok(())
}

#[tokio::test]
async fn wasm_commit_destructor_trap_preserves_mutation_uncertainty() -> Result {
    let f = Fixture::new(Mode::Wasm).await?;
    let s = f.open("full").await?;
    let mut w = writer(s.as_ref(), "drop-trap").await?;
    w.write_chunk(b"published-before-destructor").await?;
    assert_eq!(kind(w.commit().await), StorageErrorKind::OutcomeUnknown);
    assert_eq!(kind(w.commit().await), StorageErrorKind::SessionClosed);
    assert_eq!(
        kind(stat(s.as_ref(), "seed").await),
        StorageErrorKind::StaleHandle
    );
    Ok(())
}

#[tokio::test]
async fn expired_native_server_resources_are_reclaimed() -> Result {
    use vfs_plugin_grpc_server::dispatch::Dispatcher;
    let driver: Arc<dyn Driver> = Arc::new(conformance_native::ConformanceDriver::new());
    let monitor = driver
        .open(OpenContext::host_managed(
            InstanceId::parse("monitor")?,
            Default::default(),
        ))
        .await?;
    let dispatcher = Dispatcher::with_lease(driver, Duration::from_millis(20))?;
    let generation = dispatcher.generation();
    dispatcher
        .call(
            generation,
            Op::Initialize {
                settings: Default::default(),
            },
            vec![],
        )
        .await?;
    let Value::Storage { handle, .. } = dispatcher
        .call(
            generation,
            Op::Open {
                id: "instance".into(),
                config: Some(Default::default()),
            },
            vec![],
        )
        .await?
        .value
    else {
        panic!("missing storage")
    };
    dispatcher
        .call(
            generation,
            Op::Read {
                handle,
                path: "seed".into(),
                offset: 0,
                length: None,
            },
            vec![],
        )
        .await?;
    dispatcher
        .call(
            generation,
            Op::Write {
                handle,
                path: "abandoned".into(),
                replace: false,
                atomic: true,
            },
            vec![],
        )
        .await?;
    assert_eq!(metric(monitor.as_ref(), "storage").await?, 2);
    tokio::time::sleep(Duration::from_millis(40)).await;
    dispatcher.expire().await;
    assert_eq!(metric(monitor.as_ref(), "storage").await?, 1);
    assert_eq!(metric(monitor.as_ref(), "reader").await?, 0);
    assert_eq!(metric(monitor.as_ref(), "writer").await?, 0);
    assert_eq!(
        kind(stat(monitor.as_ref(), "abandoned").await),
        StorageErrorKind::NotFound
    );
    Ok(())
}

#[tokio::test]
async fn poisoned_native_server_generation_still_allows_resource_cleanup() -> Result {
    use vfs_plugin_grpc_server::dispatch::Dispatcher;
    let driver: Arc<dyn Driver> = Arc::new(conformance_native::ConformanceDriver::new());
    let monitor = driver
        .open(OpenContext::host_managed(
            InstanceId::parse("monitor")?,
            Default::default(),
        ))
        .await?;
    let dispatcher = Dispatcher::new(driver)?;
    let generation = dispatcher.generation();
    dispatcher
        .call(
            generation,
            Op::Initialize {
                settings: Default::default(),
            },
            vec![],
        )
        .await?;
    let Value::Storage { handle, .. } = dispatcher
        .call(
            generation,
            Op::Open {
                id: "instance".into(),
                config: Some(Default::default()),
            },
            vec![],
        )
        .await?
        .value
    else {
        panic!("missing storage")
    };
    let error = dispatcher
        .call(
            generation,
            Op::Storage {
                handle,
                request: p::StorageOp::Stat {
                    path: "panic".into(),
                },
            },
            vec![],
        )
        .await
        .err()
        .ok_or("panic swallowed")?;
    assert_eq!(category(&error), Some(StorageErrorKind::Unavailable));
    let error = dispatcher
        .call(
            generation,
            Op::Storage {
                handle,
                request: p::StorageOp::Stat {
                    path: "seed".into(),
                },
            },
            vec![],
        )
        .await
        .err()
        .ok_or("generation reused after panic")?;
    assert_eq!(category(&error), Some(StorageErrorKind::StaleHandle));
    dispatcher
        .call(
            generation,
            Op::Release {
                kind: p::ResourceKind::Storage,
                handle,
            },
            vec![],
        )
        .await?;
    assert_eq!(metric(monitor.as_ref(), "storage").await?, 1);
    Ok(())
}
