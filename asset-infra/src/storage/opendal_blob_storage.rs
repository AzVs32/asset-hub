use crate::config::BlobConfig;
use asset_core::CoreError;
use asset_core::domain::StorageKey;
use asset_core::port::{BlobByteStream, BlobStorage, BlobWriteResult, ScannedBlob};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use opendal::services::Fs;
use opendal::{ErrorKind, Operator};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// 基于 OpenDAL `Operator` 的对象存储适配器。
///
/// 当前默认使用 Fs backend。后续接入 S3 时，可以复用该适配器结构，只替换 `Operator`
/// 的构建方式。
#[derive(Clone)]
pub struct OpenDalBlobStorage {
    operator: Operator,
    fs_root: PathBuf,
}

impl OpenDalBlobStorage {
    /// 使用 OpenDAL `Operator` 创建适配器。
    pub fn new(operator: Operator, fs_root: PathBuf) -> Self {
        Self { operator, fs_root }
    }

    /// 根据 Fs 配置创建对象存储适配器。
    pub fn from_config(config: &BlobConfig) -> Result<Self, CoreError> {
        let root = config.fs_root.to_string_lossy();
        let builder = Fs::default().root(root.as_ref());
        let operator = Operator::new(builder)
            .map_err(|error| CoreError::storage("fs.build", error))?
            .finish();

        Ok(Self::new(operator, config.fs_root.clone()))
    }

    /// 返回内部 OpenDAL `Operator`。
    pub fn operator(&self) -> &Operator {
        &self.operator
    }
}

#[async_trait::async_trait]
impl BlobStorage for OpenDalBlobStorage {
    async fn scan(
        &self,
        directory: &str,
        include_sha256: bool,
        max_entries: usize,
    ) -> Result<Vec<ScannedBlob>, CoreError> {
        let root = self.fs_root.clone();
        let directory = directory.to_owned();
        tokio::task::spawn_blocking(move || {
            scan_files(&root, &directory, include_sha256, max_entries)
        })
        .await
        .map_err(|error| CoreError::configuration(format!("scan task failed: {error}")))?
    }

    async fn put(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError> {
        self.operator
            .write(key.as_str(), data)
            .await
            .map(|_| ())
            .map_err(|error| CoreError::storage("put", error))
    }

    async fn put_if_absent(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError> {
        self.operator
            .write_with(key.as_str(), data)
            .if_not_exists(true)
            .await
            .map(|_| ())
            .map_err(|error| conditional_write_error("put_if_absent", key, error))
    }

    async fn put_stream(
        &self,
        key: &StorageKey,
        mut data: BlobByteStream,
    ) -> Result<BlobWriteResult, CoreError> {
        let mut writer = self
            .operator
            .writer(key.as_str())
            .await
            .map_err(|error| CoreError::storage("put_stream.open", error))?;
        let mut bytes_written = 0_u64;

        while let Some(chunk) = data.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let _ = writer.abort().await;
                    return Err(error);
                }
            };

            bytes_written = bytes_written
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| CoreError::storage("put_stream.size", SizeOverflow))?;

            if let Err(error) = writer.write(chunk).await {
                let _ = writer.abort().await;
                return Err(CoreError::storage("put_stream.write", error));
            }
        }

        writer
            .close()
            .await
            .map_err(|error| CoreError::storage("put_stream.close", error))?;

        Ok(BlobWriteResult::new(bytes_written))
    }

    async fn put_stream_if_absent(
        &self,
        key: &StorageKey,
        data: BlobByteStream,
    ) -> Result<BlobWriteResult, CoreError> {
        put_stream_with_writer(
            self.operator
                .writer_with(key.as_str())
                .if_not_exists(true)
                .await
                .map_err(|error| {
                    conditional_write_error("put_stream_if_absent.open", key, error)
                })?,
            data,
        )
        .await
    }

    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, CoreError> {
        match self.operator.read(key.as_str()).await {
            Ok(buffer) => Ok(Some(buffer.to_bytes())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CoreError::storage("get", error)),
        }
    }

    async fn get_stream(&self, key: &StorageKey) -> Result<Option<BlobByteStream>, CoreError> {
        let reader = match self
            .operator
            .reader_with(key.as_str())
            .chunk(256 * 1024)
            .await
        {
            Ok(reader) => reader,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(CoreError::storage("get_stream.open", error)),
        };
        let stream = reader
            .into_bytes_stream(..)
            .await
            .map_err(|error| CoreError::storage("get_stream.open", error))?
            .map_err(|error| CoreError::storage("get_stream.read", error));

        Ok(Some(Box::pin(stream)))
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), CoreError> {
        self.operator
            .delete(key.as_str())
            .await
            .map_err(|error| CoreError::storage("delete", error))
    }

    async fn exists(&self, key: &StorageKey) -> Result<bool, CoreError> {
        self.operator
            .exists(key.as_str())
            .await
            .map_err(|error| CoreError::storage("exists", error))
    }
}

