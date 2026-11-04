//! From this plugin's directory, build the guest with `cargo build --locked --target
//! wasm32-wasip2` before explicitly running these external-server tests.
mod common;
use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        dto::*,
        mount_runtime::MountRuntime,
        port::*,
        service::{EntryService, InstanceService, MountService},
    },
    domain::{
        DriverInstance, InstanceId,
        namespace::VirtualPath,
        storage::{
            DirModel, StorageDirQuery, StorageErrorKind, StorageMoveEntryRequest, StoragePath,
            StorageReadFileRequest, StorageStatEntryRequest, StorageWriteFileMode,
            StorageWriteFileRequest,
        },
    },
    infrastructure::{
        instance_record_repository::SqliteInstanceRecordRepository,
        mount_repository::SqliteMountRepository,
    },
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use vfs_plugin_host::PluginDriver;
use vfs_plugin_wasm::WasmTransport;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn hmac(key: &[u8], value: &[u8]) -> Result<Vec<u8>> {
    let mut h = Hmac::<Sha256>::new_from_slice(key)?;
    h.update(value);
    Ok(h.finalize().into_bytes().to_vec())
}
async fn fixture_request(
    method: &str,
    endpoint: &str,
    bucket: &str,
    key: Option<&str>,
    access: &str,
    secret: &str,
) -> Result<reqwest::StatusCode> {
    let now = time::OffsetDateTime::from_unix_timestamp(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64,
    )?;
    let timestamp = now.format(&time::format_description::parse_borrowed::<2>(
        "[year][month][day]T[hour][minute][second]Z",
    )?)?;
    let date = &timestamp[..8];
    let url = reqwest::Url::parse(endpoint)?;
    let host = &url[url::Position::BeforeHost..url::Position::AfterPort];
    let hash = hex::encode(Sha256::digest([]));
    let mut uri = format!("/{bucket}");
    if let Some(key) = key {
        uri.push('/');
        for b in key.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
                uri.push(b as char);
            } else {
                uri.push_str(&format!("%{b:02X}"));
            }
        }
    }
    let canonical = format!(
        "{method}\n{uri}\n\nhost:{host}\nx-amz-content-sha256:{hash}\nx-amz-date:{timestamp}\n\nhost;x-amz-content-sha256;x-amz-date\n{hash}"
    );
    let scope = format!("{date}/us-east-1/s3/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let k = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes())?;
    let k = hmac(&k, b"us-east-1")?;
    let k = hmac(&k, b"s3")?;
    let k = hmac(&k, b"aws4_request")?;
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={access}/{scope}, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature={}",
        hex::encode(hmac(&k, to_sign.as_bytes())?)
    );
    let response = reqwest::Client::new()
        .request(
            reqwest::Method::from_bytes(method.as_bytes())?,
            format!("{endpoint}{uri}"),
        )
        .header("x-amz-date", timestamp)
        .header("x-amz-content-sha256", hash)
        .header("authorization", auth)
        .send()
        .await?;
    Ok(response.status())
}
async fn component(origins: Vec<String>) -> Result<Arc<PluginDriver>> {
    let path = std::env::var_os("VFS_S3_COMPONENT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/wasm32-wasip2/debug/vfs_driver_s3.wasm")
        });
    let bytes = std::fs::read(path)?;
    Ok(Arc::new(
        PluginDriver::connect(Arc::new(
            WasmTransport::load(
                bytes,
                vfs_plugin_wasm::HostGrants {
                    http: vfs_plugin_wasm::HttpGrants {
                        origins,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| {
                let mut diagnostic = error.to_string();
                let mut source = std::error::Error::source(&error);
                while let Some(cause) = source {
                    diagnostic.push_str(&format!(": {cause}"));
                    source = cause.source();
                }
                diagnostic
            })?,
        ))
        .await?,
    ))
}

#[tokio::test]
#[ignore = "requires the built S3 WASM component; no external service"]
async fn protobuf_bootstrap_uses_the_shared_description_and_config_messages() -> Result {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut chunk = [0; 4096];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::other("request closed"));
            }
            request.extend_from_slice(&chunk[..read]);
            assert!(request.len() < 16384);
        }
        let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
        assert!(request.starts_with("head /fixture"));
        assert!(request.contains("authorization: aws4-hmac-sha256 "));
        assert!(request.contains("x-amz-date:"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await?;
        stream.shutdown().await?;
        std::io::Result::Ok(())
    });
    let driver = component(vec![endpoint.clone()]).await?;
    assert_eq!(
        driver.negotiated_protocol().major,
        vfs_plugin_protocol::PROTOCOL_MAJOR
    );
    driver.initialize(Default::default()).await?;
    let provisioner = driver.provisioner();
    let config = InstanceConfig::from([
        ("endpoint".into(), endpoint.into()),
        ("bucket".into(), "fixture".into()),
        ("region".into(), "us-east-1".into()),
        ("access_key".into(), "test-access".into()),
        ("secret_key".into(), "test-secret".into()),
    ]);
    let prepared = provisioner.prepare(config.clone()).await?;
    assert_eq!(prepared.config(), &config);
    assert_eq!(
        prepared.details().get("bucket").and_then(|v| v.as_str()),
        Some("fixture")
    );
    assert_eq!(provisioner.migrate_config(1, config.clone()).await?, config);
    provisioner.test_connection(&config).await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), server).await???;
    Ok(())
}
async fn put(s: &dyn Storage, name: &str, bytes: &[u8]) -> Result {
    let mut w = s
        .write_file(StorageWriteFileRequest::new(StoragePath::parse(name)?))
        .await?;
    for part in bytes.chunks(64 * 1024) {
        w.write_chunk(part).await?;
    }
    w.commit().await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires the cross-compiled S3 component"]
async fn missing_required_http_grant_rejects_component_loading() -> Result {
    let error = component(vec![])
        .await
        .err()
        .ok_or("missing grant accepted")?;
    let mut messages = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        messages.push_str(&cause.to_string());
        source = cause.source();
    }
    assert!(
        messages.contains("ungranted host capabilities: http"),
        "{messages}"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires a real S3 endpoint and cross-compiled component"]
async fn s3_component_runs_through_host_records_mounts_and_streams() -> Result {
    let endpoint = std::env::var("VFS_TEST_S3_ENDPOINT")?;
    let access = std::env::var("VFS_TEST_S3_ACCESS_KEY")?;
    let secret = std::env::var("VFS_TEST_S3_SECRET_KEY")?;
    let bucket = format!("vfs-{}", InstanceId::generate().as_str().replace('-', ""));
    if !fixture_request("PUT", &endpoint, &bucket, None, &access, &secret)
        .await?
        .is_success()
    {
        return Err("bucket fixture failed".into());
    }
    let outcome: Result = async {
        let driver = component(vec![endpoint.clone()]).await?;
        let mut catalog = DriverCatalog::new();
        catalog.register(driver.clone(), Default::default()).await?;
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let records = Arc::new(SqliteInstanceRecordRepository::from_pool(pool).await?);
        let provisioner = driver.provisioner();
        let config = InstanceConfig::from([
            ("endpoint".into(), endpoint.clone().into()),
            ("bucket".into(), bucket.clone().into()),
            ("region".into(), "us-east-1".into()),
            ("access_key".into(), access.clone().into()),
            ("secret_key".into(), secret.clone().into()),
        ]);
        provisioner.prepare(config.clone()).await?;
        provisioner.test_connection(&config).await?;
        let runtime = Arc::new(MountRuntime::new(
            Arc::new(SqliteMountRepository::in_memory().await?),
            Arc::new(catalog),
            records.clone(),
            Some(Arc::new(common::Cipher::new())),
        ));
        let instances = InstanceService::new(runtime.clone());
        let instance = instances
            .create_instance(driver.info().key(), config.clone())
            .await?;
        let reference = DriverInstance::new(driver.info().key().clone(), instance.id.clone());
        assert!(
            !records
                .get(reference.clone())
                .await?
                .ok_or("host record")?
                .config_json()
                .contains(&secret)
        );
        let mounts = MountService::new(runtime.clone());
        mounts
            .mount(MountRequest {
                virtual_path: VirtualPath::root(),
                driver_instance: reference,
            })
            .await?;
        let entries = EntryService::new(runtime);
        let storage = driver
            .open(OpenContext::new(
                InstanceId::parse("s3-fixture")?,
                config.clone(),
            ))
            .await?;
        assert_eq!(storage.capabilities().dir_model(), DirModel::Prefix);
        assert!(!storage.capabilities().rename().replace);
        let mut pending = storage
            .write_file(StorageWriteFileRequest::new(StoragePath::parse("staged")?))
            .await?;
        pending.write_chunk(b"not yet visible").await?;
        assert_eq!(
            storage
                .stat_entry(StorageStatEntryRequest {
                    path: StoragePath::parse("staged")?
                })
                .await
                .err()
                .ok_or("unpublished")?
                .kind(),
            StorageErrorKind::NotFound
        );
        pending.abort().await?;
        put(storage.as_ref(), "目录/文件", b"portable object bytes").await?;
        put(storage.as_ref(), "empty", b"").await?;
        let large = vec![0x5a; 9 * 1024 * 1024 + 17];
        put(storage.as_ref(), "multipart", &large).await?;
        let mut reader = storage
            .read_file(StorageReadFileRequest {
                path: StoragePath::parse("multipart")?,
                offset: 5 * 1024 * 1024 - 3,
                length: Some(70_000),
            })
            .await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = reader.next_chunk().await? {
            bytes.extend(chunk);
        }
        assert_eq!(
            bytes,
            &large[5 * 1024 * 1024 - 3..5 * 1024 * 1024 - 3 + 70_000]
        );
        assert!(reader.next_chunk().await?.is_none());
        let mut page_query = StorageDirQuery::new(StoragePath::root());
        page_query.limit = std::num::NonZeroUsize::MIN;
        let mut names = Vec::new();
        loop {
            let page = storage
                .list_entries(StorageDirQuery {
                    path: page_query.path.clone(),
                    after: page_query.after.clone(),
                    limit: page_query.limit,
                })
                .await?;
            names.extend(page.entries.into_iter().map(|e| e.path.as_str().to_owned()));
            if page.next_after.is_none() {
                break;
            }
            page_query.after = page.next_after;
        }
        assert_eq!(names, vec!["empty", "multipart", "目录"]);
        let mut writer = entries
            .write_file(WriteFileRequest {
                path: VirtualPath::parse("/through-vfs")?,
                mode: StorageWriteFileMode::CreateNew,
                require_atomic_publish: true,
            })
            .await?;
        writer.write_chunk(b"host-managed WASM mount").await?;
        writer.commit().await?;
        let mut reader = entries
            .read_file(ReadFileRequest {
                path: VirtualPath::parse("/through-vfs")?,
                offset: 0,
                length: None,
            })
            .await?;
        assert_eq!(
            reader.next_chunk().await?.ok_or("mounted bytes")?,
            b"host-managed WASM mount"
        );
        assert!(reader.next_chunk().await?.is_none());
        let mut conflict = storage
            .write_file(StorageWriteFileRequest::new(StoragePath::parse("race")?))
            .await?;
        let mut winner = storage
            .write_file(StorageWriteFileRequest::new(StoragePath::parse("race")?))
            .await?;
        winner.write_chunk(b"winner").await?;
        conflict.write_chunk(b"loser").await?;
        winner.commit().await?;
        assert_eq!(
            conflict.commit().await.err().ok_or("conflict")?.kind(),
            StorageErrorKind::AlreadyExists
        );
        let error = storage
            .move_entry(StorageMoveEntryRequest {
                source: StoragePath::parse("empty")?,
                destination: StoragePath::parse("other")?,
                replace: true,
            })
            .await
            .err()
            .ok_or("unsupported rename")?;
        assert_eq!(error.kind(), StorageErrorKind::UnsupportedOperation);
        let denied = component(vec![]).await?;
        let mut catalog = DriverCatalog::new();
        catalog.register(denied.clone(), Default::default()).await?;
        let error = denied
            .open(OpenContext::new(InstanceId::parse("denied")?, config))
            .await
            .err()
            .ok_or("network must be denied")?;
        assert_eq!(
            error.storage_error().ok_or("category")?.kind(),
            StorageErrorKind::PermissionDenied
        );
        drop(records);
        Ok(())
    }
    .await;
    // Clean only the randomly allocated fixture bucket, including failed runs.
    let cleanup: Result = async {
        for key in [
            "empty",
            "multipart",
            "目录/文件",
            "race",
            "through-vfs",
            "staged",
        ] {
            let status =
                fixture_request("DELETE", &endpoint, &bucket, Some(key), &access, &secret).await?;
            if !status.is_success() && status != reqwest::StatusCode::NOT_FOUND {
                return Err("object fixture cleanup failed".into());
            }
        }
        let status = fixture_request("DELETE", &endpoint, &bucket, None, &access, &secret).await?;
        if !status.is_success() && status != reqwest::StatusCode::NOT_FOUND {
            return Err("bucket fixture cleanup failed".into());
        }
        Ok(())
    }
    .await;
    outcome?;
    cleanup
}
