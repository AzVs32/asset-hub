use ah_vfs::application::dto::*;
use ah_vfs::application::port::*;
use ah_vfs::domain::InstanceId;
use ah_vfs::domain::storage::StorageRemoveEntryRequest;
use ah_vfs_storage::{
    CreateDirectoryMode, CreateDirectoryRequest, EntryType, MutationOutcome, PathExpr, Publication,
    StoragePath, WriteMode, WriteRequest,
};
use ah_vfs_storage::{Failure, Rejection};
use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vfs_plugin_host::PluginDriver;
use vfs_plugin_wasm::WasmTransport;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Entries = Arc<Mutex<BTreeMap<String, Option<Vec<u8>>>>>;
struct Fixture {
    origin: String,
    task: tokio::task::JoinHandle<()>,
    deletes: Arc<AtomicUsize>,
    entries: Entries,
    root_version: Arc<AtomicUsize>,
    lost_puts: Arc<AtomicUsize>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new() -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let entries: Entries = Arc::new(Mutex::new(BTreeMap::from([(String::new(), None)])));
        let deletes = Arc::new(AtomicUsize::new(0));
        let root_version = Arc::new(AtomicUsize::new(1));
        let lost_puts = Arc::new(AtomicUsize::new(0));
        let (served_version, observed_lost) = (root_version.clone(), lost_puts.clone());
        let observed = deletes.clone();
        let served = entries.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let entries = served.clone();
                let deletes = observed.clone();
                let root_version = served_version.clone();
                let lost_puts = observed_lost.clone();
                tokio::spawn(async move {
                    let _ = handle(stream, entries, deletes, root_version, lost_puts).await;
                });
            }
        });
        Ok(Self {
            origin,
            task,
            deletes,
            entries,
            root_version,
            lost_puts,
        })
    }

    fn replace(&self, path: &str, data: Vec<u8>) -> Result {
        self.entries
            .lock()
            .map_err(|_| "entries lock")?
            .insert(path.into(), Some(data));
        Ok(())
    }
}
fn etag(path: &str, data: &[u8]) -> Option<String> {
    if path == "unversioned" {
        return None;
    }
    let mut hash = DefaultHasher::new();
    data.hash(&mut hash);
    Some(format!("\"{:x}\"", hash.finish()))
}
fn collection_etag(path: &str, root_version: usize) -> Option<String> {
    match path {
        "" => Some(format!("\"root-{root_version}\"")),
        "no-validator" => None,
        _ => Some(format!("\"collection-{path}\"")),
    }
}

