//! Build the guest with `cargo build --locked --release --target wasm32-wasip2`
//! before running these tests. The multipart test uses the default host fuel budget.
use ah_vfs_storage::{Failure, MutationOutcome, Rejection};
use ah_vfs_storage::{RenameMode, RenameRequest};
mod common;
use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::dto::*;
use ah_vfs::application::mount_runtime::MountRuntime;
use ah_vfs::application::port::*;
use ah_vfs::application::service::{EntryService, InstanceService, MountService};
use ah_vfs::domain::namespace::VirtualPath;
use ah_vfs::domain::{DriverInstance, InstanceId};
use ah_vfs::infrastructure::instance_record_repository::SqliteInstanceRecordRepository;
use ah_vfs::infrastructure::mount_repository::SqliteMountRepository;
use ah_vfs_storage::StoragePath;
use ah_vfs_storage::WriteMode;
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
                .join("target/wasm32-wasip2/release/vfs_driver_s3.wasm")
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
async fn protobuf_reads_and_conditional_commits_bind_a_single_s3_version() -> Result {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        for (expected, status, length, extra, body) in [
            ("head /fixture/ ", "200 OK", 0, "", vec![]),
            ("head /fixture/ ", "200 OK", 0, "", vec![]),
            (
                "head /fixture/file ",
                "200 OK",
                65540,
                "etag: \"original\"\r\n",
                vec![],
            ),
            (
                "get /fixture/?",
                "200 OK",
                19,
                "",
                b"<ListBucketResult/>".to_vec(),
            ),
            (
                "get /fixture/file ",
                "206 Partial Content",
                65536,
                "etag: \"original\"\r\ncontent-range: bytes 1-65536/65540\r\n",
                vec![7; 65536],
            ),
            (
                "get /fixture/file ",
                "412 Precondition Failed",
                0,
                "",
                vec![],
            ),
            ("head /fixture/file ", "200 OK", 65540, "etag: \"original\"\r\n", vec![]),
            ("get /fixture/?", "200 OK", 19, "", b"<ListBucketResult/>".to_vec()),
            (
                "post /fixture/file?uploads=", "200 OK",
                b"<InitiateMultipartUploadResult><UploadId>mock</UploadId></InitiateMultipartUploadResult>".len(),
                "",
                b"<InitiateMultipartUploadResult><UploadId>mock</UploadId></InitiateMultipartUploadResult>".to_vec(),
            ),
            ("put /fixture/file?partnumber=1&uploadid=mock", "200 OK", 0, "etag: \"part\"\r\n", vec![]),
            ("post /fixture/file?uploadid=mock", "412 Precondition Failed", 0, "", vec![]),
            ("delete /fixture/file?uploadid=mock", "204 No Content", 0, "", vec![]),
            ("head /fixture/file ", "200 OK", 65540, "etag: \"original\"\r\n", vec![]),
            ("get /fixture/?", "200 OK", 19, "", b"<ListBucketResult/>".to_vec()),
            ("delete /fixture/file ", "412 Precondition Failed", 0, "", vec![]),
            ("head /fixture/file ", "200 OK", 65540, "etag: \"original\"\r\n", vec![]),
            ("get /fixture/?", "200 OK", 19, "", b"<ListBucketResult/>".to_vec()),
            ("delete /fixture/file ", "204 No Content", 0, "", vec![]),
            ("head /fixture/file ", "200 OK", 65540, "etag: \"original\"\r\n", vec![]),
            ("get /fixture/?", "200 OK", 19, "", b"<ListBucketResult/>".to_vec()),
            ("delete /fixture/file ", "", 0, "", vec![]),
        ] {
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
            let header_end = request.windows(4).position(|bytes| bytes == b"\r\n\r\n").ok_or(std::io::Error::other("headers"))? + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
            let content_length = headers.lines().find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or_default();
            if request.len() < header_end + content_length {
                let mut rest = vec![0; header_end + content_length - request.len()];
                stream.read_exact(&mut rest).await?;
            }
            let request = headers;
            assert!(request.starts_with(expected), "{request}");
            assert!(request.contains("authorization: aws4-hmac-sha256 "));
            assert!(request.contains("x-amz-date:"));
            if expected == "get /fixture/file " {
                assert!(request.contains("if-match: \"original\"\r\n"));
                assert!(request.contains("signedheaders=host;if-match;range;"));
                let range = if length == 0 {
                    "65537-65539"
                } else {
                    "1-65536"
                };
                assert!(request.contains(&format!("range: bytes={range}\r\n")));
            }
            if expected == "post /fixture/file?uploadid=mock" {
                assert!(request.contains("if-match: \"original\"\r\n"));
                assert!(request.contains("signedheaders=content-type;host;if-match;"));
            }
            if expected == "delete /fixture/file " {
                assert!(request.contains("if-match: \"original\"\r\n"));
                assert!(request.contains("signedheaders=host;if-match;"));
                if status.is_empty() {
                    // Close after receiving the complete conditional mutation.
                    stream.shutdown().await?;
                    continue;
                }
            }
            stream.write_all(format!(
                "HTTP/1.1 {status}\r\nContent-Length: {length}\r\n{extra}Connection: close\r\n\r\n"
            ).as_bytes()).await?;
            stream.write_all(&body).await?;
            stream.shutdown().await?;
        }
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
    let storage = driver
        .open(OpenContext::new(InstanceId::parse("fixture")?, config))
        .await?;
    assert!(!driver.info().capabilities().list());
    let directories = driver.info().capabilities().directories();
    assert_eq!(
        directories,
        ah_vfs_storage::DirectorySupport::prefix(false, false)
    );
    let create = ah_vfs_storage::CreateDirectoryRequest::new(
        ah_vfs_storage::PathExpr::parse("/absent/child")?,
        ah_vfs_storage::CreateDirectoryMode::Recursive,
    );
    assert!(
        matches!(storage.create_dir(create, Default::default()).await,
            Err(error) if error.operation() == ah_vfs_storage::Operation::CreateDirectory
                && error.failure() == Failure::Rejected(Rejection::Unsupported)
                && error.outcome() == MutationOutcome::Unchanged
        )
    );
    let request = ah_vfs_storage::ListRequest::new(
        ah_vfs_storage::ListTarget::Path(StoragePath::root().to_rooted_expr()),
        Default::default(),
    );
    assert!(
        matches!(storage.list_entries(&request, Default::default()).await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::Unsupported))
    );
    let mut reader = storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                StoragePath::parse("file")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::new(1, None)?,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(reader.next_chunk().await?, Some(vec![7; 65536]));
    assert!(matches!(reader.next_chunk().await, Err(error)
        if error.failure() == Failure::Rejected(Rejection::Conflict)));
    assert!(matches!(reader.next_chunk().await, Err(error)
        if error.failure() == Failure::Backend(ah_vfs_storage::BackendFailure::Closed)));
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                StoragePath::parse("file")?.to_rooted_expr(),
                WriteMode::Replace,
                ah_vfs_storage::Publication::Atomic,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"private part").await?;
    let conflict = writer
        .commit()
        .await
        .err()
        .ok_or("expected conditional conflict")?;
    assert_eq!(conflict.failure(), Failure::Rejected(Rejection::Conflict));
    assert_eq!(
        conflict.outcome(),
        ah_vfs_storage::MutationOutcome::Unchanged
    );
    assert!(matches!(writer.commit().await, Err(error)
        if error.failure() == Failure::Backend(ah_vfs_storage::BackendFailure::Closed)));
    let remove = ah_vfs_storage::RemoveRequest::new(
        StoragePath::parse("file")?.to_rooted_expr(),
        ah_vfs_storage::RemoveKind::NonDirectory,
    )?;
    let conflict = storage
        .remove_entry(remove.clone(), Default::default())
        .await
        .err()
        .ok_or("expected conditional delete conflict")?;
    assert_eq!(conflict.operation(), ah_vfs_storage::Operation::Remove);
    assert_eq!(conflict.failure(), Failure::Rejected(Rejection::Conflict));
    assert_eq!(conflict.outcome(), MutationOutcome::Unchanged);
    storage
        .remove_entry(remove.clone(), Default::default())
        .await?;
    let lost = storage
        .remove_entry(remove, Default::default())
        .await
        .err()
        .ok_or("expected uncertain delete")?;
    assert_eq!(lost.operation(), ah_vfs_storage::Operation::Remove);
    assert_eq!(lost.outcome(), MutationOutcome::Unknown);
    tokio::time::timeout(std::time::Duration::from_secs(5), server).await???;
    Ok(())
}
async fn put(s: &dyn Storage, name: &str, bytes: &[u8]) -> Result {
    let mut w = s
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                (StoragePath::parse(name)?).to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
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
        assert!(matches!(
            driver.info().capabilities().directories(),
            ah_vfs_storage::DirectorySupport::Prefix { .. }
        ));
        assert!(
            !driver
                .info()
                .capabilities()
                .rename()
                .supports(ah_vfs_storage::RenameMode::Replace)
        );
        let mut pending = storage
            .write_file(
                ah_vfs_storage::WriteRequest::new(
                    StoragePath::parse("staged")?.to_rooted_expr(),
                    ah_vfs_storage::WriteMode::CreateNew,
                    ah_vfs_storage::Publication::Staged,
                ),
                Default::default(),
            )
            .await?;
        pending.write_chunk(b"not yet visible").await?;
        assert_eq!(
            storage
                .stat_entry(
                    ah_vfs_storage::StatRequest::new(
                        StoragePath::parse("staged")?.to_rooted_expr(),
                        ah_vfs_storage::SymlinkAction::Preserve
                    ),
                    Default::default()
                )
                .await
                .err()
                .ok_or("unpublished")?
                .failure(),
            Failure::Rejected(Rejection::NotFound)
        );
        pending.abort().await?;
        assert!(
            fixture_request(
                "PUT",
                &endpoint,
                &bucket,
                Some("目录/文件"),
                &access,
                &secret
            )
            .await?
            .is_success()
        );
        let nested = ah_vfs_storage::WriteRequest::new(
            StoragePath::parse("目录/other")?.to_rooted_expr(),
            WriteMode::CreateNew,
            ah_vfs_storage::Publication::Atomic,
        );
        assert!(
            matches!(storage.write_file(nested, Default::default()).await, Err(error)
            if error.failure() == Failure::Rejected(Rejection::Unsupported))
        );
        put(storage.as_ref(), "empty", b"").await?;
        let large = vec![0x5a; 9 * 1024 * 1024 + 17];
        put(storage.as_ref(), "multipart", &large).await?;
        let mut reader = storage
            .read_file(
                ah_vfs_storage::ReadRequest::new(
                    StoragePath::parse("multipart")?.to_rooted_expr(),
                    ah_vfs_storage::ReadRange::new(5 * 1024 * 1024 - 3, Some(70_000))?,
                ),
                Default::default(),
            )
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
        let listing = ah_vfs_storage::ListRequest::new(
            ah_vfs_storage::ListTarget::Path(StoragePath::root().to_rooted_expr()),
            Default::default(),
        );
        assert!(
            matches!(storage.list_entries(&listing, Default::default()).await,
            Err(error) if error.failure() == Failure::Rejected(Rejection::Unsupported))
        );
        let mut writer = entries
            .write_file(WriteFileRequest {
                path: VirtualPath::parse("/through-vfs")?,
                mode: WriteMode::CreateNew,
                publication: ah_vfs_storage::Publication::Atomic,
            })
            .await?;
        writer.write_chunk(b"host-managed WASM mount").await?;
        writer.commit().await?;
        let mut reader = entries
            .read_file(ReadFileRequest {
                path: VirtualPath::parse("/through-vfs")?,
                range: ah_vfs_storage::ReadRange::new(0, None)?,
            })
            .await?;
        assert_eq!(
            reader.next_chunk().await?.ok_or("mounted bytes")?,
            b"host-managed WASM mount"
        );
        assert!(reader.next_chunk().await?.is_none());
        let mut conflict = storage
            .write_file(
                ah_vfs_storage::WriteRequest::new(
                    StoragePath::parse("race")?.to_rooted_expr(),
                    ah_vfs_storage::WriteMode::CreateNew,
                    ah_vfs_storage::Publication::Staged,
                ),
                Default::default(),
            )
            .await?;
        let mut winner = storage
            .write_file(
                ah_vfs_storage::WriteRequest::new(
                    StoragePath::parse("race")?.to_rooted_expr(),
                    ah_vfs_storage::WriteMode::CreateNew,
                    ah_vfs_storage::Publication::Staged,
                ),
                Default::default(),
            )
            .await?;
        winner.write_chunk(b"winner").await?;
        conflict.write_chunk(b"loser").await?;
        winner.commit().await?;
        assert_eq!(
            conflict.commit().await.err().ok_or("conflict")?.failure(),
            Failure::Rejected(Rejection::Conflict)
        );
        let error = storage
            .move_entry(
                RenameRequest::new(
                    (StoragePath::parse("empty")?).to_rooted_expr(),
                    (StoragePath::parse("other")?).to_rooted_expr(),
                    RenameMode::Replace,
                )?,
                Default::default(),
            )
            .await
            .err()
            .ok_or("unsupported rename")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::Unsupported));
        // Grant HTTP so the component can load, but exclude the fixture origin.
        let denied = component(vec!["http://127.0.0.1:1".into()]).await?;
        let mut catalog = DriverCatalog::new();
        catalog.register(denied.clone(), Default::default()).await?;
        let error = denied
            .open(OpenContext::new(InstanceId::parse("denied")?, config))
            .await
            .err()
            .ok_or("network must be denied")?;
        assert_eq!(
            error.lifecycle_error().ok_or("category")?.failure(),
            ah_vfs::application::driver_error::LifecycleFailure::AccessDenied
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