async fn put_stream_with_writer(
    mut writer: opendal::Writer,
    mut data: BlobByteStream,
) -> Result<BlobWriteResult, CoreError> {
    let mut bytes_written = 0_u64;

    while let Some(chunk) = data.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                let _ = writer.abort().await;
                return Err(error);
            }
        };

        bytes_written = bytes_written
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| CoreError::storage("put_stream.size", SizeOverflow))?;

        if let Err(error) = writer.write(chunk).await {
            let _ = writer.abort().await;
            return Err(CoreError::storage("put_stream.write", error));
        }
    }

    writer
        .close()
        .await
        .map_err(|error| CoreError::storage("put_stream.close", error))?;

    Ok(BlobWriteResult::new(bytes_written))
}

fn conditional_write_error(
    operation: &'static str,
    key: &StorageKey,
    error: opendal::Error,
) -> CoreError {
    if matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::ConditionNotMatch
    ) {
        return CoreError::conflict(format!("storage key `{key}` already exists"));
    }

    CoreError::storage(operation, error)
}

fn scan_files(
    configured_root: &Path,
    directory: &str,
    include_sha256: bool,
    max_entries: usize,
) -> Result<Vec<ScannedBlob>, CoreError> {
    let root = configured_root.canonicalize().map_err(|error| {
        CoreError::configuration(format!("storage root is not readable: {error}"))
    })?;
    let scan_root = root
        .join(directory)
        .canonicalize()
        .map_err(|error| CoreError::configuration(format!("scan path is not readable: {error}")))?;
    if !scan_root.starts_with(&root) || !scan_root.is_dir() {
        return Err(CoreError::configuration(
            "scan path must be a directory inside storage root",
        ));
    }

    let mut files = Vec::new();
    let mut visited = 0;
    collect_files(
        &root,
        &scan_root,
        include_sha256,
        max_entries,
        &mut visited,
        &mut files,
    )?;
    files.sort_by(|left, right| left.key.as_str().cmp(right.key.as_str()));
    Ok(files)
}