fn metadata(path: &str, data: &Option<Vec<u8>>, root_version: usize) -> String {
    let kind = if data.is_none() {
        "<d:collection/>"
    } else {
        ""
    };
    let suffix = if data.is_none() && !path.is_empty() {
        "/"
    } else {
        ""
    };
    let tag = match data {
        None => collection_etag(path, root_version),
        Some(data) => etag(path, data),
    };
    let resource = if path.is_empty() {
        format!("<d:resource-id><d:href>urn:example:root:{root_version}</d:href></d:resource-id>")
    } else {
        String::new()
    };
    let version = tag
        .map(|tag| format!("<d:getetag>{tag}</d:getetag>"))
        .unwrap_or_default();
    format!(
        "<d:response><d:href>/dav/{path}{suffix}</d:href><d:propstat><d:prop><d:resourcetype>{kind}</d:resourcetype><d:getcontentlength>{}</d:getcontentlength>{version}{resource}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
        data.as_ref().map_or(0, Vec::len)
    )
}
async fn handle(
    mut stream: tokio::net::TcpStream,
    entries: Entries,
    deletes: Arc<AtomicUsize>,
    root_version: Arc<AtomicUsize>,
    lost_puts: Arc<AtomicUsize>,
) -> Result {
    let mut bytes = vec![];
    let mut buffer = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            return Err("incomplete HTTP request".into());
        }
        bytes.extend_from_slice(&buffer[..n]);
        if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            break end + 4;
        }
        if bytes.len() > 65536 {
            return Err("header limit".into());
        }
    };
    let header = String::from_utf8(bytes[..header_end].to_vec())?;
    let mut lines = header.lines();
    let mut line = lines.next().ok_or("request line")?.split_whitespace();
    let method = line.next().ok_or("method")?;
    let uri = line.next().ok_or("uri")?;
    let path = uri
        .strip_prefix("/dav/")
        .ok_or("confined URI")?
        .trim_end_matches('/');
    let fields: BTreeMap<_, _> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length = fields
        .get("content-length")
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(0usize);
    if length > vfs_plugin_protocol::MAX_DATA_BYTES {
        return Err("body limit".into());
    }
    while bytes.len() - header_end < length {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            return Err("incomplete body".into());
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    let (status, body, extra) = {
        let version = root_version.load(Ordering::SeqCst);
        let mut entries = entries.lock().map_err(|_| "entries lock")?;
        match method {
            "PROPFIND" => match entries.get(path) {
                None => ("404 Not Found", vec![], String::new()),
                Some(value) => {
                    let mut xml = metadata(path, value, version);
                    if fields.get("depth").is_some_and(|v| v == "1") {
                        let prefix = if path.is_empty() {
                            String::new()
                        } else {
                            format!("{path}/")
                        };
                        for (child, value) in entries.iter().filter(|(child, _)| {
                            child.as_str() != path
                                && child.starts_with(&prefix)
                                && !child[prefix.len()..].contains('/')
                        }) {
                            xml.push_str(&metadata(child, value, version));
                        }
                    }
                    (
                        "207 Multi-Status",
                        format!("<d:multistatus xmlns:d=\"DAV:\">{xml}</d:multistatus>")
                            .into_bytes(),
                        "content-type: application/xml\r\n".into(),
                    )
                }
            },
            "PUT" => {
                if path == "lost-response" {
                    lost_puts.fetch_add(1, Ordering::SeqCst);
                }
                let parent = format!(
                    "<http://{}/dav/> ([\"root-{version}\"])",
                    fields.get("host").ok_or("host")?
                );
                let target = match entries.get(path) {
                    None => fields
                        .get("if-none-match")
                        .is_some_and(|value| value == "*"),
                    Some(Some(data)) => etag(path, data)
                        .as_ref()
                        .is_some_and(|tag| fields.get("if-match") == Some(tag)),
                    Some(None) => false,
                };
                if fields.get("if") != Some(&parent) || !target {
                    ("412 Precondition Failed", vec![], String::new())
                } else {
                    entries.insert(
                        path.into(),
                        Some(bytes[header_end..header_end + length].to_vec()),
                    );
                    ("201 Created", vec![], String::new())
                }
            }
            "MKCOL" => {
                let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
                let version = if path == "raced" {
                    root_version.fetch_add(1, Ordering::SeqCst) + 1
                } else {
                    version
                };
                let host = fields.get("host").ok_or("host")?;
                let suffix = if parent.is_empty() { "" } else { "/" };
                let condition = collection_etag(parent, version)
                    .map(|tag| format!("<http://{host}/dav/{parent}{suffix}> ([{tag}])"));
                let status = if fields.get("if") != condition.as_ref() {
                    "412 Precondition Failed"
                } else if name == "denied" {
                    "403 Forbidden"
                } else if entries.contains_key(path) {
                    "405 Method Not Allowed"
                } else if entries.get(parent) != Some(&None) {
                    "409 Conflict"
                } else {
                    entries.insert(path.into(), None);
                    "201 Created"
                };
                (status, vec![], String::new())
            }
            "GET" => {
                let data = entries.get(path).and_then(|v| v.as_ref()).ok_or("file")?;
                let version = etag(path, data);
                if fields.get("if-match") != version.as_ref() {
                    ("412 Precondition Failed", vec![], String::new())
                } else {
                    let range = fields
                        .get("range")
                        .and_then(|v| v.strip_prefix("bytes="))
                        .ok_or("range")?;
                    let (start, end) = range.split_once('-').ok_or("range bounds")?;
                    let start: usize = start.parse()?;
                    let end: usize = end.parse()?;
                    let version_header = version
                        .map(|tag| format!("etag: {tag}\r\n"))
                        .unwrap_or_default();
                    (
                        "206 Partial Content",
                        data[start..=end].to_vec(),
                        format!(
                            "content-range: bytes {start}-{end}/{}\r\n{version_header}",
                            data.len()
                        ),
                    )
                }
            }
            "DELETE" => {
                deletes.fetch_add(1, Ordering::SeqCst);
                ("500 Internal Server Error", vec![], String::new())
            }
            _ => ("405 Method Not Allowed", vec![], String::new()),
        }
    };
    if path == "lost-response" && status == "201 Created" {
        // Publication completed under the server lock. Drop the connection before replying.
        return Ok(());
    }
    stream
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await?;
    stream.write_all(&body).await?;
    stream.shutdown().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a built WebDAV WASM component; fixture uses an isolated loopback server"]
async fn protobuf_component_opens_writes_commits_reads_and_lists() -> Result {
    let path = std::env::var_os("VFS_WEBDAV_COMPONENT").ok_or("VFS_WEBDAV_COMPONENT")?;
    let fixture = Fixture::new().await?;
    let driver = PluginDriver::connect(Arc::new(
        WasmTransport::load(
            std::fs::read(path)?,
            vfs_plugin_wasm::HostGrants {
                http: vfs_plugin_wasm::HttpGrants {
                    origins: vec![fixture.origin.clone()],
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await?,
    ))
    .await?;
    assert_eq!(
        driver.negotiated_protocol().major,
        vfs_plugin_protocol::PROTOCOL_MAJOR
    );
    driver.initialize(Default::default()).await?;
    let provisioner = driver.provisioner();
    let config =
        InstanceConfig::from([("endpoint".into(), format!("{}/dav/", fixture.origin).into())]);
    let prepared = provisioner.prepare(config.clone()).await?;
    assert_eq!(
        prepared
            .config()
            .get("max_write_bytes")
            .and_then(|v| v.as_u64()),
        Some(vfs_plugin_protocol::MAX_DATA_BYTES as u64)
    );
    provisioner.test_connection(&config).await?;
    let storage = driver
        .open(OpenContext::new(InstanceId::parse("fixture")?, config))
        .await?;
    assert_eq!(storage.chunk_size().get(), 65536);
    assert!(!driver.info().capabilities().remove());
    let file = StoragePath::parse("file")?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                file.clone().to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(&[0, 255, 1, 2]).await?;
    let entry = writer.commit().await?;
    assert_eq!(entry.entry_type(), ah_vfs_storage::EntryType::file(Some(4)));
    assert_eq!(entry.path(), &file);
    let mut reader = storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                file.clone().to_rooted_expr(),
                ah_vfs_storage::ReadRange::new(1, Some(2))?,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(reader.next_chunk().await?, Some(vec![255, 1]));
    assert_eq!(reader.next_chunk().await?, None);
    assert_eq!(reader.next_chunk().await?, None);
    let directory = storage
        .create_dir(
            ah_vfs_storage::CreateDirectoryRequest::new(
                StoragePath::parse("empty")?.to_rooted_expr(),
                ah_vfs_storage::CreateDirectoryMode::Single,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(directory.entry_type(), EntryType::Directory);
    let request = ah_vfs_storage::ListRequest::new(
        ah_vfs_storage::ListTarget::Path(StoragePath::root().to_rooted_expr()),
        ah_vfs_storage::PageSize::new(1)?,
    );
    let page = storage.list_entries(&request, Default::default()).await?;
    assert_eq!(page.entries()[0].path().as_str(), "empty");
    let continuation = ah_vfs_storage::ListRequest::new(
        ah_vfs_storage::ListTarget::Continue(page.continuation().ok_or("next page")?),
        Default::default(),
    );
    fixture.replace("entry", b"added during listing".to_vec())?;
    let page = storage
        .list_entries(&continuation, Default::default())
        .await?;
    assert_eq!(
        page.entries()
            .iter()
            .map(|entry| entry.path().as_str())
            .collect::<Vec<_>>(),
        ["entry", "file"]
    );
    assert!(page.continuation().is_none());
    for path in ["versioned", "unversioned"] {
        let chunk = storage.chunk_size().get();
        fixture.replace(path, vec![1; chunk + 4])?;
        let mut stream = storage
            .read_file(
                ah_vfs_storage::ReadRequest::new(
                    StoragePath::parse(path)?.to_rooted_expr(),
                    ah_vfs_storage::ReadRange::new(1, None)?,
                ),
                Default::default(),
            )
            .await?;
        assert_eq!(stream.next_chunk().await?, Some(vec![1; chunk]));
        fixture.replace(path, vec![2; chunk + 4])?;
        if path == "unversioned" {
            assert_eq!(stream.next_chunk().await?, Some(vec![1; 3]));
            assert_eq!(stream.next_chunk().await?, None);
        } else {
            assert!(matches!(stream.next_chunk().await, Err(error)
                if error.failure() == Failure::Rejected(Rejection::Conflict)));
            assert!(matches!(stream.next_chunk().await, Err(error)
                if error.failure() == Failure::Backend(ah_vfs_storage::BackendFailure::Closed)));
        }
    }
    assert_eq!(
        storage
            .remove_entry(StorageRemoveEntryRequest { path: file })
            .await
            .err()
            .ok_or("unsupported removal")?
            .failure(),
        Failure::Rejected(Rejection::Unsupported)
    );
    let write = |path: &str, mode, publication| -> Result<WriteRequest> {
        Ok(WriteRequest::new(
            StoragePath::parse(path)?.to_rooted_expr(),
            mode,
            publication,
        ))
    };
    assert!(matches!(
        storage.write_file(write("atomic", WriteMode::CreateNew, Publication::Atomic)?, Default::default()).await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::Unsupported)
    ));

    let mut replace = storage
        .write_file(
            write("file", WriteMode::Replace, Publication::Staged)?,
            Default::default(),
        )
        .await?;
    replace.write_chunk(b"staged").await?;
    fixture.replace("file", b"winner".to_vec())?;
    let conflict = replace.commit().await.err().ok_or("replace must fail")?;
    assert_eq!(conflict.failure(), Failure::Rejected(Rejection::Conflict));
    assert_eq!(conflict.outcome(), MutationOutcome::Unchanged);

    let mut moved = storage
        .write_file(
            write("parent-replaced", WriteMode::CreateNew, Publication::Staged)?,
            Default::default(),
        )
        .await?;
    moved.write_chunk(b"staged").await?;
    fixture.root_version.fetch_add(1, Ordering::SeqCst);
    assert!(
        matches!(storage.list_entries(&continuation, Default::default()).await,
        Err(error) if error.failure() == Failure::Backend(ah_vfs_storage::BackendFailure::Stale))
    );
    assert!(!storage.is_stale());
    let conflict = moved
        .commit()
        .await
        .err()
        .ok_or("parent change must fail")?;
    assert_eq!(conflict.failure(), Failure::Rejected(Rejection::Conflict));
    assert_eq!(conflict.outcome(), MutationOutcome::Unchanged);
    assert!(
        !fixture
            .entries
            .lock()
            .map_err(|_| "entries lock")?
            .contains_key("parent-replaced")
    );

    let create = |operand: &str, mode| -> Result<_> {
        Ok(storage.create_dir(
            CreateDirectoryRequest::new(PathExpr::parse(operand)?, mode),
            Default::default(),
        ))
    };
    use CreateDirectoryMode::{Recursive, Single};
    let directory = create("/sequential/../destination/", Recursive)?.await?;
    assert_eq!(directory.path(), &StoragePath::parse("destination")?);
    for (operand, mode, rejection, outcome) in [
        (
            "/empty",
            Single,
            Rejection::AlreadyExists,
            MutationOutcome::Unchanged,
        ),
        (
            "/file/",
            Single,
            Rejection::NotDirectory,
            MutationOutcome::Unchanged,
        ),
        (
            "/denied",
            Single,
            Rejection::AccessDenied,
            MutationOutcome::Unchanged,
        ),
        (
            "/visible/denied",
            Recursive,
            Rejection::AccessDenied,
            MutationOutcome::Unknown,
        ),
    ] {
        let error = create(operand, mode)?
            .await
            .err()
            .ok_or("unexpected creation success")?;
        assert_eq!(error.failure(), Failure::Rejected(rejection));
        assert_eq!(error.outcome(), outcome);
    }
    create("/no-validator", Single)?.await?;
    for (operand, rejection) in [
        ("/raced", Rejection::Conflict),
        ("/no-validator/child", Rejection::Unsupported),
    ] {
        let error = create(operand, Single)?
            .await
            .err()
            .ok_or("unbound parent accepted")?;
        assert_eq!(error.failure(), Failure::Rejected(rejection));
        assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    }
    let entries = fixture.entries.lock().map_err(|_| "entries lock")?;
    for directory in ["sequential", "destination", "visible"] {
        assert_eq!(entries.get(directory), Some(&None));
    }
    for path in ["visible/denied", "raced", "no-validator/child"] {
        assert!(!entries.contains_key(path));
    }
    drop(entries);

    let mut lost = storage
        .write_file(
            write("lost-response", WriteMode::CreateNew, Publication::Staged)?,
            Default::default(),
        )
        .await?;
    lost.write_chunk(b"published once").await?;
    let uncertain = lost.commit().await.err().ok_or("response should be lost")?;
    assert_eq!(uncertain.outcome(), MutationOutcome::Unknown);
    assert_eq!(
        uncertain.failure(),
        Failure::Backend(ah_vfs_storage::BackendFailure::Unavailable)
    );
    assert!(matches!(lost.commit().await, Err(error)
        if error.failure() == Failure::Backend(ah_vfs_storage::BackendFailure::Closed)));
    assert!(lost.abort().await.is_err());
    assert_eq!(fixture.lost_puts.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture
            .entries
            .lock()
            .map_err(|_| "entries lock")?
            .get("lost-response")
            .and_then(|value| value.as_deref()),
        Some(b"published once".as_slice())
    );

    assert_eq!(fixture.deletes.load(Ordering::SeqCst), 0);
    Ok(())
}
