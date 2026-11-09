use ah_vfs::application::dto::*;
use ah_vfs::application::port::*;
use ah_vfs::domain::InstanceId;
use ah_vfs::domain::storage::{
    StorageCreateDirRequest, StorageDirQuery, StorageReadFileRequest, StorageRemoveEntryRequest,
    StorageWriteFileRequest,
};
use ah_vfs_storage::{EntryType, StoragePath};
use ah_vfs_storage::{Failure, Rejection};
use std::{
    collections::BTreeMap,
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
        let observed = deletes.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let entries = entries.clone();
                let deletes = observed.clone();
                tokio::spawn(async move {
                    let _ = handle(stream, entries, deletes).await;
                });
            }
        });
        Ok(Self {
            origin,
            task,
            deletes,
        })
    }
}
fn metadata(path: &str, data: &Option<Vec<u8>>) -> String {
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
    format!(
        "<d:response><d:href>/dav/{path}{suffix}</d:href><d:propstat><d:prop><d:resourcetype>{kind}</d:resourcetype><d:getcontentlength>{}</d:getcontentlength><d:getetag>\"fixture\"</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
        data.as_ref().map_or(0, Vec::len)
    )
}
async fn handle(
    mut stream: tokio::net::TcpStream,
    entries: Entries,
    deletes: Arc<AtomicUsize>,
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
        let mut entries = entries.lock().map_err(|_| "entries lock")?;
        match method {
            "PROPFIND" => match entries.get(path) {
                None => ("404 Not Found", vec![], String::new()),
                Some(value) => {
                    let mut xml = metadata(path, value);
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
                            xml.push_str(&metadata(child, value));
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
                entries.insert(
                    path.into(),
                    Some(bytes[header_end..header_end + length].to_vec()),
                );
                ("201 Created", vec![], String::new())
            }
            "MKCOL" => {
                entries.insert(path.into(), None);
                ("201 Created", vec![], String::new())
            }
            "GET" => {
                let data = entries.get(path).and_then(|v| v.as_ref()).ok_or("file")?;
                let range = fields
                    .get("range")
                    .and_then(|v| v.strip_prefix("bytes="))
                    .ok_or("range")?;
                let (start, end) = range.split_once('-').ok_or("range bounds")?;
                let start: usize = start.parse()?;
                let end: usize = end.parse()?;
                (
                    "206 Partial Content",
                    data[start..=end].to_vec(),
                    format!(
                        "content-range: bytes {start}-{end}/{}\r\netag: \"fixture\"\r\n",
                        data.len()
                    ),
                )
            }
            "DELETE" => {
                deletes.fetch_add(1, Ordering::SeqCst);
                ("500 Internal Server Error", vec![], String::new())
            }
            _ => ("405 Method Not Allowed", vec![], String::new()),
        }
    };
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
        .write_file(StorageWriteFileRequest::new(file.clone()))
        .await?;
    writer.write_chunk(&[0, 255, 1, 2]).await?;
    let entry = writer.commit().await?;
    assert_eq!(entry.entry_type(), ah_vfs_storage::EntryType::file(Some(4)));
    assert_eq!(entry.path(), &file);
    let mut reader = storage
        .read_file(StorageReadFileRequest {
            path: file.clone(),
            offset: 1,
            length: Some(2),
        })
        .await?;
    assert_eq!(reader.next_chunk().await?, Some(vec![255, 1]));
    assert_eq!(reader.next_chunk().await?, None);
    assert_eq!(reader.next_chunk().await?, None);
    let directory = storage
        .create_dir(StorageCreateDirRequest {
            path: StoragePath::parse("empty")?,
            recursive: false,
        })
        .await?;
    assert_eq!(directory.entry_type(), EntryType::Directory);
    let page = storage
        .list_entries(StorageDirQuery::new(StoragePath::root()))
        .await?;
    assert_eq!(page.entries.len(), 2);
    assert_eq!(
        storage
            .remove_entry(StorageRemoveEntryRequest { path: file })
            .await
            .err()
            .ok_or("unsupported removal")?
            .failure(),
        Failure::Rejected(Rejection::Unsupported)
    );
    assert_eq!(fixture.deletes.load(Ordering::SeqCst), 0);
    Ok(())
}