fn collect_files(
    root: &Path,
    current: &Path,
    include_sha256: bool,
    max_entries: usize,
    visited: &mut usize,
    files: &mut Vec<ScannedBlob>,
) -> Result<(), CoreError> {
    let entries =
        std::fs::read_dir(current).map_err(|error| CoreError::storage("scan.read_dir", error))?;
    for entry in entries {
        *visited += 1;
        if *visited > max_entries {
            return Err(CoreError::configuration(format!(
                "storage scan exceeds the limit of {max_entries} entries"
            )));
        }
        let entry = entry.map_err(|error| CoreError::storage("scan.read_dir_entry", error))?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| CoreError::storage("scan.metadata", error))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_files(root, &path, include_sha256, max_entries, visited, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .map_err(|_| CoreError::configuration("scanned object path escaped storage root"))?;
        let mut parts = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => {
                    parts.push(part.to_str().ok_or_else(|| {
                        CoreError::configuration("storage path must be valid UTF-8")
                    })?)
                }
                Component::CurDir => {}
                _ => return Err(CoreError::configuration("invalid storage object path")),
            }
        }
        let key = StorageKey::new(parts.join("/"))?;
        files.push(ScannedBlob {
            key,
            size: metadata.len(),
            mime_type: content_type_from_path(&path).map(str::to_owned),
            sha256: include_sha256.then(|| sha256_file(&path)).transpose()?,
        });
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, CoreError> {
    let mut file =
        std::fs::File::open(path).map_err(|error| CoreError::storage("scan.open", error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| CoreError::storage("scan.read", error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn content_type_from_path(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "txt" => Some("text/plain; charset=utf-8"),
        "md" | "markdown" => Some("text/markdown; charset=utf-8"),
        "json" => Some("application/json"),
        "html" | "htm" => Some("text/html; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "js" | "mjs" => Some("text/javascript"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "pdf" => Some("application/pdf"),
        "epub" => Some("application/epub+zip"),
        "mp3" => Some("audio/mpeg"),
        "mp4" => Some("video/mp4"),
        "zip" => Some("application/zip"),
        _ => None,
    }
}

#[derive(Debug)]
struct SizeOverflow;

impl std::fmt::Display for SizeOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stream size exceeds u64::MAX")
    }
}

impl std::error::Error for SizeOverflow {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn fs_storage_roundtrips_blob_content() {
        let storage = storage("fs-roundtrip");
        let key = StorageKey::new("assets/image.png").unwrap();
        let data = Bytes::from_static(b"image bytes");

        storage.put(&key, data.clone()).await.unwrap();

        assert!(storage.exists(&key).await.unwrap());
        assert_eq!(storage.get(&key).await.unwrap(), Some(data));

        storage.delete(&key).await.unwrap();

        assert!(!storage.exists(&key).await.unwrap());
        assert_eq!(storage.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn fs_storage_writes_streaming_blob_content() {
        let storage = storage("fs-stream");
        let key = StorageKey::new("assets/large.bin").unwrap();
        let stream: BlobByteStream = Box::pin(futures_util::stream::iter([
            Ok(Bytes::from_static(b"large ")),
            Ok(Bytes::from_static(b"file ")),
            Ok(Bytes::from_static(b"bytes")),
        ]));

        let result = storage.put_stream(&key, stream).await.unwrap();

        assert_eq!(result.bytes_written(), 16);
        assert_eq!(
            storage.get(&key).await.unwrap(),
            Some(Bytes::from_static(b"large file bytes"))
        );
    }

    #[tokio::test]
    async fn fs_storage_put_if_absent_rejects_existing_blob() {
        let storage = storage("fs-put-if-absent");
        let key = StorageKey::new("assets/image.png").unwrap();

        storage
            .put_if_absent(&key, Bytes::from_static(b"first"))
            .await
            .unwrap();
        let error = storage
            .put_if_absent(&key, Bytes::from_static(b"second"))
            .await
            .unwrap_err();

        match error {
            CoreError::Conflict { message } => assert!(message.contains("already exists")),
            other => panic!("expected conflict, got {other:?}"),
        }
        assert_eq!(
            storage.get(&key).await.unwrap(),
            Some(Bytes::from_static(b"first"))
        );
    }

    #[tokio::test]
    async fn fs_storage_put_stream_if_absent_rejects_existing_blob() {
        let storage = storage("fs-stream-if-absent");
        let key = StorageKey::new("assets/large.bin").unwrap();
        let first: BlobByteStream = Box::pin(futures_util::stream::iter([Ok(Bytes::from_static(
            b"first",
        ))]));
        let second: BlobByteStream = Box::pin(futures_util::stream::iter([Ok(
            Bytes::from_static(b"second"),
        )]));

        storage.put_stream_if_absent(&key, first).await.unwrap();
        let error = storage
            .put_stream_if_absent(&key, second)
            .await
            .unwrap_err();

        match error {
            CoreError::Conflict { message } => assert!(message.contains("already exists")),
            other => panic!("expected conflict, got {other:?}"),
        }
        assert_eq!(
            storage.get(&key).await.unwrap(),
            Some(Bytes::from_static(b"first"))
        );
    }

    fn storage(name: &str) -> OpenDalBlobStorage {
        let root = unique_temp_path(name);
        std::fs::create_dir_all(&root).unwrap();

        OpenDalBlobStorage::from_config(&BlobConfig { fs_root: root }).unwrap()
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
    }
}
